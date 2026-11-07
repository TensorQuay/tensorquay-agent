//! GW-I-045 / GW-I-047 path resolution: nothing outside the workspace is ever opened, whatever the agent plants.
use super::*;
use std::fs;
use std::os::unix::fs::symlink;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

fn workspace()
-> std::result::Result<(tempfile::TempDir, std::path::PathBuf, std::path::PathBuf), std::io::Error>
{
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("workspace");
    let outside = temporary.path().join("outside");
    for base in [&root, &outside] {
        fs::create_dir_all(base.join("d1/d2/d3"))?;
    }
    fs::write(root.join("d1/d2/d3/f.txt"), "inside")?;
    fs::write(outside.join("d1/d2/d3/f.txt"), "outside secret")?;
    Ok((temporary, root, outside))
}

fn read(root: &std::path::Path, path: &str) -> std::result::Result<String, TreeError> {
    let root = open_root(root)?;
    let (file, _) = open_file(root.as_fd(), &WorkspacePath::parse(path)?)?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut fs::File::from(file), &mut text)
        .map_err(|_| TreeError::Io)?;
    Ok(text)
}

// TEST: GW-I-013
#[test]
fn gw_i_045_paths_are_relative_bounded_and_never_climb() -> Result {
    for accepted in [
        "",
        ".",
        "a",
        "a/b.txt",
        "a/",
        "space name.md",
        "ünïcode/файл",
    ] {
        assert!(WorkspacePath::parse(accepted).is_ok(), "{accepted}");
    }
    for rejected in [
        "/etc/passwd",
        "..",
        "a/../b",
        "a//b",
        "a/./b",
        "./a",
        "a\0b",
        &"x".repeat(MAX_PATH_BYTES + 1),
    ] {
        assert_eq!(
            WorkspacePath::parse(rejected),
            Err(TreeError::PathInvalid),
            "{rejected:?}"
        );
    }
    // Other tasks' spill files and logs live under .tqa/; they are not part of the file view.
    for hidden in [".tqa", ".tqa/other/log.txt"] {
        assert_eq!(WorkspacePath::parse(hidden), Err(TreeError::NotFound));
    }
    assert_eq!(WorkspacePath::parse("a/b.txt")?.to_string(), "a/b.txt");
    assert_eq!(WorkspacePath::parse("a/")?.to_string(), "a");
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_047_reads_a_nested_file_and_reports_missing_ones() -> Result {
    let (_temporary, root, _outside) = workspace()?;
    assert_eq!(read(&root, "d1/d2/d3/f.txt")?, "inside");
    assert_eq!(read(&root, "d1/nope.txt"), Err(TreeError::NotFound));
    assert_eq!(read(&root, "nope/f.txt"), Err(TreeError::NotFound));
    // A directory is not a file, and a file is not a directory.
    assert_eq!(read(&root, "d1/d2"), Err(TreeError::PathInvalid));
    assert_eq!(read(&root, "d1/d2/d3/f.txt/more"), Err(TreeError::NotFound));
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_a_symlink_at_every_depth_is_refused() -> Result {
    let components = ["d1", "d2", "d3", "f.txt"];
    for depth in 0..components.len() {
        for absolute in [true, false] {
            let (_temporary, root, outside) = workspace()?;
            let prefix = components[..depth].join("/");
            let planted = root.join(&prefix).join(components[depth]);
            let target = outside.join(&prefix).join(components[depth]);
            if planted.is_dir() {
                fs::remove_dir_all(&planted)?;
            } else {
                fs::remove_file(&planted)?;
            }
            if absolute {
                symlink(&target, &planted)?;
            } else {
                // A relative link that climbs out through the workspace root.
                let climb = "../".repeat(depth + 1);
                symlink(
                    format!("{climb}outside/{}", components[..=depth].join("/")),
                    &planted,
                )?;
            }
            assert_eq!(
                read(&root, "d1/d2/d3/f.txt"),
                Err(TreeError::PathInvalid),
                "depth {depth}, absolute {absolute}"
            );
        }
    }
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_a_directory_swapped_for_a_symlink_during_the_walk_is_refused() -> Result {
    for swap_at in 0..3 {
        let (_temporary, root, outside) = workspace()?;
        let names = ["d1", "d2", "d3"];
        let swapped = root.join(names[..=swap_at].join("/"));
        let target = outside.join(names[..=swap_at].join("/"));
        let root_fd = open_root(&root)?;
        let path = WorkspacePath::parse("d1/d2/d3/f.txt")?;
        let result = open_file_with(root_fd.as_fd(), &path, |depth| {
            // Just before the walk opens `names[swap_at]`, the agent replaces it with a link out of the workspace.
            if depth == swap_at {
                let _ = fs::rename(&swapped, swapped.with_extension("old"));
                let _ = symlink(&target, &swapped);
            }
        });
        assert_eq!(
            result.err(),
            Some(TreeError::PathInvalid),
            "swap at {swap_at}"
        );
    }
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_a_swap_after_the_walk_still_reads_from_the_walked_directory() -> Result {
    let (_temporary, root, outside) = workspace()?;
    let root_fd = open_root(&root)?;
    let path = WorkspacePath::parse("d1/d2/d3/f.txt")?;
    let (file, _) = open_file_with(root_fd.as_fd(), &path, |depth| {
        // After the last directory is open, swap it: the final open uses the walked fd, never the path again.
        if depth == 3 {
            let d3 = root.join("d1/d2/d3");
            let _ = fs::rename(&d3, root.join("d1/d2/d3-old"));
            let _ = symlink(outside.join("d1/d2/d3"), &d3);
        }
    })?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut fs::File::from(file), &mut text)?;
    assert_eq!(text, "inside");
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_047_lists_one_level_without_following_links() -> Result {
    let (_temporary, root, outside) = workspace()?;
    fs::write(root.join("d1/a.txt"), "12345")?;
    symlink(outside.join("d1"), root.join("d1/link"))?;
    let root_fd = open_root(&root)?;
    let dir = open_dir(root_fd.as_fd(), &WorkspacePath::parse("d1")?)?;
    let mut entries = list(dir.as_fd())?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let summary: Vec<(String, Kind, u64)> = entries
        .into_iter()
        .map(|entry| {
            (
                entry.name,
                entry.kind,
                if entry.kind == Kind::File {
                    entry.size
                } else {
                    0
                },
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("a.txt".to_owned(), Kind::File, 5),
            ("d2".to_owned(), Kind::Dir, 0),
            ("link".to_owned(), Kind::Link, 0),
        ]
    );
    // The link is listed, but opening through it is refused.
    assert_eq!(
        read(&root, "d1/link/d2/d3/f.txt"),
        Err(TreeError::PathInvalid)
    );
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_047_the_root_listing_leaves_out_other_tasks_notes() -> Result {
    let (_temporary, root, _outside) = workspace()?;
    fs::create_dir_all(root.join(".tqa/other"))?;
    fs::create_dir_all(root.join("d1/.tqa"))?;
    let root_fd = open_root(&root)?;
    let mut top = list(root_fd.as_fd())?;
    hide_private(&mut top, true);
    assert_eq!(
        top.iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["d1"]
    );
    // Only the workspace root's .tqa is private; a nested directory of that name is ordinary content.
    let nested = open_dir(root_fd.as_fd(), &WorkspacePath::parse("d1")?)?;
    let mut inner = list(nested.as_fd())?;
    hide_private(&mut inner, false);
    assert!(inner.iter().any(|entry| entry.name == ".tqa"));
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_047_the_root_and_the_entry_kinds_are_named() -> Result {
    assert!(WorkspacePath::parse("")?.is_root());
    assert!(!WorkspacePath::parse("a/b")?.is_root());
    let names = [Kind::File, Kind::Dir, Kind::Link, Kind::Other].map(Kind::as_str);
    assert_eq!(names, ["file", "dir", "link", "other"]);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_047_special_and_unnamed_entries_are_listed_or_left_out_safely() -> Result {
    use std::os::unix::ffi::OsStrExt;
    let (_temporary, root, _) = workspace()?;
    nix::unistd::mkfifo(
        &root.join("pipe"),
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )?;
    fs::write(
        root.join(std::ffi::OsStr::from_bytes(b"bad-\xff")),
        "not UTF-8",
    )?;
    let directory = open_root(&root)?;
    let entries = list(directory.as_fd())?;
    let pipe = entries
        .iter()
        .find(|entry| entry.name == "pipe")
        .ok_or("pipe")?;
    assert_eq!(pipe.kind, Kind::Other);
    assert!(
        entries.iter().all(|entry| !entry.name.starts_with("bad-")),
        "a name that cannot round-trip is left out"
    );
    // A regular-file open refuses anything else, even when the name resolves.
    assert_eq!(
        child_file(directory.as_fd(), "d1").err(),
        Some(TreeError::PathInvalid)
    );
    assert_eq!(
        child_file(directory.as_fd(), "pipe").err(),
        Some(TreeError::PathInvalid)
    );
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_047_directories_without_access_fail_as_io() -> Result {
    use std::os::unix::fs::PermissionsExt;
    let (_temporary, root, _) = workspace()?;
    fs::create_dir(root.join("closed"))?;
    fs::create_dir(root.join("listed"))?;
    fs::write(root.join("listed/f.txt"), "x")?;
    fs::set_permissions(root.join("closed"), fs::Permissions::from_mode(0o000))?;
    // Readable but not searchable: the directory opens, but reading it reopens "." inside it, which fails.
    fs::set_permissions(root.join("listed"), fs::Permissions::from_mode(0o400))?;
    let directory = open_root(&root)?;
    let closed = child_dir(directory.as_fd(), "closed").err();
    let listed = walk(directory.as_fd(), &["listed".to_owned()], |_| ()).map(|fd| list(fd.as_fd()));
    for name in ["closed", "listed"] {
        fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700))?;
    }
    assert_eq!(
        closed,
        Some(TreeError::Io),
        "run as a non-root user: root ignores directory permissions"
    );
    assert_eq!(listed?.err(), Some(TreeError::Io));
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_the_root_is_not_a_file_and_single_opens_classify_their_failure() -> Result {
    let (_temporary, root, _) = workspace()?;
    symlink("/etc/passwd", root.join("linked"))?;
    let directory = open_root(&root)?;
    assert_eq!(
        open_file(directory.as_fd(), &WorkspacePath::parse("")?).err(),
        Some(TreeError::PathInvalid)
    );
    assert_eq!(
        child_file(directory.as_fd(), "absent").err(),
        Some(TreeError::NotFound)
    );
    assert_eq!(
        child_file(directory.as_fd(), "linked").err(),
        Some(TreeError::PathInvalid)
    );
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_a_root_that_lost_its_search_permission_fails_as_io() -> Result {
    use std::os::unix::fs::PermissionsExt;
    let (_temporary, root, _) = workspace()?;
    let directory = open_root(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000))?;
    let walked = walk(directory.as_fd(), &[], |_| ()).err();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    assert_eq!(
        walked,
        Some(TreeError::Io),
        "run as a non-root user: root ignores directory permissions"
    );
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_a_path_of_exactly_the_bound_is_accepted_and_one_byte_more_is_not() -> Result {
    let exact = "a".repeat(MAX_PATH_BYTES);
    assert_eq!(WorkspacePath::parse(&exact)?.to_string(), exact);
    assert_eq!(
        WorkspacePath::parse(&"a".repeat(MAX_PATH_BYTES + 1)),
        Err(TreeError::PathInvalid)
    );
    Ok(())
}

#[test]
fn rt_i_025_one_entry_is_described_without_following_it() -> Result {
    let (_temporary, root, outside) = workspace()?;
    symlink(outside.join("d1/d2/d3/f.txt"), root.join("d1/link"))?;
    let root = open_root(&root)?;
    let d1 = walk(root.as_fd(), &["d1".to_owned()], |_| ())?;
    let described = |name: &str| entry(d1.as_fd(), name);
    let directory = described("d2")?;
    assert_eq!((directory.name.as_str(), directory.kind), ("d2", Kind::Dir));
    let link = described("link")?;
    assert_eq!(link.kind, Kind::Link, "the link itself, never its target");
    let file = entry(
        walk(
            root.as_fd(),
            &["d1".into(), "d2".into(), "d3".into()],
            |_| (),
        )?
        .as_fd(),
        "f.txt",
    )?;
    assert_eq!((file.kind, file.size), (Kind::File, 6));
    assert_eq!(
        described("missing").map(|entry| entry.kind),
        Err(TreeError::NotFound)
    );
    Ok(())
}
