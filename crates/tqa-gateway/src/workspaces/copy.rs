//! Bounded workspace walks (066c): the fork copy and the size measurement. Blocking; callers run them on the
//! blocking pool. The source is a directory a sandbox writes to, so it is walked one directory descriptor at a time
//! with `O_NOFOLLOW`: a link (or anything swapped in for an entry while the walk runs) is never followed. A link is
//! copied as a link; other special files are skipped. Each file is written under a temporary name and renamed, so a
//! fork never shows a half-written file.
use rustix::fs::{self as sys, AtFlags, FileType, Mode, OFlags};
use std::{
    cell::Cell,
    ffi::OsStr,
    fs,
    io::Read,
    os::{fd::OwnedFd, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
    rc::Rc,
};

const DIRECTORY: OFlags = OFlags::DIRECTORY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC)
    .union(OFlags::RDONLY);
// Non-blocking, so a FIFO swapped in for a file cannot stall the walk.
const FILE: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC)
    .union(OFlags::NONBLOCK);

/// The deepest directory a walk enters. A walk holds one open descriptor per level of the path it is in (never one
/// per pending directory), so this bounds the descriptors a workspace's shape can make the gateway hold.
pub(crate) const MAX_DEPTH: usize = 256;

/// Bounds for one walk: regular-file bytes (the quota) and directory entries.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub bytes: u64,
    pub entries: u64,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CopyError {
    /// The quota, the entry bound or the depth bound would be exceeded.
    #[error("the workspace quota, entry bound or depth bound would be exceeded")]
    Exceeded,
    #[error("workspace I/O failed")]
    Io(#[from] std::io::Error),
}

impl From<rustix::io::Errno> for CopyError {
    fn from(errno: rustix::io::Errno) -> Self {
        Self::Io(errno.into())
    }
}

/// One walk's running totals.
struct Walk {
    limits: Limits,
    bytes: u64,
    entries: u64,
    /// Whether bytes count against the limit (a copy) or are only reported (a measurement).
    bound_bytes: bool,
}

impl Walk {
    const fn new(limits: Limits, bound_bytes: bool) -> Self {
        Self {
            limits,
            bytes: 0,
            entries: 0,
            bound_bytes,
        }
    }

    const fn entry(&mut self) -> Result<(), CopyError> {
        self.entries += 1;
        if self.entries > self.limits.entries {
            return Err(CopyError::Exceeded);
        }
        Ok(())
    }

    const fn file(&mut self, length: u64) -> Result<(), CopyError> {
        self.bytes = self.bytes.saturating_add(length);
        if self.bound_bytes && self.bytes > self.limits.bytes {
            return Err(CopyError::Exceeded);
        }
        Ok(())
    }

    /// The bytes one more file may still add.
    const fn remaining(&self) -> u64 {
        self.limits.bytes.saturating_sub(self.bytes)
    }
}

/// An open directory of a walk; the walk counts how many it holds at once (at most one per level of its path).
struct Opened {
    fd: OwnedFd,
    open: Rc<Cell<(usize, usize)>>,
}

impl Opened {
    /// Opens `name` in `parent` (or `root` from the working directory) as a directory, never following a link. `None`
    /// when the entry is gone or no longer a directory since it was listed (a running workspace changes under a walk).
    fn at(
        parent: Option<&OwnedFd>,
        name: &[u8],
        open: &Rc<Cell<(usize, usize)>>,
    ) -> Result<Option<Self>, CopyError> {
        let fd = match parent {
            Some(parent) => sys::openat(parent, name, DIRECTORY, Mode::empty()),
            None => sys::openat(sys::CWD, name, DIRECTORY, Mode::empty()),
        };
        let fd = match fd {
            Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP)
                if parent.is_some() =>
            {
                return Ok(None);
            }
            fd => fd?,
        };
        let (now, peak) = open.get();
        open.set((now + 1, peak.max(now + 1)));
        Ok(Some(Self {
            fd,
            open: Rc::clone(open),
        }))
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        let (now, peak) = self.open.get();
        self.open.set((now.saturating_sub(1), peak));
    }
}

/// A directory still to walk: its parent (kept open only while it has pending children), its name, its depth, and
/// where a copy puts it.
struct Pending {
    parent: Rc<Opened>,
    name: Vec<u8>,
    depth: usize,
    to: PathBuf,
}

/// The pending child `name` of `parent`, refused past the depth bound.
fn descend(
    parent: &Rc<Opened>,
    name: Vec<u8>,
    depth: usize,
    to: PathBuf,
) -> Result<Pending, CopyError> {
    if depth >= MAX_DEPTH {
        return Err(CopyError::Exceeded);
    }
    Ok(Pending {
        parent: Rc::clone(parent),
        name,
        depth: depth + 1,
        to,
    })
}

/// The next pending directory, opened; its parent closes once no other child needs it. `None` when the walk is done.
fn next(
    pending: &mut Vec<Pending>,
    open: &Rc<Cell<(usize, usize)>>,
) -> Result<Option<(Rc<Opened>, usize, PathBuf)>, CopyError> {
    while let Some(child) = pending.pop() {
        if let Some(opened) = Opened::at(Some(&child.parent.fd), &child.name, open)? {
            return Ok(Some((Rc::new(opened), child.depth, child.to)));
        }
    }
    Ok(None)
}

/// The entries of an open directory, with their types as seen without following links.
fn entries(directory: &OwnedFd) -> Result<Vec<(Vec<u8>, FileType)>, CopyError> {
    let mut found = Vec::new();
    for entry in sys::Dir::read_from(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        // An entry removed since it was listed (a running workspace) is simply not there any more.
        let stat = match sys::statat(directory, entry.file_name(), AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => continue,
            stat => stat?,
        };
        found.push((name.to_vec(), FileType::from_raw_mode(stat.st_mode)));
    }
    Ok(found)
}

/// Copies the tree under `source` into the existing, gateway-created directory `target`; returns the regular-file
/// bytes copied.
///
/// # Errors
/// `Exceeded` past the byte, entry or depth bound, or the first I/O failure. The caller removes a partial target.
pub(crate) fn copy_tree(source: &Path, target: &Path, limits: Limits) -> Result<u64, CopyError> {
    copy_walk(source, target, limits).map(|(bytes, _)| bytes)
}

/// `copy_tree`, also returning the most directories the walk held open at once.
fn copy_walk(source: &Path, target: &Path, limits: Limits) -> Result<(u64, usize), CopyError> {
    let mut walk = Walk::new(limits, true);
    let open = Rc::new(Cell::new((0, 0)));
    let root =
        Opened::at(None, source.as_os_str().as_bytes(), &open)?.ok_or(CopyError::Exceeded)?;
    let mut current = Some((Rc::new(root), 0, target.to_path_buf()));
    let mut pending: Vec<Pending> = Vec::new();
    while let Some((directory, depth, to)) = match current.take() {
        Some(current) => Some(current),
        None => next(&mut pending, &open)?,
    } {
        for (name, kind) in entries(&directory.fd)? {
            walk.entry()?;
            let target_path = to.join(OsStr::from_bytes(&name));
            match kind {
                FileType::Directory => {
                    let child = descend(&directory, name, depth, target_path)?;
                    fs::create_dir(&child.to)?;
                    pending.push(child);
                }
                FileType::Symlink => {
                    let link = match sys::readlinkat(&directory.fd, name.as_slice(), Vec::new()) {
                        Err(rustix::io::Errno::NOENT) => continue,
                        link => link?,
                    };
                    std::os::unix::fs::symlink(OsStr::from_bytes(link.as_bytes()), &target_path)?;
                }
                FileType::RegularFile => {
                    copy_file(&directory.fd, &name, &to, &target_path, &mut walk)?;
                }
                _ => (),
            }
        }
    }
    Ok((walk.bytes, open.get().1))
}

/// Copies one regular file, opened without following links and checked again once open.
fn copy_file(
    directory: &OwnedFd,
    name: &[u8],
    to: &Path,
    target_path: &Path,
    walk: &mut Walk,
) -> Result<(), CopyError> {
    let file = match sys::openat(directory, name, FILE, Mode::empty()) {
        Err(rustix::io::Errno::NOENT) => return Ok(()),
        file => file?,
    };
    let stat = sys::fstat(&file)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        // Swapped for something else since it was listed: skip it, as any special file.
        return Ok(());
    }
    // Checked before copying, so one large file cannot overshoot the quota.
    let expected = u64::try_from(stat.st_size).unwrap_or(0);
    walk.file(expected)?;
    let mut temporary_name = b".".to_vec();
    temporary_name.extend_from_slice(name);
    temporary_name.extend_from_slice(b".tqa-copy");
    let temporary = to.join(OsStr::from_bytes(&temporary_name));
    let mut output = fs::File::create_new(&temporary)?;
    // Bounded: a file that grows while it is copied cannot take more than the quota allows.
    let mut input =
        fs::File::from(file).take(expected.saturating_add(walk.remaining()).saturating_add(1));
    let copied = std::io::copy(&mut input, &mut output)?;
    walk.file(copied.saturating_sub(expected))?;
    fs::rename(&temporary, target_path)?;
    Ok(())
}

/// What one entry costs the workspace's disk: its allocated blocks, and for a regular file at least its length (a
/// sparse file counts its full length). The same charge applies between runs and during one (N4, D2).
fn charged(stat: &sys::Stat) -> u64 {
    let blocks = u64::try_from(stat.st_blocks)
        .unwrap_or(0)
        .saturating_mul(512);
    if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile {
        blocks.max(u64::try_from(stat.st_size).unwrap_or(0))
    } else {
        blocks
    }
}

/// The bytes charged for everything under `root` (see `charged`), walking at most `limits.entries` entries and never
/// following a link.
///
/// # Errors
/// `Exceeded` past the entry or depth bound, or the first I/O failure.
pub(crate) fn measure(root: &Path, limits: Limits) -> Result<u64, CopyError> {
    measure_walk(root, limits, false).map(|(bytes, _)| bytes)
}

/// `measure` that stops as soon as the charge passes `limits.bytes`: a live check needs only to know a workspace is
/// past its hard limit, not by how much, so an oversized tree is not walked to its end.
///
/// # Errors
/// `Exceeded` past the byte, entry or depth bound, or the first I/O failure.
pub(crate) fn measure_within(root: &Path, limits: Limits) -> Result<u64, CopyError> {
    measure_walk(root, limits, true).map(|(bytes, _)| bytes)
}

/// `measure` (or `measure_within` when `bound_bytes`), also returning the most directories held open at once.
fn measure_walk(root: &Path, limits: Limits, bound_bytes: bool) -> Result<(u64, usize), CopyError> {
    let mut walk = Walk::new(limits, bound_bytes);
    let open = Rc::new(Cell::new((0, 0)));
    let root = Opened::at(None, root.as_os_str().as_bytes(), &open)?.ok_or(CopyError::Exceeded)?;
    let mut current = Some((Rc::new(root), 0, PathBuf::new()));
    let mut pending: Vec<Pending> = Vec::new();
    while let Some((directory, depth, _)) = match current.take() {
        Some(current) => Some(current),
        None => next(&mut pending, &open)?,
    } {
        for entry in sys::Dir::read_from(&directory.fd)? {
            let entry = entry?;
            let name = entry.file_name();
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            walk.entry()?;
            let stat = match sys::statat(&directory.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                Err(rustix::io::Errno::NOENT) => continue,
                stat => stat?,
            };
            match FileType::from_raw_mode(stat.st_mode) {
                FileType::Directory => {
                    walk.file(charged(&stat))?;
                    pending.push(descend(
                        &directory,
                        name.to_bytes().to_vec(),
                        depth,
                        PathBuf::new(),
                    )?);
                }
                _ => walk.file(charged(&stat))?,
            }
        }
    }
    Ok((walk.bytes, open.get().1))
}

#[cfg(test)]
#[path = "copy_tests.rs"]
mod tests;
