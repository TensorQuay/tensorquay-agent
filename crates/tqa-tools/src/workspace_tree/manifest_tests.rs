//! GW-I-048 snapshots (the walker, shared by the gateway and the runtime): what a workspace manifest records, copies and leaves out, within its bounds.
use super::*;
use std::fs;
use std::os::unix::fs::symlink;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

fn limits() -> Limits {
    Limits {
        files: 100,
        entries: 1000,
        text_bytes: 64,
        copy_bytes: 1024,
        deadline: None,
    }
}

fn tree() -> std::result::Result<(tempfile::TempDir, std::path::PathBuf), std::io::Error> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("workspace");
    for directory in [
        "src",
        ".tqa",
        "uploads",
        ".git",
        "web/node_modules/x",
        "src/.git",
    ] {
        fs::create_dir_all(root.join(directory))?;
    }
    fs::write(root.join("README.md"), "# Title\n")?;
    fs::write(root.join("src/app.py"), "print('hi')\n")?;
    fs::write(root.join("src/data.bin"), [0_u8, 1, 2, 3])?;
    fs::write(root.join("src/big.txt"), "x".repeat(65))?;
    // The person's uploads are walked (an edit to one is the run's change); an upload still being written is not.
    fs::write(root.join("uploads/a.csv"), "a,b\n")?;
    for hidden in [
        ".tqa/state.json",
        "uploads/.upload-1",
        ".git/HEAD",
        "web/node_modules/x/i.js",
        "src/.git/HEAD",
    ] {
        fs::write(root.join(hidden), "hidden")?;
    }
    symlink("/etc/passwd", root.join("src/link"))?;
    symlink("/etc", root.join("escape"))?;
    Ok((temporary, root))
}

/// Scans `root`, returning the manifest and every byte string handed to the store, in order.
fn scan_at(
    root: &std::path::Path,
    previous: Option<&Manifest>,
    limits: &Limits,
) -> std::result::Result<(Manifest, Vec<Vec<u8>>), TreeError> {
    let fd = path::open_root(root)?;
    let mut stored = Vec::new();
    let manifest = scan(fd.as_fd(), previous, limits, &mut |bytes: &[u8]| {
        stored.push(bytes.to_vec());
        Ok::<(), TreeError>(())
    })?;
    Ok((manifest, stored))
}

// TEST: GW-I-013
#[test]
fn gw_i_048_records_files_and_leaves_out_private_generated_and_linked_trees() -> Result {
    let (_temporary, root) = tree()?;
    let manifest = scan_at(&root, None, &limits())?.0;
    assert_eq!(
        manifest
            .files
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "README.md",
            "src/app.py",
            "src/big.txt",
            "src/data.bin",
            "uploads/a.csv"
        ]
    );
    assert!(!manifest.truncated);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_copies_small_text_only() -> Result {
    let (_temporary, root) = tree()?;
    let (manifest, stored) = scan_at(&root, None, &limits())?;
    let app = &manifest.files["src/app.py"];
    assert!(app.text && app.copied);
    let hash = app.hash.as_deref().ok_or("hash")?;
    assert_eq!(hash, hex(&Sha256::digest(b"print('hi')\n")));
    assert!(
        stored.contains(&b"print('hi')\n".to_vec()),
        "the store received the copied text"
    );
    let binary = &manifest.files["src/data.bin"];
    assert!(!binary.text && !binary.copied && binary.hash.is_some());
    let big = &manifest.files["src/big.txt"];
    assert!(!big.copied && big.hash.is_none());
    assert_eq!(big.size, 65);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_unchanged_files_reuse_the_previous_record() -> Result {
    let (_temporary, root) = tree()?;
    let mut previous = scan_at(&root, None, &limits())?.0;
    // A record whose stamp still matches is trusted without reading the file again.
    previous.files.get_mut("README.md").ok_or("readme")?.hash = Some("reused".to_owned());
    let next = scan_at(&root, Some(&previous), &limits())?.0;
    assert_eq!(next.files["README.md"].hash.as_deref(), Some("reused"));
    fs::write(root.join("README.md"), "# Changed title\n")?;
    let changed = scan_at(&root, Some(&previous), &limits())?.0;
    assert_ne!(changed.files["README.md"].hash.as_deref(), Some("reused"));
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_bounds_mark_a_snapshot_truncated_or_stop_copying() -> Result {
    let (_temporary, root) = tree()?;
    let few = Limits {
        files: 2,
        ..limits()
    };
    let manifest = scan_at(&root, None, &few)?.0;
    assert_eq!(manifest.files.len(), 2);
    assert!(manifest.truncated);
    let budget = Limits {
        copy_bytes: 8,
        ..limits()
    };
    let manifest = scan_at(&root, None, &budget)?.0;
    let copied: Vec<_> = manifest
        .files
        .iter()
        .filter(|(_, record)| record.copied)
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(copied, ["README.md"]);
    assert!(manifest.files["src/app.py"].text);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_a_linked_root_is_refused() -> Result {
    let (temporary, root) = tree()?;
    let linked = temporary.path().join("linked");
    symlink(&root, &linked)?;
    // Refused, never walked through: ELOOP (path_invalid) on Linux, ENOTDIR (404) on macOS for a directory open.
    assert!(matches!(
        scan_at(&linked, None, &limits()),
        Err(TreeError::PathInvalid | TreeError::NotFound)
    ));
    Ok(())
}

// Review of 068c: the agent controls the workspace, so the walk is bounded in work, not only in recorded files.
// TEST: GW-I-013
#[test]
fn gw_i_048_many_empty_directories_stop_at_the_entry_budget() -> Result {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("workspace");
    for index in 0..300 {
        fs::create_dir_all(root.join(format!("d{index:03}")))?;
    }
    fs::write(root.join("zz.txt"), "last\n")?;
    let bounded = Limits {
        entries: 100,
        ..limits()
    };
    let manifest = scan_at(&root, None, &bounded)?.0;
    assert!(manifest.truncated);
    assert!(manifest.files.is_empty());
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_a_directory_of_links_stops_at_the_entry_budget() -> Result {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("workspace");
    fs::create_dir_all(root.join("links"))?;
    for index in 0..300 {
        symlink("/etc/passwd", root.join(format!("links/l{index:03}")))?;
    }
    let bounded = Limits {
        entries: 100,
        ..limits()
    };
    assert!(scan_at(&root, None, &bounded)?.0.truncated);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_a_passed_deadline_stops_the_walk() -> Result {
    let (_temporary, root) = tree()?;
    let late = Limits {
        deadline: Some(std::time::Instant::now()),
        ..limits()
    };
    let manifest = scan_at(&root, None, &late)?.0;
    assert!(manifest.truncated);
    assert!(manifest.files.is_empty());
    Ok(())
}

// Review of 068c (walk-through): generated caches are noise in every task's changes.
// TEST: GW-I-013
#[test]
fn gw_i_048_well_known_caches_are_left_out_at_any_depth() -> Result {
    let (_temporary, root) = tree()?;
    for cache in [
        "calc/__pycache__",
        ".pytest_cache/v",
        "src/.mypy_cache",
        ".ruff_cache",
        ".tox/py311",
        ".venv/lib",
    ] {
        fs::create_dir_all(root.join(cache))?;
        fs::write(root.join(cache).join("entry"), "generated")?;
    }
    fs::write(
        root.join("calc/ops.py"),
        "def add(a, b):\n    return a + b\n",
    )?;
    let manifest = scan_at(&root, None, &limits())?.0;
    assert_eq!(
        manifest
            .files
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "README.md",
            "calc/ops.py",
            "src/app.py",
            "src/big.txt",
            "src/data.bin",
            "uploads/a.csv"
        ]
    );
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_a_store_failure_ends_the_walk_with_its_error() -> Result {
    let (_temporary, root) = tree()?;
    let fd = path::open_root(&root)?;
    let mut calls = 0;
    let failed = scan(fd.as_fd(), None, &limits(), &mut |_: &[u8]| {
        calls += 1;
        Err(TreeError::Io)
    });
    assert_eq!(failed, Err(TreeError::Io));
    assert_eq!(calls, 1, "the walk stops at the first failed store");
    Ok(())
}

#[test]
fn tree_errors_have_stable_codes() {
    assert_eq!(TreeError::PathInvalid.code(), "path_invalid");
    assert_eq!(TreeError::NotFound.code(), "not_found");
    assert_eq!(TreeError::Io.code(), "io");
    assert_eq!(hex(&[0, 15, 255]), "000fff");
}
