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

fn media_type(file: &mut File) -> Result<ImageMediaType, ImageError> {
    let mut header = Vec::with_capacity(IMAGE_HEADER_BYTES);
    file.by_ref()
        .take(IMAGE_HEADER_BYTES as u64)
        .read_to_end(&mut header)
        .map_err(|_| ImageError::Unreadable)?;
    ImageMediaType::from_header(&header).ok_or(ImageError::NotImage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

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
