//! Workspace-relative paths, resolved one directory fd at a time with `O_NOFOLLOW`, so a symbolic link planted by the
//! agent anywhere in a path - or swapped in while the walk is running - can never lead outside the workspace. The
//! final open always uses the walked directory's fd; nothing is resolved by path twice.
use super::TreeError;
use rustix::fs::{self as sys, AtFlags, FileType, Mode, OFlags};
use rustix::io::Errno;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;

/// The longest accepted relative path, in UTF-8 bytes.
pub const MAX_PATH_BYTES: usize = 1024;
/// Per-task spill files, run notes and logs (ARCHITECTURE §4.5); never part of the file view.
const PRIVATE_DIRECTORY: &str = ".tqa";

/// A validated path relative to the workspace root; empty for the root itself.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspacePath {
    parts: Vec<String>,
}

impl WorkspacePath {
    /// Accepts `""`, `"."` (the root) and `a/b/c` with an optional trailing `/`.
    ///
    /// # Errors
    /// `PathInvalid` for an absolute path, NUL, an empty, `.` or `..` segment, or more than
    /// [`MAX_PATH_BYTES`]; `NotFound` for anything under `.tqa/`.
    pub fn parse(raw: &str) -> Result<Self, TreeError> {
        if raw.is_empty() || raw == "." {
            return Ok(Self::default());
        }
        if raw.len() > MAX_PATH_BYTES || raw.starts_with('/') || raw.contains('\0') {
            return Err(TreeError::PathInvalid);
        }
        let trimmed = raw.strip_suffix('/').unwrap_or(raw);
        let parts: Vec<String> = trimmed.split('/').map(str::to_owned).collect();
        if parts
            .iter()
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(TreeError::PathInvalid);
        }
        if parts
            .first()
            .is_some_and(|first| first == PRIVATE_DIRECTORY)
        {
            return Err(TreeError::NotFound);
        }
        Ok(Self { parts })
    }

    /// The directories leading to the last component, and that component; `None` for the root.
    #[must_use]
    pub fn split_last(&self) -> Option<(&[String], &str)> {
        self.parts
            .split_last()
            .map(|(last, parents)| (parents, last.as_str()))
    }

    /// Whether this is the workspace root itself.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.parts.is_empty()
    }
}

impl std::fmt::Display for WorkspacePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.parts.join("/"))
    }
}

/// What a directory entry is, without following links.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link, never followed.
    Link,
    /// Anything else: a device, FIFO or socket.
    Other,
}

impl Kind {
    /// The kind's stable name on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "dir",
            Self::Link => "link",
            Self::Other => "other",
        }
    }
}

/// One directory entry; `size` and `modified` (Unix seconds) describe the entry itself, never a link's target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The entry's UTF-8 name.
    pub name: String,
    /// What the entry is, without following links.
    pub kind: Kind,
    /// The entry's own size in bytes.
    pub size: u64,
    /// The entry's own modification time, in Unix seconds.
    pub modified: i64,
}

const DIRECTORY: OFlags = OFlags::DIRECTORY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC)
    .union(OFlags::RDONLY);
const FILE: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC)
    .union(OFlags::NONBLOCK);

/// Opens the workspace root itself (a path the host built, never user input).
///
/// # Errors
/// `NotFound` while the workspace directory does not exist yet; `PathInvalid` if it is a link; host I/O failures.
pub fn open_root(root: &Path) -> Result<OwnedFd, TreeError> {
    sys::openat(sys::CWD, root, DIRECTORY, Mode::empty()).map_err(|errno| classify(errno, None))
}

/// Opens a directory of the workspace by walking from the root.
///
/// # Errors
/// As [`walk`].
pub fn open_dir(root: BorrowedFd<'_>, path: &WorkspacePath) -> Result<OwnedFd, TreeError> {
    walk(root, &path.parts, |_| ())
}

/// Opens a regular file of the workspace for reading.
///
/// # Errors
/// As [`open_file_with`].
pub fn open_file(
    root: BorrowedFd<'_>,
    path: &WorkspacePath,
) -> Result<(OwnedFd, sys::Stat), TreeError> {
    open_file_with(root, path, |_| ())
}

/// Opens a regular file; `between(depth)` runs before each directory is opened and once more (with the number of
/// directories) before the final open, which is how the tests swap a directory mid-walk.
///
/// # Errors
/// `PathInvalid` for the root, a link anywhere in the path or anything but a regular file; `NotFound` when a component is
/// missing; host I/O failures.
pub fn open_file_with(
    root: BorrowedFd<'_>,
    path: &WorkspacePath,
    mut between: impl FnMut(usize),
) -> Result<(OwnedFd, sys::Stat), TreeError> {
    let (parents, name) = path.split_last().ok_or(TreeError::PathInvalid)?;
    let directory = walk(root, parents, &mut between)?;
    between(parents.len());
    let file = sys::openat(&directory, name, FILE, Mode::empty())
        .map_err(|errno| classify(errno, Some((directory.as_fd(), name))))?;
    let stat = sys::fstat(&file).map_err(|_| TreeError::Io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(TreeError::PathInvalid);
    }
    Ok((file, stat))
}

/// Opens one child directory of `directory` without following a link.
///
/// # Errors
/// As [`walk`] for one component.
pub fn child_dir(directory: BorrowedFd<'_>, name: &str) -> Result<OwnedFd, TreeError> {
    sys::openat(directory, name, DIRECTORY, Mode::empty())
        .map_err(|errno| classify(errno, Some((directory, name))))
}

/// Opens one regular file of `directory` without following a link.
///
/// # Errors
/// As [`open_file_with`] for its last component.
pub fn child_file(directory: BorrowedFd<'_>, name: &str) -> Result<OwnedFd, TreeError> {
    let file = sys::openat(directory, name, FILE, Mode::empty())
        .map_err(|errno| classify(errno, Some((directory, name))))?;
    let stat = sys::fstat(&file).map_err(|_| TreeError::Io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(TreeError::PathInvalid);
    }
    Ok(file)
}

/// Walks `names` from `root`, one `openat(O_DIRECTORY | O_NOFOLLOW)` per component.
///
/// # Errors
/// `PathInvalid` when a component is a link; `NotFound` when it is missing or not a directory; host I/O failures.
pub fn walk(
    root: BorrowedFd<'_>,
    names: &[String],
    mut between: impl FnMut(usize),
) -> Result<OwnedFd, TreeError> {
    let mut current =
        sys::openat(root, ".", DIRECTORY, Mode::empty()).map_err(|_| TreeError::Io)?;
    for (depth, name) in names.iter().enumerate() {
        between(depth);
        current = sys::openat(&current, name.as_str(), DIRECTORY, Mode::empty())
            .map_err(|errno| classify(errno, Some((current.as_fd(), name))))?;
    }
    Ok(current)
}

/// Lists one directory without following links. Names that are not UTF-8 are left out (they cannot round-trip through
/// a JSON API), as are `.` and `..`.
///
/// # Errors
/// Host I/O failures.
pub fn list(directory: BorrowedFd<'_>) -> Result<Vec<Entry>, TreeError> {
    Ok(list_at_most(directory, usize::MAX)?.0)
}

/// Lists at most `limit` entries of one directory, without following links; `true` when more were left unread.
///
/// # Errors
/// Host I/O failures.
pub fn list_at_most(
    directory: BorrowedFd<'_>,
    limit: usize,
) -> Result<(Vec<Entry>, bool), TreeError> {
    let mut entries = Vec::new();
    for entry in sys::Dir::read_from(directory).map_err(|_| TreeError::Io)? {
        if entries.len() >= limit {
            return Ok((entries, true));
        }
        let entry = entry.map_err(|_| TreeError::Io)?;
        let Ok(name) = entry.file_name().to_str() else {
            continue;
        };
        if name == "." || name == ".." {
            continue;
        }
        // Only an entry removed between listing and this call fails here (a running workspace changes under us).
        let Ok(stat) = sys::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        entries.push(described(name, &stat));
    }
    Ok((entries, false))
}

/// One entry of a directory, described without following it (the workspace service's `stat`, ADR-0030).
///
/// # Errors
/// `NotFound` for a missing name; `PathInvalid` or host I/O as a walk would report them.
pub fn entry(directory: BorrowedFd<'_>, name: &str) -> Result<Entry, TreeError> {
    let stat = sys::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|errno| classify(errno, None))?;
    Ok(described(name, &stat))
}

fn described(name: &str, stat: &sys::Stat) -> Entry {
    Entry {
        name: name.to_owned(),
        kind: kind(stat),
        size: u64::try_from(stat.st_size).unwrap_or(0),
        modified: stat.st_mtime,
    }
}

/// Leaves out `.tqa/` (other tasks' spill files, run notes and logs) when listing the workspace root.
pub fn hide_private(entries: &mut Vec<Entry>, at_root: bool) {
    if at_root {
        entries.retain(|entry| entry.name != PRIVATE_DIRECTORY);
    }
}

/// What a `stat` result describes.
#[must_use]
pub fn kind(stat: &sys::Stat) -> Kind {
    match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => Kind::File,
        FileType::Directory => Kind::Dir,
        FileType::Symlink => Kind::Link,
        _ => Kind::Other,
    }
}

/// Maps an `openat` failure: a link is `PathInvalid`; a missing component or a file where a directory was expected
/// is `NotFound`; anything else is host I/O.
fn classify(errno: Errno, at: Option<(BorrowedFd<'_>, &str)>) -> TreeError {
    match errno {
        Errno::LOOP | Errno::MLINK => TreeError::PathInvalid,
        Errno::NOENT => TreeError::NotFound,
        Errno::NOTDIR => match at.and_then(|(directory, name)| {
            sys::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW).ok()
        }) {
            Some(stat) if kind(&stat) == Kind::Link => TreeError::PathInvalid,
            _ => TreeError::NotFound,
        },
        _ => TreeError::Io,
    }
}
