//! Confined, bounded reads of attached images (TOOL-U-021).
use crate::WorkspaceRoot;
use nix::{
    fcntl::{self, OFlag},
    sys::stat::Mode,
};
use std::{
    fs::File,
    io::{Read, Seek},
};
use tqa_types::{IMAGE_HEADER_BYTES, ImageMediaType, MAX_IMAGE_BYTES, validate_attachments};

/// The directory, relative to the workspace root, that holds uploads.
const UPLOADS: &str = "uploads";
const UPLOADS_PREFIX: &str = "uploads/";

/// A readable attached image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageData {
    /// The format recognised from the bytes.
    pub media_type: ImageMediaType,
    /// The whole file, at most [`MAX_IMAGE_BYTES`].
    pub bytes: Vec<u8>,
}

/// Why an attachment is not sent as an image. Errors carry no path or file content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ImageError {
    /// The file is readable but is not a supported image format.
    #[error("not an image")]
    NotImage,
    /// An iPhone-style HEIC or HEIF photo: a picture no model reads, which the agent can convert itself.
    #[error("heic photo")]
    Heic,
    /// The image is larger than [`MAX_IMAGE_BYTES`].
    #[error("too large")]
    TooLarge,
    /// The path is not an upload path, is missing, is not a regular file, or is a symlink.
    #[error("unreadable")]
    Unreadable,
}

/// Recognises an attached image from its first bytes without reading the rest.
///
/// # Errors
/// `Unreadable` for any path that is not a confined regular file; `NotImage` otherwise.
pub fn probe_image(workspace: &WorkspaceRoot, path: &str) -> Result<ImageMediaType, ImageError> {
    let mut file = open(workspace, path, &mut || ())?.0;
    media_type(&mut file)
}

/// Reads a whole attached image of at most [`MAX_IMAGE_BYTES`].
///
/// # Errors
/// `Unreadable`, `NotImage` or `TooLarge`, checked in that order; a file that grows while it is
/// read is also `TooLarge`.
pub fn read_image(workspace: &WorkspaceRoot, path: &str) -> Result<ImageData, ImageError> {
    read_image_with(workspace, path, &mut || ())
}

/// `read_image`, with `between` called after the `uploads` directory is checked and before the
/// file is opened, so a test can change the directory at that point.
fn read_image_with(
    workspace: &WorkspaceRoot,
    path: &str,
    between: &mut dyn FnMut(),
) -> Result<ImageData, ImageError> {
    let (mut file, size) = open(workspace, path, between)?;
    let media_type = media_type(&mut file)?;
    if size > MAX_IMAGE_BYTES as u64 {
        return Err(ImageError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.rewind()
        .and_then(|()| {
            file.by_ref()
                .take(MAX_IMAGE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| ImageError::Unreadable)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageError::TooLarge);
    }
    Ok(ImageData { media_type, bytes })
}

/// Opens `uploads/<name>` by walking from the workspace root without following links: the
/// directory with `O_DIRECTORY | O_NOFOLLOW`, then the file relative to that directory with
/// `O_NOFOLLOW | O_NONBLOCK` (a FIFO cannot block), then checks the opened descriptor is a regular
/// file. Swapping a component for a symlink after any step cannot redirect the read.
fn open(
    workspace: &WorkspaceRoot,
    path: &str,
    between: &mut dyn FnMut(),
) -> Result<(File, u64), ImageError> {
    validate_attachments(&[path.to_owned()]).map_err(|_| ImageError::Unreadable)?;
    let name = path
        .strip_prefix(UPLOADS_PREFIX)
        .ok_or(ImageError::Unreadable)?;
    let directory = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC;
    let root = fcntl::open(workspace.path(), directory, Mode::empty())
        .map_err(|_| ImageError::Unreadable)?;
    let uploads = fcntl::openat(&root, UPLOADS, directory | OFlag::O_NOFOLLOW, Mode::empty())
        .map_err(|_| ImageError::Unreadable)?;
    between();
    let flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC;
    let file = File::from(
        fcntl::openat(&uploads, name, flags, Mode::empty()).map_err(|_| ImageError::Unreadable)?,
    );
    let opened = file.metadata().map_err(|_| ImageError::Unreadable)?;
    if !opened.is_file() {
        return Err(ImageError::Unreadable);
    }
    Ok((file, opened.len()))
}

/// Reads a picture anywhere in the workspace that the agent read with `read_file` (TOOL-U-143): `path` is relative to
/// the workspace root, each directory is opened with `O_NOFOLLOW` from the root down and the file with
/// `O_NOFOLLOW | O_NONBLOCK`, so no link leads out; then the same format and size rules as an attached image.
///
/// # Errors
/// `Unreadable` for a path that climbs, is absolute, names a link, a missing or non-regular file; `NotImage` or
/// `TooLarge` as for an attachment.
pub fn read_workspace_image(
    workspace: &WorkspaceRoot,
    path: &str,
) -> Result<ImageData, ImageError> {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if path.starts_with('/') || parts.is_empty() || parts.contains(&"..") {
        return Err(ImageError::Unreadable);
    }
    let directory = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC;
    let mut dir = fcntl::open(workspace.path(), directory, Mode::empty())
        .map_err(|_| ImageError::Unreadable)?;
    let (name, folders) = parts.split_last().ok_or(ImageError::Unreadable)?;
    for folder in folders {
        dir = fcntl::openat(&dir, *folder, directory | OFlag::O_NOFOLLOW, Mode::empty())
            .map_err(|_| ImageError::Unreadable)?;
    }
    let flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC;
    let mut file = File::from(
        fcntl::openat(&dir, *name, flags, Mode::empty()).map_err(|_| ImageError::Unreadable)?,
    );
    let size = file.metadata().map_err(|_| ImageError::Unreadable)?;
    if !size.is_file() {
        return Err(ImageError::Unreadable);
    }
    let media_type = media_type(&mut file)?;
    if size.len() > MAX_IMAGE_BYTES as u64 {
        return Err(ImageError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.rewind()
        .and_then(|()| {
            file.by_ref()
                .take(MAX_IMAGE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| ImageError::Unreadable)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageError::TooLarge);
    }
    Ok(ImageData { media_type, bytes })
}

fn media_type(file: &mut File) -> Result<ImageMediaType, ImageError> {
    let mut header = Vec::with_capacity(IMAGE_HEADER_BYTES);
    file.by_ref()
        .take(IMAGE_HEADER_BYTES as u64)
        .read_to_end(&mut header)
        .map_err(|_| ImageError::Unreadable)?;
    ImageMediaType::from_header(&header).ok_or_else(|| {
        if heif(&header) {
            ImageError::Heic
        } else {
            ImageError::NotImage
        }
    })
}

/// Whether `header` starts an ISO media file of a HEIC or HEIF still picture (its `ftyp` box's major brand).
fn heif(header: &[u8]) -> bool {
    header.get(4..8) == Some(b"ftyp".as_slice())
        && header.get(8..12).is_some_and(|brand| {
            [
                b"heic", b"heix", b"hevc", b"heim", b"heis", b"mif1", b"msf1",
            ]
            .iter()
            .any(|known| brand == known.as_slice())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    // TEST: TOOL-U-143
    #[test]
    fn tool_u_143_a_picture_anywhere_in_the_workspace_is_read_and_nothing_leads_out()
    -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        std::fs::create_dir_all(workspace.path().join("site/img"))?;
        std::fs::write(
            workspace.path().join("site/img/a.png"),
            b"\x89PNG\r\n\x1a\nINSIDE",
        )?;
        std::fs::write(workspace.path().join("notes.txt"), b"plain words")?;
        std::fs::write(outside.path().join("b.png"), b"\x89PNG\r\n\x1a\nOUTSIDE")?;
        symlink(outside.path(), workspace.path().join("out"))?;
        symlink(
            outside.path().join("b.png"),
            workspace.path().join("site/link.png"),
        )?;
        let root = WorkspaceRoot::open(workspace.path()).map_err(|error| error.to_string())?;
        let image = read_workspace_image(&root, "site/img/a.png")?;
        assert_eq!(image.media_type, ImageMediaType::Png);
        assert!(image.bytes.ends_with(b"INSIDE"));
        assert_eq!(
            read_workspace_image(&root, "./site/img/a.png")?.bytes,
            image.bytes
        );
        for path in [
            "out/b.png",
            "site/link.png",
            "../b.png",
            "/etc/hostname",
            "site/img",
            "",
            "missing.png",
        ] {
            assert_eq!(
                read_workspace_image(&root, path),
                Err(ImageError::Unreadable),
                "{path}"
            );
        }
        assert_eq!(
            read_workspace_image(&root, "notes.txt"),
            Err(ImageError::NotImage)
        );
        Ok(())
    }

    #[test]
    fn tool_u_021_uploads_swapped_for_a_symlink_after_the_check_is_never_followed()
    -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        std::fs::create_dir(workspace.path().join("uploads"))?;
        std::fs::write(
            workspace.path().join("uploads/a.png"),
            b"\x89PNG\r\n\x1a\nINSIDE",
        )?;
        std::fs::write(outside.path().join("a.png"), b"\x89PNG\r\n\x1a\nOUTSIDE")?;
        let root = WorkspaceRoot::open(workspace.path()).map_err(|error| error.to_string())?;
        let (uploads, moved) = (
            workspace.path().join("uploads"),
            workspace.path().join("moved"),
        );
        let mut swap = || {
            let swapped =
                std::fs::rename(&uploads, &moved).and_then(|()| symlink(outside.path(), &uploads));
            assert!(swapped.is_ok(), "swap failed: {swapped:?}");
        };
        match read_image_with(&root, "uploads/a.png", &mut swap) {
            Ok(image) => assert!(
                image.bytes.ends_with(b"INSIDE"),
                "read outside the workspace"
            ),
            Err(error) => assert_eq!(error, ImageError::Unreadable),
        }
        Ok(())
    }
}
