//! GW-I-045: upload names are sanitised to the attachment contract and never overwrite a stored file; `uploads/` is
//! never a link, and its usage counts regular files only.
use super::*;
use std::fs;
use std::os::fd::AsFd;
use std::os::unix::fs::symlink;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

// TEST: GW-I-013
#[test]
fn gw_i_045_names_are_sanitised_to_the_attachment_contract() {
    for (raw, expected) in [
        ("report.pdf", "report.pdf"),
        ("My Report (final).pdf", "My_Report__final_.pdf"),
        ("../../etc/passwd", "passwd"),
        ("C:\\Temp\\drafts\\notes.txt", "notes.txt"),
        (".bashrc", "bashrc"),
        ("...", "file"),
        ("", "file"),
        ("报告.pdf", "__.pdf"),
        ("a\0b.txt", "a_b.txt"),
    ] {
        assert_eq!(sanitise(raw), expected, "{raw:?}");
    }
    let long = format!("{}.tar.gz", "x".repeat(300));
    let name = sanitise(&long);
    assert_eq!(name.len(), MAX_NAME_BYTES);
    assert!(name.ends_with(".tar.gz"), "{name}");
    for raw in ["a b", ".hidden", "-dash", "_under", "ünï.txt"] {
        let name = sanitise(raw);
        let first = name.as_bytes()[0];
        assert!(
            first.is_ascii_alphanumeric() || matches!(first, b'_' | b'-'),
            "{name}"
        );
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        );
    }
}

fn workspace() -> std::result::Result<(tempfile::TempDir, std::path::PathBuf), std::io::Error> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("workspace");
    fs::create_dir(&root)?;
    Ok((temporary, root))
}

/// A path to a new regular file inside the opened `uploads/`.
fn temporary_dir_file(uploads: &std::os::fd::OwnedFd) -> String {
    format!(
        "/proc/self/fd/{}/plain-file",
        std::os::fd::AsRawFd::as_raw_fd(uploads)
    )
}

// TEST: GW-I-013
#[test]
fn gw_i_045_finished_files_take_the_first_free_name() -> Result {
    let (_temporary, root) = workspace()?;
    let root = path::open_root(&root)?;
    let uploads = directory(root.as_fd())?;
    let mut stored = Vec::new();
    for round in 0..3 {
        let temporary = format!("{TEMPORARY_PREFIX}{round}");
        fs::write(
            format!(
                "/proc/self/fd/{}/{temporary}",
                std::os::fd::AsRawFd::as_raw_fd(&uploads)
            ),
            "abc",
        )?;
        stored.push(link(uploads.as_fd(), &temporary, "notes.tar.gz")?.ok_or("no free name")?);
    }
    assert_eq!(stored, ["notes.tar.gz", "notes-2.tar.gz", "notes-3.tar.gz"]);
    // Only regular files count: three copies of three bytes each, plus the temporaries still there.
    fs::create_dir(format!(
        "/proc/self/fd/{}/nested",
        std::os::fd::AsRawFd::as_raw_fd(&uploads)
    ))?;
    assert_eq!(used_in(uploads.as_fd())?, 18);
    // A long name keeps its extension within the limit when numbered.
    let long = format!("{}.txt", "y".repeat(MAX_NAME_BYTES - 4));
    let temporary = format!("{TEMPORARY_PREFIX}long");
    fs::write(
        format!(
            "/proc/self/fd/{}/{temporary}",
            std::os::fd::AsRawFd::as_raw_fd(&uploads)
        ),
        "x",
    )?;
    assert_eq!(
        link(uploads.as_fd(), &temporary, &long)?,
        Some(long.clone())
    );
    let again = link(uploads.as_fd(), &temporary, &long)?.ok_or("no free name")?;
    assert!(
        again.len() <= MAX_NAME_BYTES && again.ends_with("-2.txt"),
        "{again}"
    );
    // Every numbered name taken: none is free.
    for attempt in 3..=1000 {
        fs::write(
            format!(
                "/proc/self/fd/{}/taken-{attempt}",
                std::os::fd::AsRawFd::as_raw_fd(&uploads)
            ),
            "",
        )?;
    }
    fs::write(
        format!(
            "/proc/self/fd/{}/taken",
            std::os::fd::AsRawFd::as_raw_fd(&uploads)
        ),
        "",
    )?;
    fs::write(
        format!(
            "/proc/self/fd/{}/taken-2",
            std::os::fd::AsRawFd::as_raw_fd(&uploads)
        ),
        "",
    )?;
    assert_eq!(link(uploads.as_fd(), &temporary, "taken")?, None);
    // Usage of something that is not a directory is host I/O.
    let file = fs::File::create(temporary_dir_file(&uploads))?;
    assert_eq!(used_in(file.as_fd()), Err(TreeError::Io));
    // A temporary that is gone is host I/O, not a taken name.
    assert_eq!(
        link(uploads.as_fd(), ".upload-missing", "fresh"),
        Err(TreeError::Io)
    );
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_an_uploads_link_is_refused_and_a_missing_one_is_created() -> Result {
    let (temporary, root) = workspace()?;
    let opened = path::open_root(&root)?;
    directory(opened.as_fd())?;
    assert!(root.join(DIRECTORY).is_dir());
    fs::remove_dir(root.join(DIRECTORY))?;
    symlink(temporary.path(), root.join(DIRECTORY))?;
    assert_eq!(
        directory(opened.as_fd()).err(),
        Some(TreeError::PathInvalid)
    );
    // A root that is gone is host I/O.
    let gone = path::open_root(&root)?;
    drop(temporary);
    assert_eq!(directory(gone.as_fd()).err(), Some(TreeError::Io));
    Ok(())
}

// TEST: RT-I-026
#[test]
fn rt_i_026_part_files_are_created_once_reopened_as_regular_files_and_swept_when_stale() -> Result {
    use std::io::Write;
    use std::time::{Duration, SystemTime};
    let (temporary, root) = workspace()?;
    let opened = path::open_root(&root)?;
    let uploads = directory(opened.as_fd())?;
    let mut part = create_part(uploads.as_fd(), ".upload-7")?;
    part.write_all(b"abc")?;
    // A part is created once: the same id again is refused.
    assert_eq!(
        create_part(uploads.as_fd(), ".upload-7").err(),
        Some(TreeError::PathInvalid)
    );
    let (_, size) = open_part(uploads.as_fd(), ".upload-7")?;
    assert_eq!(size, 3);
    assert_eq!(
        open_part(uploads.as_fd(), ".upload-8").err(),
        Some(TreeError::NotFound)
    );
    // A link or a directory in a part's place is never opened.
    symlink(temporary.path(), root.join("uploads/.upload-9"))?;
    fs::create_dir(root.join("uploads/.upload-10"))?;
    for planted in [".upload-9", ".upload-10"] {
        assert_eq!(
            open_part(uploads.as_fd(), planted).err(),
            Some(TreeError::PathInvalid),
            "{planted}"
        );
        assert_eq!(
            create_part(uploads.as_fd(), planted).err(),
            Some(TreeError::PathInvalid),
            "{planted}"
        );
    }
    fs::write(root.join("uploads/kept.txt"), "kept")?;
    // A part hard-linked to another file would redirect the upload's bytes into it: never opened.
    fs::hard_link(
        root.join("uploads/kept.txt"),
        root.join("uploads/.upload-12"),
    )?;
    assert_eq!(
        open_part(uploads.as_fd(), ".upload-12").err(),
        Some(TreeError::PathInvalid)
    );
    fs::remove_file(root.join("uploads/.upload-12"))?;
    // Nothing is stale yet; an hour later every regular part is, and only those go.
    assert_eq!(
        sweep_parts(uploads.as_fd(), SystemTime::now(), Duration::from_mins(10))?,
        0
    );
    let later = SystemTime::now() + Duration::from_hours(1);
    assert_eq!(
        sweep_parts(uploads.as_fd(), later, Duration::from_mins(10))?,
        1
    );
    assert!(!root.join("uploads/.upload-7").exists());
    for kept in [
        "uploads/kept.txt",
        "uploads/.upload-9",
        "uploads/.upload-10",
    ] {
        assert!(root.join(kept).symlink_metadata().is_ok(), "{kept}");
    }
    // A part is removed once; a directory in its place is not a part.
    fs::write(root.join("uploads/.upload-11"), "")?;
    remove_part(uploads.as_fd(), ".upload-11")?;
    assert_eq!(
        remove_part(uploads.as_fd(), ".upload-11"),
        Err(TreeError::NotFound)
    );
    assert_eq!(
        remove_part(uploads.as_fd(), ".upload-10"),
        Err(TreeError::Io)
    );
    // Not a directory: host I/O.
    let file = fs::File::open(root.join("uploads/kept.txt"))?;
    assert_eq!(
        sweep_parts(file.as_fd(), later, Duration::from_mins(10)),
        Err(TreeError::Io)
    );
    Ok(())
}
