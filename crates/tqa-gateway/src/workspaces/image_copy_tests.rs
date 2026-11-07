//! GW-I-064 (unit): copying a microVM workspace image for a fork, without mounting or reading it as a filesystem.
use super::{CopyError, copy_image, copy_image_with, falls_back};
use rustix::io::Errno;
use std::{
    cell::Cell,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SIZE: u64 = 64 << 20;
/// Data at the start, in the middle and at an unaligned offset near the end; everything else is a hole.
const EXTENTS: [(u64, usize, u8); 3] = [
    (0, 4096, b'a'),
    (16 << 20, 1 << 20, b'b'),
    ((40 << 20) + 100, 5000, b'c'),
];

fn image(directory: &Path) -> Result<PathBuf, std::io::Error> {
    let path = directory.join("source.ext4");
    let mut file = fs::File::create_new(&path)?;
    file.set_len(SIZE)?;
    for (offset, length, byte) in EXTENTS {
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&vec![byte; length])?;
    }
    file.sync_all()?;
    Ok(path)
}

fn allocation(path: &Path) -> Result<u64, std::io::Error> {
    Ok(fs::metadata(path)?.blocks() * 512)
}

fn same_bytes(left: &Path, right: &Path) -> Result<bool, std::io::Error> {
    let (mut left, mut right) = (fs::File::open(left)?, fs::File::open(right)?);
    let (mut a, mut b) = (vec![0; 1 << 20], vec![0; 1 << 20]);
    loop {
        let (read_a, read_b) = (left.read(&mut a)?, right.read(&mut b)?);
        if read_a != read_b || a[..read_a] != b[..read_b] {
            return Ok(false);
        }
        if read_a == 0 {
            return Ok(true);
        }
    }
}

/// Only the source and, when present, the target: no temporary file is ever left behind.
fn names(directory: &Path) -> Result<Vec<String>, std::io::Error> {
    let mut names: Vec<String> = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    names.sort();
    Ok(names)
}

/// A clone that copies every byte, standing in for a filesystem's reflink.
fn dense_clone(from: &std::os::fd::OwnedFd, to: &std::os::fd::OwnedFd) -> rustix::io::Result<()> {
    let mut input = fs::File::from(from.try_clone().map_err(|_| Errno::IO)?);
    let mut output = fs::File::from(to.try_clone().map_err(|_| Errno::IO)?);
    std::io::copy(&mut input, &mut output).map_err(|_| Errno::IO)?;
    Ok(())
}

#[test]
fn gw_i_064_a_sparse_image_is_copied_byte_for_byte_and_stays_sparse() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = image(directory.path())?;
    let target = directory.path().join("fork.ext4");
    let allocated = copy_image(&source, &target, 1 << 30)?.ok_or("the source exists")?;
    assert!(same_bytes(&source, &target)?);
    assert_eq!(fs::metadata(&target)?.len(), SIZE);
    assert_eq!(
        allocated,
        allocation(&target)?,
        "the reported size is the copy's allocation"
    );
    assert!(
        allocated <= allocation(&source)? + (64 << 10),
        "holes stay holes: {allocated}"
    );
    assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(names(directory.path())?, ["fork.ext4", "source.ext4"]);
    Ok(())
}

#[test]
fn gw_i_064_a_workspace_never_started_has_no_image_and_forks_empty() -> TestResult {
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("fork.ext4");
    assert!(copy_image(&directory.path().join("absent.ext4"), &target, 1 << 30)?.is_none());
    assert!(names(directory.path())?.is_empty());
    Ok(())
}

#[test]
fn gw_i_064_an_image_over_the_quota_is_refused_and_leaves_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = image(directory.path())?;
    let target = directory.path().join("fork.ext4");
    // Before copying: the source's allocation alone exceeds the quota.
    let small = allocation(&source)? - 1;
    assert!(matches!(
        copy_image(&source, &target, small),
        Err(CopyError::Exceeded)
    ));
    // After copying: a clone that allocates every block exceeds a quota the source fits in.
    let fits = allocation(&source)? + (1 << 20);
    let dense = copy_image_with(&source, &target, fits, dense_clone);
    assert!(matches!(dense, Err(CopyError::Exceeded)), "{dense:?}");
    assert_eq!(names(directory.path())?, ["source.ext4"]);
    Ok(())
}

#[test]
fn gw_i_064_links_directories_and_existing_targets_are_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = image(directory.path())?;
    let link = directory.path().join("link.ext4");
    std::os::unix::fs::symlink(&source, &link)?;
    let target = directory.path().join("fork.ext4");
    assert!(
        copy_image(&link, &target, 1 << 30).is_err(),
        "a link is never followed"
    );
    assert!(
        copy_image(directory.path(), &target, 1 << 30).is_err(),
        "not a regular file"
    );
    fs::write(&target, b"existing")?;
    assert!(
        copy_image(&source, &target, 1 << 30).is_err(),
        "an existing target is never replaced"
    );
    assert_eq!(fs::read(&target)?, b"existing");
    assert_eq!(
        names(directory.path())?,
        ["fork.ext4", "link.ext4", "source.ext4"]
    );
    Ok(())
}

#[test]
fn gw_i_064_a_reflink_is_used_when_offered_and_a_sparse_copy_otherwise() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = image(directory.path())?;
    let cloned = Cell::new(false);
    let target = directory.path().join("cloned.ext4");
    copy_image_with(&source, &target, 1 << 30, |from, to| {
        cloned.set(true);
        dense_clone(from, to)
    })?;
    assert!(cloned.get() && same_bytes(&source, &target)?);
    let target = directory.path().join("fallback.ext4");
    copy_image_with(&source, &target, 1 << 30, |_, _| Err(Errno::OPNOTSUPP))?;
    assert!(same_bytes(&source, &target)?);
    assert!(allocation(&target)? < 8 << 20, "the fallback keeps holes");
    let target = directory.path().join("failed.ext4");
    let failed = copy_image_with(&source, &target, 1 << 30, |_, _| Err(Errno::IO));
    assert!(matches!(failed, Err(CopyError::Io(_))), "{failed:?}");
    assert!(!target.exists());
    assert_eq!(
        names(directory.path())?,
        ["cloned.ext4", "fallback.ext4", "source.ext4"]
    );
    Ok(())
}

#[test]
fn gw_i_064_only_an_unsupported_reflink_falls_back_to_copying() {
    for errno in [
        Errno::OPNOTSUPP,
        Errno::NOTTY,
        Errno::XDEV,
        Errno::INVAL,
        Errno::NOSYS,
    ] {
        assert!(falls_back(errno), "{errno:?}");
    }
    for errno in [Errno::IO, Errno::ACCESS, Errno::NOSPC, Errno::PERM] {
        assert!(!falls_back(errno), "{errno:?}");
    }
}

#[test]
fn gw_i_064_a_dense_image_is_copied_to_its_end_and_the_copy_is_bounded() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("dense.ext4");
    fs::write(&source, vec![9; 3 << 20])?;
    let target = directory.path().join("fork.ext4");
    copy_image_with(&source, &target, 1 << 30, |_, _| Err(Errno::OPNOTSUPP))?;
    assert!(
        same_bytes(&source, &target)?,
        "data up to the last byte, no trailing hole"
    );
    // The bound on copied data holds within the copy too, not only in the allocation check before it.
    let (from, to) = (
        fs::File::open(&source)?,
        tempfile::tempfile_in(directory.path())?,
    );
    let bounded = super::sparse_copy(&from.into(), &to.into(), 3 << 20, 1 << 20);
    assert!(matches!(bounded, Err(CopyError::Exceeded)), "{bounded:?}");
    // Copying exactly the bound is allowed.
    let (from, to) = (
        fs::File::open(&source)?,
        tempfile::tempfile_in(directory.path())?,
    );
    super::sparse_copy(&from.into(), &to.into(), 3 << 20, 3 << 20)?;
    Ok(())
}

#[test]
fn gw_i_064_a_target_without_a_file_name_is_refused() {
    assert!(matches!(
        super::temporary_path(Path::new("/")),
        Err(CopyError::Io(_))
    ));
    assert_eq!(
        super::temporary_path(Path::new("/storage/workspaces/w.ext4")).ok(),
        Some(PathBuf::from("/storage/workspaces/.w.ext4.tqa-copy"))
    );
}

#[test]
fn gw_i_064_the_quota_is_checked_before_any_copy_and_an_exact_fit_is_allowed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("dense.ext4");
    fs::write(&source, vec![5; 1 << 20])?;
    let exact = allocation(&source)?;
    let target = directory.path().join("fork.ext4");
    let copied = Cell::new(false);
    let refused = copy_image_with(&source, &target, exact - 1, |from, to| {
        copied.set(true);
        dense_clone(from, to)
    });
    assert!(matches!(refused, Err(CopyError::Exceeded)), "{refused:?}");
    assert!(
        !copied.get(),
        "an image over the quota is refused before anything is copied"
    );
    // A dense copy allocates what its source does, so a quota equal to that allocation is enough.
    assert_eq!(
        copy_image_with(&source, &target, exact, dense_clone)?,
        Some(allocation(&target)?)
    );
    Ok(())
}
