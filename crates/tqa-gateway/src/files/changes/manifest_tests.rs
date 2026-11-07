//! GW-I-048 snapshots, gateway side: the shared walker (tested in `tqa-tools`) stores its copies as blobs.
use super::*;
use std::fs;
use std::os::fd::AsFd;
use std::os::unix::fs::symlink;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

pub(crate) fn limits() -> Limits {
    Limits {
        files: 100,
        entries: 1000,
        text_bytes: 64,
        copy_bytes: 1024,
        deadline: None,
    }
}

pub(crate) fn tree()
-> std::result::Result<(tempfile::TempDir, std::path::PathBuf, Blobs), std::io::Error> {
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
    for hidden in [
        ".tqa/state.json",
        "uploads/a.csv",
        ".git/HEAD",
        "web/node_modules/x/i.js",
        "src/.git/HEAD",
    ] {
        fs::write(root.join(hidden), "hidden")?;
    }
    symlink("/etc/passwd", root.join("src/link"))?;
    symlink("/etc", root.join("escape"))?;
    let blobs = Blobs::new(temporary.path().join("changes/blobs"));
    Ok((temporary, root, blobs))
}

pub(crate) fn scan_at(
    root: &std::path::Path,
    previous: Option<&Manifest>,
    limits: &Limits,
    blobs: &Blobs,
) -> std::result::Result<Manifest, GatewayError> {
    let fd = crate::files::path::open_root(root)?;
    scan(fd.as_fd(), previous, limits, blobs)
}

// TEST: GW-I-013
#[test]
fn gw_i_048_copies_go_to_the_blob_store_by_their_hash() -> Result {
    let (_temporary, root, blobs) = tree()?;
    let manifest = scan_at(&root, None, &limits(), &blobs)?;
    let app = &manifest.files["src/app.py"];
    assert!(app.copied);
    assert_eq!(
        blobs.get(app.hash.as_deref().ok_or("hash")?)?,
        b"print('hi')\n"
    );
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_a_linked_root_is_refused_with_its_http_outcome() -> Result {
    let (temporary, root, blobs) = tree()?;
    let linked = temporary.path().join("linked");
    symlink(&root, &linked)?;
    assert!(matches!(
        scan_at(&linked, None, &limits(), &blobs),
        Err(GatewayError::PathInvalid | GatewayError::NotFound)
    ));
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_blobs_are_content_addressed_and_pruned_by_reference() -> Result {
    let temporary = tempfile::tempdir()?;
    let blobs = Blobs::new(temporary.path().join("blobs"));
    let first = blobs.put(b"one")?;
    assert_eq!(blobs.put(b"one")?, first);
    let second = blobs.put(b"two")?;
    blobs.prune(&[first.clone()].into_iter().collect())?;
    assert_eq!(blobs.get(&first)?, b"one");
    assert!(blobs.get(&second).is_err());
    assert!(blobs.get("../escape").is_err());
    Ok(())
}
