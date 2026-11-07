use super::*;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

const WIDE: Limits = Limits {
    bytes: 1 << 20,
    entries: 100,
};

fn tree() -> std::result::Result<(tempfile::TempDir, tempfile::TempDir), Box<dyn std::error::Error>>
{
    let source = tempfile::tempdir()?;
    let root = source.path();
    std::fs::write(root.join("a.txt"), "alpha")?;
    std::fs::create_dir_all(root.join("nested/deeper"))?;
    std::fs::write(root.join("nested/deeper/b.txt"), "beta")?;
    let outside = tempfile::tempdir()?;
    std::fs::write(outside.path().join("big.bin"), vec![0_u8; 4096])?;
    std::os::unix::fs::symlink(outside.path().join("big.bin"), root.join("link"))?;
    std::os::unix::fs::symlink(outside.path(), root.join("dir-link"))?;
    // A socket: neither a regular file, a directory nor a link.
    let _socket = std::os::unix::net::UnixListener::bind(root.join("socket"))?;
    Ok((source, outside))
}

#[test]
fn sec_014_a_copy_keeps_files_and_links_and_never_follows_a_link() -> Result {
    let (source, outside) = tree()?;
    let target = tempfile::tempdir()?;
    let copied = copy_tree(source.path(), target.path(), WIDE)?;
    assert_eq!(copied, 9, "regular file bytes: alpha + beta");
    let root = target.path();
    assert_eq!(std::fs::read_to_string(root.join("a.txt"))?, "alpha");
    assert_eq!(
        std::fs::read_to_string(root.join("nested/deeper/b.txt"))?,
        "beta"
    );
    for link in ["link", "dir-link"] {
        assert!(
            std::fs::symlink_metadata(root.join(link))?
                .file_type()
                .is_symlink(),
            "{link}"
        );
    }
    assert_eq!(std::fs::read_link(root.join("dir-link"))?, outside.path());
    assert!(
        !root.join("socket").exists(),
        "other file types are skipped"
    );
    let names: Vec<_> = std::fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::result::Result<_, _>>()?;
    assert!(
        names
            .iter()
            .all(|name| !name.to_string_lossy().contains(".tqa-copy")),
        "{names:?}"
    );
    Ok(())
}

#[test]
fn gw_i_030_a_copy_stops_at_its_byte_and_entry_bounds() -> Result {
    let (source, _outside) = tree()?;
    for limits in [
        Limits {
            bytes: 8,
            entries: 100,
        },
        Limits {
            bytes: 1 << 20,
            entries: 3,
        },
    ] {
        let target = tempfile::tempdir()?;
        assert!(matches!(
            copy_tree(source.path(), target.path(), limits),
            Err(CopyError::Exceeded)
        ));
    }
    Ok(())
}

/// The charge of everything under `root`, computed independently with `std::fs` (never following a link).
fn expected(root: &Path) -> std::result::Result<u64, Box<dyn std::error::Error>> {
    use std::os::unix::fs::MetadataExt;
    let mut total = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let path = entry?.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            let blocks = metadata.blocks() * 512;
            total += if metadata.is_file() {
                blocks.max(metadata.len())
            } else {
                blocks
            };
            if metadata.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(total)
}

// TEST: GW-U-053
#[test]
fn gw_i_030_measuring_charges_allocated_blocks_without_following_links() -> Result {
    let (source, _outside) = tree()?;
    // The link to a 4 KiB file outside, and the link to a directory, are charged as links, never followed.
    let charged = measure(source.path(), WIDE)?;
    assert_eq!(charged, expected(source.path())?);
    assert!(charged >= 9, "at least the regular files' lengths");
    assert!(matches!(
        measure(
            source.path(),
            Limits {
                bytes: 1 << 20,
                entries: 2
            }
        ),
        Err(CopyError::Exceeded)
    ));
    // Measuring is not bounded by bytes: an over-quota workspace still reports its real size.
    assert_eq!(
        measure(
            source.path(),
            Limits {
                bytes: 1,
                entries: 100
            }
        )?,
        charged
    );
    Ok(())
}

// TEST: GW-U-053
#[test]
fn gw_u_053_small_and_sparse_files_are_charged_for_what_they_hold() -> Result {
    let small = tempfile::tempdir()?;
    for index in 0..200 {
        std::fs::write(small.path().join(format!("f{index}")), "x")?;
    }
    // Two hundred one-byte files take at least a block each wherever blocks are counted, not 200 bytes.
    let charged = measure(small.path(), MANY)?;
    assert_eq!(charged, expected(small.path())?);
    assert!(charged >= 200);
    let sparse = tempfile::tempdir()?;
    std::fs::File::create(sparse.path().join("hole"))?.set_len(1 << 30)?;
    // A sparse file counts its full length, though it allocates almost nothing.
    assert!(measure(sparse.path(), MANY)? >= 1 << 30);
    Ok(())
}

// TEST: GW-U-053
#[test]
fn gw_u_053_a_bounded_measure_stops_past_its_byte_limit() -> Result {
    let (source, _outside) = tree()?;
    let charged = measure(source.path(), WIDE)?;
    assert_eq!(
        measure_within(
            source.path(),
            Limits {
                bytes: charged,
                entries: 100
            }
        )?,
        charged
    );
    assert!(matches!(
        measure_within(
            source.path(),
            Limits {
                bytes: charged - 1,
                entries: 100
            }
        ),
        Err(CopyError::Exceeded)
    ));
    Ok(())
}

#[test]
fn gw_i_030_a_missing_source_is_an_error_not_an_empty_copy() -> Result {
    let target = tempfile::tempdir()?;
    let missing = target.path().join("absent");
    assert!(matches!(
        copy_tree(&missing, target.path(), WIDE),
        Err(CopyError::Io(_))
    ));
    Ok(())
}

const MANY: Limits = Limits {
    bytes: 1 << 20,
    entries: 100_000,
};

/// A chain of `levels` nested directories `d/d/…`, with a file at the bottom.
fn chain(root: &Path, levels: usize) -> Result {
    let mut path = root.to_path_buf();
    for _ in 0..levels {
        path.push("d");
    }
    std::fs::create_dir_all(&path)?;
    std::fs::write(path.join("end.txt"), "end")?;
    Ok(())
}

// TEST: GW-U-053
#[test]
fn gw_u_053_a_wide_tree_holds_one_descriptor_per_level_not_one_per_directory() -> Result {
    // Two thousand sibling directories each holding a file: a walk that opened every directory it discovered would
    // hold two thousand descriptors at once (the gateway's descriptors run out on a wider tree).
    let source = tempfile::tempdir()?;
    for index in 0..2000 {
        let directory = source.path().join(format!("s{index}"));
        std::fs::create_dir(&directory)?;
        std::fs::write(directory.join("f.txt"), "x")?;
    }
    let (bytes, peak) = measure_walk(source.path(), MANY, false)?;
    assert_eq!(bytes, expected(source.path())?);
    assert!(peak <= 2, "held {peak} directories open at once");
    let target = tempfile::tempdir()?;
    let (bytes, peak) = copy_walk(source.path(), target.path(), MANY)?;
    assert_eq!(bytes, 2000);
    assert!(peak <= 2, "held {peak} directories open at once");
    assert!(target.path().join("s1999/f.txt").exists());
    Ok(())
}

// TEST: GW-U-053
#[test]
fn gw_u_053_a_deep_chain_is_walked_to_the_depth_bound_and_refused_past_it() -> Result {
    let within = tempfile::tempdir()?;
    chain(within.path(), MAX_DEPTH)?;
    let (bytes, peak) = measure_walk(within.path(), MANY, false)?;
    assert_eq!(bytes, expected(within.path())?);
    assert!(peak <= 2, "held {peak} directories open at once");
    let target = tempfile::tempdir()?;
    let (bytes, _) = copy_walk(within.path(), target.path(), MANY)?;
    assert_eq!(bytes, 3);
    let beyond = tempfile::tempdir()?;
    chain(beyond.path(), MAX_DEPTH + 1)?;
    assert!(matches!(
        measure(beyond.path(), MANY),
        Err(CopyError::Exceeded)
    ));
    let target = tempfile::tempdir()?;
    assert!(matches!(
        copy_tree(beyond.path(), target.path(), MANY),
        Err(CopyError::Exceeded)
    ));
    Ok(())
}
