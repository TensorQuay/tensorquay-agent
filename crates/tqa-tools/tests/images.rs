//! TOOL-U-021: attached images are read only as confined regular files, typed by their bytes.
use std::{error::Error, fs, os::unix::fs::symlink, path::Path, process::Command};
use tqa_tools::{ImageError, WorkspaceRoot, probe_image, read_image};
use tqa_types::{ImageMediaType, MAX_IMAGE_BYTES};

type TestResult = Result<(), Box<dyn Error>>;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n rest";
const JPEG: &[u8] = b"\xff\xd8\xff\xe0 rest";
const GIF87: &[u8] = b"GIF87a rest";
const GIF89: &[u8] = b"GIF89a rest";
const WEBP: &[u8] = b"RIFF\x10\x00\x00\x00WEBPVP8 rest";

fn workspace() -> Result<(tempfile::TempDir, WorkspaceRoot), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    fs::create_dir(directory.path().join("uploads"))?;
    let root = WorkspaceRoot::open(directory.path())?;
    Ok((directory, root))
}
fn write(root: &Path, name: &str, bytes: &[u8]) -> Result<(), std::io::Error> {
    fs::write(root.join("uploads").join(name), bytes)
}

#[test]
fn tool_u_021_media_type_comes_from_the_bytes_not_the_name() -> TestResult {
    let (directory, root) = workspace()?;
    for (name, bytes, expected) in [
        ("photo.txt", PNG, ImageMediaType::Png),
        ("photo.png", JPEG, ImageMediaType::Jpeg),
        ("old", GIF87, ImageMediaType::Gif),
        ("new.jpg", GIF89, ImageMediaType::Gif),
        ("image.gif", WEBP, ImageMediaType::Webp),
    ] {
        write(directory.path(), name, bytes)?;
        let path = format!("uploads/{name}");
        let image = read_image(&root, &path)?;
        assert_eq!(image.media_type, expected, "{name}");
        assert_eq!(image.bytes, bytes);
        assert_eq!(probe_image(&root, &path)?, expected);
    }
    Ok(())
}

#[test]
fn tool_u_021_non_images_are_reported_as_not_images() -> TestResult {
    let (directory, root) = workspace()?;
    for (name, bytes) in [
        ("notes.png", b"plain text, not an image".as_slice()),
        ("empty.png", b"".as_slice()),
        ("short.png", b"\x89PN".as_slice()),
        ("riff.webp", b"RIFF\x10\x00\x00\x00WAVEfmt ".as_slice()),
    ] {
        write(directory.path(), name, bytes)?;
        let path = format!("uploads/{name}");
        assert_eq!(
            read_image(&root, &path).err(),
            Some(ImageError::NotImage),
            "{name}"
        );
        assert_eq!(
            probe_image(&root, &path).err(),
            Some(ImageError::NotImage),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn tool_u_021_size_bound_is_inclusive() -> TestResult {
    let (directory, root) = workspace()?;
    let mut bytes = PNG.to_vec();
    bytes.resize(MAX_IMAGE_BYTES, 0);
    write(directory.path(), "exact.png", &bytes)?;
    assert_eq!(
        read_image(&root, "uploads/exact.png")?.bytes.len(),
        MAX_IMAGE_BYTES
    );
    bytes.push(0);
    write(directory.path(), "over.png", &bytes)?;
    assert_eq!(
        read_image(&root, "uploads/over.png").err(),
        Some(ImageError::TooLarge)
    );
    // A probe reads only the header, so an oversized image is still recognised as one.
    assert_eq!(probe_image(&root, "uploads/over.png")?, ImageMediaType::Png);
    Ok(())
}

#[test]
fn tool_u_021_missing_irregular_and_symlinked_paths_are_refused() -> TestResult {
    let (directory, root) = workspace()?;
    let uploads = directory.path().join("uploads");
    let outside = tempfile::tempdir()?;
    fs::write(outside.path().join("secret.png"), PNG)?;
    fs::write(directory.path().join("inside.png"), PNG)?;
    fs::create_dir(uploads.join("folder.png"))?;
    symlink(
        directory.path().join("inside.png"),
        uploads.join("inside-link.png"),
    )?;
    symlink(
        outside.path().join("secret.png"),
        uploads.join("outside-link.png"),
    )?;
    let status = Command::new("mkfifo")
        .arg(uploads.join("pipe.png"))
        .status()?;
    assert!(status.success());
    for path in [
        "uploads/missing.png",
        "uploads/folder.png",
        "uploads/inside-link.png",
        "uploads/outside-link.png",
        "uploads/pipe.png",
        "inside.png",
        "uploads/../inside.png",
        "/workspace/uploads/inside-link.png",
        "",
    ] {
        assert_eq!(
            read_image(&root, path).err(),
            Some(ImageError::Unreadable),
            "{path}"
        );
        assert_eq!(
            probe_image(&root, path).err(),
            Some(ImageError::Unreadable),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn tool_u_021_a_symlinked_uploads_directory_is_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let elsewhere = directory.path().join("elsewhere");
    fs::create_dir(&elsewhere)?;
    fs::write(elsewhere.join("a.png"), PNG)?;
    symlink(&elsewhere, directory.path().join("uploads"))?;
    let root = WorkspaceRoot::open(directory.path())?;
    assert_eq!(
        read_image(&root, "uploads/a.png").err(),
        Some(ImageError::Unreadable)
    );
    Ok(())
}
