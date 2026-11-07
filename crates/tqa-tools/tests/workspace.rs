//! TOOL-S-002: workspace resolution rejects every escape before a file operation.

use proptest::prelude::*;
use std::{error::Error, fs};
use tqa_tools::{ToolError, WorkspaceRoot};

#[test]
fn tool_s_002_rejects_invalid_roots() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    assert!(WorkspaceRoot::open("relative").is_err());
    assert!(WorkspaceRoot::open(directory.path().join("missing")).is_err());
    let file = directory.path().join("file");
    fs::write(&file, "text")?;
    assert!(WorkspaceRoot::open(file).is_err());
    Ok(())
}

#[test]
fn tool_s_002_rejects_invalid_and_outside_paths() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let root = WorkspaceRoot::open(directory.path())?;
    for path in [
        String::new(),
        "nul\0suffix".to_owned(),
        "x".repeat(5000),
        std::iter::repeat_n("x", 65).collect::<Vec<_>>().join("/"),
    ] {
        assert!(
            matches!(root.resolve(&path), Err(ToolError::PathInvalid(_))),
            "{path:?}"
        );
    }
    for path in [
        "../x".to_owned(),
        "dir/../../x".to_owned(),
        outside.path().display().to_string(),
    ] {
        assert!(
            matches!(root.resolve(&path), Err(ToolError::PathOutsideWorkspace)),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn tool_s_002_resolves_existing_prefix_and_new_directories() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let root = WorkspaceRoot::open(directory.path())?;
    let canonical = directory.path().canonicalize()?;
    fs::create_dir(canonical.join("existing"))?;
    assert_eq!(
        root.resolve("existing/new/file")?,
        canonical.join("existing/new/file")
    );
    assert_eq!(root.resolve("missing/../file")?, canonical.join("file"));
    assert_eq!(root.resolve("./existing/../file")?, canonical.join("file"));
    assert_eq!(
        root.resolve(canonical.join("file").to_str().ok_or("unicode path")?)?,
        canonical.join("file")
    );
    let maximum = std::iter::repeat_n("x", 64).collect::<Vec<_>>().join("/");
    assert!(root.resolve(&maximum)?.starts_with(&canonical));
    Ok(())
}

#[cfg(unix)]
#[test]
fn tool_s_002_symlinks_cannot_escape_or_hide_a_parent_traversal() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let root = WorkspaceRoot::open(directory.path())?;
    fs::create_dir(directory.path().join("nested"))?;
    symlink(outside.path(), directory.path().join("escape"))?;
    symlink(
        directory.path().join("nested"),
        directory.path().join("inside"),
    )?;
    symlink(
        outside.path().join("missing"),
        directory.path().join("dangling"),
    )?;
    for path in ["escape", "escape/file", "escape/../file"] {
        assert!(
            matches!(root.resolve(path), Err(ToolError::PathOutsideWorkspace)),
            "{path}"
        );
    }
    assert!(root.resolve("dangling/file").is_err());
    assert_eq!(
        root.resolve("inside/new")?,
        directory.path().canonicalize()?.join("nested/new")
    );
    assert_eq!(
        root.resolve("inside/../new")?,
        directory.path().canonicalize()?.join("new")
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, rng_seed: proptest::test_runner::RngSeed::Fixed(7), ..ProptestConfig::default() })]
    #[test]
    fn tool_s_002_resolved_random_paths_remain_inside(parts in prop::collection::vec("[a-z.]{0,8}", 0..80), absolute in any::<bool>()) {
        let directory = tempfile::tempdir()?;
        let root = WorkspaceRoot::open(directory.path())?;
        let path = format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"));
        if let Ok(resolved) = root.resolve(&path) {
            prop_assert!(resolved.starts_with(directory.path().canonicalize()?));
        }
    }
}

#[test]
fn tool_s_002_non_directory_prefix_fails_and_dot_names_are_valid() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let root = WorkspaceRoot::open(directory.path())?;
    fs::write(directory.path().join("file"), "text")?;
    assert!(root.resolve("file/child").is_err());
    assert_eq!(root.resolve(".")?, directory.path().canonicalize()?);
    assert_eq!(
        root.resolve(".hidden")?,
        directory.path().canonicalize()?.join(".hidden")
    );
    Ok(())
}

#[test]
fn tool_s_002_cloned_roots_keep_confinement_and_safe_diagnostics() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    fs::write(
        directory.path().join("file"),
        "fixture confidential contents",
    )?;
    let root = WorkspaceRoot::open(directory.path())?;
    let cloned = root.clone();
    assert_eq!(cloned.resolve("file")?, root.resolve("file")?);
    assert!(matches!(
        cloned.resolve("../file"),
        Err(ToolError::PathOutsideWorkspace)
    ));
    let diagnostic = format!("{cloned:?}");
    assert!(diagnostic.contains("WorkspaceRoot"));
    assert!(!diagnostic.contains("fixture confidential contents"));
    Ok(())
}

#[test]
fn tool_u_026_space_is_exhausted_only_when_the_filesystem_says_no_blocks_are_available()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let root = WorkspaceRoot::open(directory.path())?;
    assert!(
        !root.space_exhausted(),
        "a temporary directory has free space"
    );
    // procfs reports no available blocks to anyone: the same answer a full workspace filesystem gives.
    assert!(WorkspaceRoot::open("/proc")?.space_exhausted());
    // An unanswerable query never claims a full workspace.
    let gone = directory.path().join("gone");
    fs::create_dir(&gone)?;
    let removed = WorkspaceRoot::open(&gone)?;
    fs::remove_dir(&gone)?;
    assert!(!removed.space_exhausted());
    Ok(())
}
