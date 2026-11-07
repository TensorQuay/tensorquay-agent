//! Forking a microVM workspace: a copy of its block image (ARCHITECTURE §4.6), never a mount or a read of the
//! filesystem inside it (§4.5). A reflink where the filesystem offers one, else a sparse copy that keeps holes. Written
//! under a temporary name and renamed without replacing anything. Blocking; callers run it on the blocking pool. An
//! image copied while its VM runs is crash-consistent (ext4 replays its journal at first mount); the fork records that.
use super::copy::CopyError;
use rustix::{
    fs::{self as sys, FileType, Mode, OFlags, RenameFlags, SeekFrom},
    io::Errno,
};
use std::{
    ffi::OsString,
    os::fd::OwnedFd,
    path::{Path, PathBuf},
};

#[cfg(test)]
#[path = "image_copy_tests.rs"]
mod tests;

/// Bytes one `copy_file_range` call may move, so a large extent is copied in bounded steps.
const COPY_CHUNK_BYTES: usize = 8 << 20;
/// Bytes per allocated block as `st_blocks` counts them (POSIX).
const BLOCK_BYTES: u64 = 512;

const SOURCE: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC)
    .union(OFlags::NONBLOCK);
const TARGET: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// Copies the image at `source` to the new file `target`, private to its owner (the sandbox backend adds the VMM's
/// access when it first starts the fork). Returns the copy's allocated bytes, or `None` when the source has no image
/// yet (a workspace never started forks empty).
///
/// # Errors
/// `Exceeded` when the source's or the copy's allocation is over `max_bytes`; `Io` for a source that is a link or not
/// a regular file, an existing target, or any I/O failure. Nothing is left behind on error.
pub(crate) fn copy_image(
    source: &Path,
    target: &Path,
    max_bytes: u64,
) -> Result<Option<u64>, CopyError> {
    copy_image_with(source, target, max_bytes, |from, to| {
        sys::ioctl_ficlone(to, from)
    })
}

/// [`copy_image`] with the reflink step injected, so both of its outcomes are testable on any filesystem.
fn copy_image_with(
    source: &Path,
    target: &Path,
    max_bytes: u64,
    clone: impl FnOnce(&OwnedFd, &OwnedFd) -> rustix::io::Result<()>,
) -> Result<Option<u64>, CopyError> {
    let from = match sys::open(source, SOURCE, Mode::empty()) {
        Err(Errno::NOENT) => return Ok(None),
        from => from?,
    };
    let stat = sys::fstat(&from)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(CopyError::Io(std::io::Error::other(
            "a workspace image must be a regular file",
        )));
    }
    if allocation(&stat) > max_bytes {
        return Err(CopyError::Exceeded);
    }
    let temporary = temporary_path(target)?;
    let copied = write_copy(&from, &temporary, target, max_bytes, clone);
    if copied.is_err() {
        // The temporary file is this call's own; a failed removal leaves only a hidden file for a later delete.
        let _ = std::fs::remove_file(&temporary);
    }
    copied.map(Some)
}

fn write_copy(
    from: &OwnedFd,
    temporary: &Path,
    target: &Path,
    max_bytes: u64,
    clone: impl FnOnce(&OwnedFd, &OwnedFd) -> rustix::io::Result<()>,
) -> Result<u64, CopyError> {
    // A workspace image has a fixed size: the one `mkfs` gave it.
    let length = u64::try_from(sys::fstat(from)?.st_size).unwrap_or(0);
    let to = sys::open(temporary, TARGET, Mode::from_raw_mode(0o600))?;
    match clone(from, &to) {
        Ok(()) => (),
        Err(errno) if falls_back(errno) => sparse_copy(from, &to, length, max_bytes)?,
        Err(errno) => return Err(errno.into()),
    }
    sys::fsync(&to)?;
    let allocated = allocation(&sys::fstat(&to)?);
    if allocated > max_bytes {
        return Err(CopyError::Exceeded);
    }
    sys::renameat_with(
        sys::CWD,
        temporary,
        sys::CWD,
        target,
        RenameFlags::NOREPLACE,
    )?;
    Ok(allocated)
}

/// Whether a failed reflink means "not offered here" (fall back to copying) rather than a real failure.
const fn falls_back(errno: Errno) -> bool {
    matches!(
        errno,
        Errno::OPNOTSUPP | Errno::NOTTY | Errno::XDEV | Errno::INVAL | Errno::NOSYS
    )
}

/// Copies only the data extents of `from`, leaving holes as holes, bounded by `max_bytes` of copied data.
fn sparse_copy(from: &OwnedFd, to: &OwnedFd, length: u64, max_bytes: u64) -> Result<(), CopyError> {
    sys::ftruncate(to, length)?;
    let (mut offset, mut copied) = (0, 0_u64);
    // `<=` here would be an equivalent mutant: at `offset == length` the data seek ends the copy with ENXIO.
    while offset < length {
        let data = match sys::seek(from, SeekFrom::Data(offset)) {
            // No data after `offset`: the rest is a hole.
            Err(Errno::NXIO) => return Ok(()),
            data => data?,
        };
        let hole = sys::seek(from, SeekFrom::Hole(data))?.min(length);
        let mut position = data;
        while position < hole {
            let step = usize::try_from(hole - position)
                .unwrap_or(usize::MAX)
                .min(COPY_CHUNK_BYTES);
            let (mut input, mut output) = (position, position);
            let moved = sys::copy_file_range(from, Some(&mut input), to, Some(&mut output), step)?;
            if moved == 0 {
                // The source ended early (it cannot grow or shrink while copied, but never loop on it).
                return Ok(());
            }
            let moved = u64::try_from(moved).unwrap_or(u64::MAX);
            (position, copied) = (position + moved, copied.saturating_add(moved));
            if copied > max_bytes {
                return Err(CopyError::Exceeded);
            }
        }
        offset = hole;
    }
    Ok(())
}

/// `st_blocks` in bytes: what the image occupies on the host disk, the Firecracker workspace size measure.
fn allocation(stat: &sys::Stat) -> u64 {
    u64::try_from(stat.st_blocks)
        .unwrap_or(0)
        .saturating_mul(BLOCK_BYTES)
}

/// `.<name>.tqa-copy` beside `target`, so the copy is never visible under its final name before it is complete.
fn temporary_path(target: &Path) -> Result<PathBuf, CopyError> {
    let name = target
        .file_name()
        .ok_or_else(|| CopyError::Io(std::io::Error::other("a fork target needs a file name")))?;
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(".tqa-copy");
    Ok(target.with_file_name(temporary))
}
