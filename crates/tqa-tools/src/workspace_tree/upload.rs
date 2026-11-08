//! Upload names and placement in a workspace's `uploads/` (FR-36, GW-I-045), shared by the gateway (Docker workspaces)
//! and the runtime (microVM workspaces, ADR-0030) so both name files the same way. A name is sanitised to the
//! attachment contract, and a finished file is linked to the first free name (`name-2.ext`), never overwriting one.
use rustix::fs::{self as sys, AtFlags, FileType, Mode, OFlags};
use rustix::io::Errno;
use std::fs::File;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::time::{Duration, SystemTime};

use super::{
    TreeError,
    path::{self, Kind},
};

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;

/// The longest stored name, in bytes (the attachment contract's `<name>`).
pub const MAX_NAME_BYTES: usize = 128;
/// The directory, relative to the workspace root, that uploads land in.
pub const DIRECTORY: &str = "uploads";
/// The prefix of an upload's hidden file while it is written.
pub const TEMPORARY_PREFIX: &str = ".upload-";
/// How many `-N` suffixes are tried before giving up.
const MAX_SUFFIX: u32 = 1000;

/// The last path segment of a client-supplied name, reduced to `[A-Za-z0-9._-]` (every other character becomes
/// `_`), without leading dots, at most [`MAX_NAME_BYTES`] with its extension kept; `file` when nothing is left.
#[must_use]
pub fn sanitise(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let mapped: String = last
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = mapped.trim_start_matches('.');
    if trimmed.is_empty() {
        return "file".to_owned();
    }
    shorten(trimmed, MAX_NAME_BYTES)
}

/// Cuts a sanitised (ASCII) name to `limit` bytes, keeping a short extension such as `.tar.gz`.
fn shorten(name: &str, limit: usize) -> String {
    if name.len() <= limit {
        return name.to_owned();
    }
    let extension = name
        .find('.')
        .map(|dot| &name[dot..])
        .filter(|extension| extension.len() <= 16)
        .unwrap_or_default();
    format!("{}{extension}", &name[..limit - extension.len()])
}

/// `stem-N.ext` for the Nth attempt (`N` ≥ 2), within the name limit.
fn numbered(name: &str, attempt: u32) -> String {
    let suffix = format!("-{attempt}");
    let (stem, extension) = name
        .find('.')
        .map_or((name, ""), |dot| (&name[..dot], &name[dot..]));
    let stem = shorten(stem, MAX_NAME_BYTES - extension.len() - suffix.len());
    format!("{stem}{suffix}{extension}")
}

/// Opens `uploads/`, creating it (mode 0755) when missing; a link there is refused.
///
/// # Errors
/// `PathInvalid` when `uploads` is a link; host I/O failures.
pub fn directory(root: BorrowedFd<'_>) -> Result<OwnedFd, TreeError> {
    match sys::mkdirat(root, DIRECTORY, Mode::from_raw_mode(0o755)) {
        Ok(()) | Err(Errno::EXIST) => {}
        Err(_) => return Err(TreeError::Io),
    }
    path::walk(root, &[DIRECTORY.to_owned()], |_| ())
}

/// Bytes stored in `uploads/` (regular files only).
///
/// # Errors
/// Host I/O failures.
pub fn used_in(directory: BorrowedFd<'_>) -> Result<u64, TreeError> {
    Ok(path::list(directory)?
        .iter()
        .filter(|entry| entry.kind == Kind::File)
        .map(|entry| entry.size)
        .sum())
}

/// Links the finished temporary file to the first free name; `linkat` fails on an existing name, so nothing is
/// ever overwritten. `None` when every numbered name is taken.
///
/// # Errors
/// Host I/O failures.
pub fn link(
    directory: BorrowedFd<'_>,
    temporary: &str,
    name: &str,
) -> Result<Option<String>, TreeError> {
    for attempt in 1..=MAX_SUFFIX {
        let candidate = if attempt == 1 {
            name.to_owned()
        } else {
            numbered(name, attempt)
        };
        match sys::linkat(
            directory,
            temporary,
            directory,
            candidate.as_str(),
            AtFlags::empty(),
        ) {
            Ok(()) => return Ok(Some(candidate)),
            Err(Errno::EXIST) => {}
            Err(_) => return Err(TreeError::Io),
        }
    }
    Ok(None)
}

/// Creates an upload's part file (mode 0644) in `uploads/`, never over an existing entry and never through a link.
///
/// # Errors
/// `PathInvalid` when anything already has the name; host I/O failures.
pub fn create_part(directory: BorrowedFd<'_>, name: &str) -> Result<File, TreeError> {
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    sys::openat(directory, name, flags, Mode::from_raw_mode(0o644))
        .map(File::from)
        .map_err(|errno| match errno {
            Errno::EXIST => TreeError::PathInvalid,
            _ => TreeError::Io,
        })
}

/// Opens an upload's existing part file for writing, and its size; only a regular file with one link, never through a
/// symbolic link.
///
/// # Errors
/// `NotFound` when there is none; `PathInvalid` for a link, a hard-linked file or anything but a regular file; host
/// I/O failures.
pub fn open_part(directory: BorrowedFd<'_>, name: &str) -> Result<(File, u64), TreeError> {
    // Non-blocking, so a planted FIFO without a reader fails instead of waiting.
    let flags = OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let fd = sys::openat(directory, name, flags, Mode::empty()).map_err(|errno| match errno {
        Errno::NOENT => TreeError::NotFound,
        Errno::LOOP | Errno::MLINK | Errno::ISDIR => TreeError::PathInvalid,
        _ => TreeError::Io,
    })?;
    let stat = sys::fstat(&fd).map_err(|_| TreeError::Io)?;
    // One link only: a part hard-linked to another file would write the upload into that file.
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_nlink != 1 {
        return Err(TreeError::PathInvalid);
    }
    Ok((File::from(fd), u64::try_from(stat.st_size).unwrap_or(0)))
}

/// Removes part files (regular `.upload-*` files, never a link or directory) not written for `age` before `now`:
/// uploads that were abandoned. Returns how many it removed.
///
/// # Errors
/// Host I/O failures listing `uploads/`.
pub fn sweep_parts(
    directory: BorrowedFd<'_>,
    now: SystemTime,
    age: Duration,
) -> Result<usize, TreeError> {
    let cutoff = now.checked_sub(age).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut removed = 0;
    for entry in path::list(directory)? {
        let modified = u64::try_from(entry.modified)
            .ok()
            .and_then(|seconds| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if entry.kind == Kind::File
            && entry.name.starts_with(TEMPORARY_PREFIX)
            && modified < cutoff
            && sys::unlinkat(directory, entry.name.as_str(), AtFlags::empty()).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Removes an upload's part file once it is linked into place (or abandoned).
///
/// # Errors
/// `NotFound` when it is gone; host I/O failures.
pub fn remove_part(directory: BorrowedFd<'_>, name: &str) -> Result<(), TreeError> {
    sys::unlinkat(directory, name, AtFlags::empty()).map_err(|errno| match errno {
        Errno::NOENT => TreeError::NotFound,
        _ => TreeError::Io,
    })
}
