//! TOOL-U-152: a person's changes to memory notes: write, remove and one Undo, by directory handle. Names are one
//! plain `*.md` component; nothing is written or read through a link; each change keeps one previous version.
use super::*;
use crate::workspace_tree::path::open_root;
use std::{error::Error, fs};

type TestResult = Result<(), Box<dyn Error>>;

fn workspace() -> Result<(tempfile::TempDir, OwnedFd), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let root = open_root(directory.path())?;
    Ok((directory, root))
}

// TEST: TOOL-U-152
#[test]
fn tool_u_152_a_note_name_is_one_plain_markdown_component() {
    for name in ["pnpm.md", "Build-notes_2.MD", "a.md"] {
        assert!(valid_name(name), "{name}");
    }
    let long = format!("{}.md", "x".repeat(MAX_NAME_BYTES));
    for name in [
        "",
        ".md",
        ".hidden.md",
        "notes.txt",
        "a/b.md",
        "../x.md",
        "a b.md",
        "tasks",
        "é.md",
        &long,
    ] {
        assert!(!valid_name(name), "{name}");
    }
}

// TEST: TOOL-U-152
#[test]
fn tool_u_152_write_then_undo_removes_a_new_note_and_restores_an_edited_one() -> TestResult {
    let (directory, root) = workspace()?;
    let memory = super::directory(root.as_fd())?;
    let at = directory.path().join(DIRECTORY);
    // A new note: written, read back; its Undo removes it.
    write(memory.as_fd(), "build.md", "about: make all\n")?;
    assert_eq!(
        read(memory.as_fd(), "build.md")?.as_deref(),
        Some("about: make all\n")
    );
    assert!(!at.join(UNDO).join("build.md").exists());
    assert_eq!(undo(memory.as_fd(), "build.md")?, Undone::Removed);
    assert_eq!(read(memory.as_fd(), "build.md")?, None);
    assert!(matches!(
        undo(memory.as_fd(), "build.md"),
        Err(TreeError::NotFound)
    ));
    // An edit keeps the version before it; Undo puts it back, once.
    write(memory.as_fd(), "build.md", "about: make all\n")?;
    write(memory.as_fd(), "build.md", "about: make release\n")?;
    assert_eq!(
        fs::read_to_string(at.join(UNDO).join("build.md"))?,
        "about: make all\n"
    );
    assert_eq!(undo(memory.as_fd(), "build.md")?, Undone::Restored);
    assert_eq!(
        read(memory.as_fd(), "build.md")?.as_deref(),
        Some("about: make all\n")
    );
    // A second Undo (a retried request) has nothing kept: it changes nothing, and the restored note stays.
    assert!(matches!(
        undo(memory.as_fd(), "build.md"),
        Err(TreeError::NotFound)
    ));
    assert_eq!(
        read(memory.as_fd(), "build.md")?.as_deref(),
        Some("about: make all\n")
    );
    // Removing keeps the note for an Undo.
    remove(memory.as_fd(), "build.md")?;
    assert_eq!(read(memory.as_fd(), "build.md")?, None);
    assert_eq!(undo(memory.as_fd(), "build.md")?, Undone::Restored);
    assert_eq!(
        read(memory.as_fd(), "build.md")?.as_deref(),
        Some("about: make all\n")
    );
    // A second Undo (a retried request) has nothing kept: it changes nothing, and the restored note stays.
    assert!(matches!(
        undo(memory.as_fd(), "build.md"),
        Err(TreeError::NotFound)
    ));
    assert_eq!(
        read(memory.as_fd(), "build.md")?.as_deref(),
        Some("about: make all\n")
    );
    // A note that was there before any change was kept (written some other way) has nothing to undo.
    fs::write(at.join("old.md"), "about: old\n")?;
    assert!(matches!(
        undo(memory.as_fd(), "old.md"),
        Err(TreeError::NotFound)
    ));
    assert_eq!(
        read(memory.as_fd(), "old.md")?.as_deref(),
        Some("about: old\n")
    );
    assert!(matches!(
        remove(memory.as_fd(), "none.md"),
        Err(TreeError::NotFound)
    ));
    // No temporary file is left behind.
    let left: Vec<_> = fs::read_dir(&at)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".note"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    Ok(())
}

// TEST: TOOL-U-152
#[test]
fn tool_u_152_bad_names_and_big_notes_are_refused_before_anything_is_written() -> TestResult {
    let (directory, root) = workspace()?;
    let memory = super::directory(root.as_fd())?;
    for name in ["../escape.md", ".hidden.md", "notes.txt", "a/b.md"] {
        assert!(
            matches!(
                write(memory.as_fd(), name, "x"),
                Err(TreeError::PathInvalid)
            ),
            "{name}"
        );
        assert!(
            matches!(read(memory.as_fd(), name), Err(TreeError::PathInvalid)),
            "{name}"
        );
        assert!(
            matches!(remove(memory.as_fd(), name), Err(TreeError::PathInvalid)),
            "{name}"
        );
        assert!(
            matches!(undo(memory.as_fd(), name), Err(TreeError::PathInvalid)),
            "{name}"
        );
    }
    let big = "x".repeat(MAX_NOTE_BYTES + 1);
    assert!(matches!(
        write(memory.as_fd(), "big.md", &big),
        Err(TreeError::PathInvalid)
    ));
    let names: Vec<_> = fs::read_dir(directory.path().join(DIRECTORY))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert!(names.is_empty(), "{names:?}");
    assert!(!directory.path().join("escape.md").exists());
    Ok(())
}

// TEST: TOOL-U-152
#[cfg(unix)]
#[test]
fn tool_u_152_nothing_goes_through_a_link() -> TestResult {
    use std::os::unix::fs::symlink;
    // The memory folder itself a link to somewhere else: refused, nothing written there.
    let (directory, root) = workspace()?;
    let outside = tempfile::tempdir()?;
    symlink(outside.path(), directory.path().join(DIRECTORY))?;
    assert!(super::directory(root.as_fd()).is_err());
    assert_eq!(fs::read_dir(outside.path())?.count(), 0);
    // A note that is a link to a file outside: never read, never written through, never moved.
    let (directory, root) = workspace()?;
    let memory = super::directory(root.as_fd())?;
    let target = outside.path().join("secret.txt");
    fs::write(&target, "outside")?;
    symlink(&target, directory.path().join(DIRECTORY).join("link.md"))?;
    assert!(read(memory.as_fd(), "link.md").is_err());
    assert!(write(memory.as_fd(), "link.md", "about: mine\n").is_err());
    assert!(remove(memory.as_fd(), "link.md").is_err());
    assert_eq!(fs::read_to_string(&target)?, "outside");
    // The undo folder a link: refused too.
    let (directory, root) = workspace()?;
    let memory = super::directory(root.as_fd())?;
    write(memory.as_fd(), "a.md", "one")?;
    fs::remove_dir_all(directory.path().join(DIRECTORY).join(UNDO))?;
    symlink(outside.path(), directory.path().join(DIRECTORY).join(UNDO))?;
    assert!(write(memory.as_fd(), "a.md", "two").is_err());
    assert_eq!(
        fs::read_to_string(directory.path().join(DIRECTORY).join("a.md"))?,
        "one"
    );
    Ok(())
}
