//! GW-I-045: uploads land in `uploads/` under a sanitised, never-overwriting name, within the per-file and
//! per-workspace limits; nothing is left behind by a refused upload.
use super::*;
use crate::files::path::open_root;
use std::fs;
use std::os::fd::AsFd;
use std::os::unix::fs::symlink;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

const LIMITS: Limits = Limits {
    per_file: 10,
    per_workspace: 25,
};

fn root() -> std::result::Result<(tempfile::TempDir, std::path::PathBuf), std::io::Error> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("workspace");
    fs::create_dir(&root)?;
    Ok((temporary, root))
}

// TEST: GW-I-013
#[test]
fn gw_i_045_stores_under_uploads_and_never_overwrites() -> Result {
    let (_temporary, root) = root()?;
    let fd = open_root(&root)?;
    let first = store(fd.as_fd(), "notes.txt", &[b"hello".to_vec()], LIMITS)?;
    assert_eq!((first.path.as_str(), first.size), ("uploads/notes.txt", 5));
    let second = store(fd.as_fd(), "notes.txt", &[b"again".to_vec()], LIMITS)?;
    assert_eq!(second.path, "uploads/notes-2.txt");
    let third = store(fd.as_fd(), "notes.txt", &[b"x".to_vec()], LIMITS)?;
    assert_eq!(third.path, "uploads/notes-3.txt");
    assert_eq!(fs::read_to_string(root.join("uploads/notes.txt"))?, "hello");
    assert_eq!(
        fs::read_to_string(root.join("uploads/notes-2.txt"))?,
        "again"
    );
    // No temporary files remain.
    let mut names: Vec<String> = fs::read_dir(root.join("uploads"))?
        .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<std::result::Result<_, _>>()?;
    names.sort();
    assert_eq!(names, ["notes-2.txt", "notes-3.txt", "notes.txt"]);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_an_oversize_file_is_refused_and_leaves_nothing() -> Result {
    let (_temporary, root) = root()?;
    let fd = open_root(&root)?;
    let result = store(fd.as_fd(), "big.bin", &[vec![0; 6], vec![0; 5]], LIMITS);
    assert_eq!(result.err(), Some(GatewayError::UploadTooLarge));
    // Refused before anything is created: no file and no directory.
    assert!(!root.join("uploads").exists());
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_the_workspace_upload_space_is_bounded() -> Result {
    let (_temporary, root) = root()?;
    let fd = open_root(&root)?;
    store(fd.as_fd(), "a.bin", &[vec![0; 10]], LIMITS)?;
    store(fd.as_fd(), "b.bin", &[vec![0; 10]], LIMITS)?;
    let result = store(fd.as_fd(), "c.bin", &[vec![0; 6]], LIMITS);
    assert_eq!(result.err(), Some(GatewayError::Conflict("uploads_full")));
    assert!(!root.join("uploads/c.bin").exists());
    assert_eq!(used(fd.as_fd())?, 20);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_an_uploads_link_planted_by_the_agent_is_refused() -> Result {
    let (temporary, root) = root()?;
    let outside = temporary.path().join("outside");
    fs::create_dir(&outside)?;
    symlink(&outside, root.join("uploads"))?;
    let fd = open_root(&root)?;
    let result = store(fd.as_fd(), "x.txt", &[b"x".to_vec()], LIMITS);
    assert_eq!(result.err(), Some(GatewayError::PathInvalid));
    assert_eq!(fs::read_dir(&outside)?.count(), 0);
    Ok(())
}
