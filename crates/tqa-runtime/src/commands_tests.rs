//! RT-I-036: which of the usual commands the runtime finds on its PATH, for the system prompt's Commands line.
use super::{USUAL, available};
use std::{ffi::OsString, os::unix::fs::PermissionsExt, path::Path};

fn executable(directory: &Path, name: &str) -> std::io::Result<()> {
    let path = directory.join(name);
    std::fs::write(&path, "#!/bin/sh\n")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
}

// TEST: RT-I-036
#[test]
fn rt_i_036_the_runtime_names_the_usual_commands_on_its_path() -> std::io::Result<()> {
    let first = tempfile::tempdir()?;
    let second = tempfile::tempdir()?;
    executable(first.path(), "python3")?;
    std::os::unix::fs::symlink("python3", first.path().join("python"))?; // as the runtime image links it
    executable(second.path(), "git")?;
    executable(second.path(), "node")?;
    std::fs::write(second.path().join("go"), "not executable")?;
    std::fs::create_dir(second.path().join("npm"))?; // a directory named like a command is none
    executable(second.path(), "not-a-usual-command")?;
    let path = std::env::join_paths([first.path(), second.path(), Path::new("/nonexistent")])
        .map_err(std::io::Error::other)?;
    assert_eq!(
        available(&path),
        ["python3", "python", "node", "git"],
        "in the usual list's order"
    );
    assert!(available(&OsString::new()).is_empty());
    assert!(
        USUAL.len() <= 32,
        "a bounded list: the prompt line stays short"
    );
    Ok(())
}
