//! A person's own changes to a workspace's memory notes (ADR-0041): write, remove and one Undo, on the host's files
//! by directory handle, never through a link, as uploads are placed. The agent's saves go through the file tools,
//! which keep the same one previous version (`builtin::memory_note`), so an Undo works on either's change.
use rustix::fs::{self as sys, AtFlags, FileType, Mode, OFlags, RenameFlags};
use rustix::io::Errno;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use super::{TreeError, path};

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;

/// The notes' folder, a child of the workspace root.
pub const DIRECTORY: &str = ".memory";
/// One previous version of each note, a child of [`DIRECTORY`]; hidden, so no index lists it.
pub const UNDO: &str = ".undo";
/// The longest note name, in bytes.
pub const MAX_NAME_BYTES: usize = 128;
/// The largest note, in bytes.
pub const MAX_NOTE_BYTES: usize = 64 * 1024;
const TEMPORARY: &str = ".note-part";
/// The suffix of the mark in `.undo/` that says a note was new at its last change. No note can have such a name (a
/// note ends `.md`), so the mark never collides with a kept version. The file tools write the same mark.
pub const NEW_SUFFIX: &str = ".new";

/// Where task digests live, from the workspace root: under `.tqa/`, which the file view, the change list and the
/// runtime link keep private as a whole, then one folder per person (ADR-0041 section 3). A digest holds a task's
/// title and the first lines of its answer, which belong to the person, not to everyone the workspace is shared with.
pub const TASKS: [&str; 3] = [".tqa", "memory", "tasks"];
/// The largest digest, in bytes.
pub const MAX_DIGEST_BYTES: usize = 16 * 1024;
const DIGEST_TEMPORARY: &str = ".digest-part";

/// What an Undo did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Undone {
    /// The note's previous version is back.
    Restored,
    /// The note had no previous version (it was new), so it is gone.
    Removed,
}

/// Whether `name` may name a note: one component of `[A-Za-z0-9._-]`, not hidden, ending `.md`, within
/// [`MAX_NAME_BYTES`].
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && !name.starts_with('.')
        && name.len() > 3
        && name[name.len() - 3..].eq_ignore_ascii_case(".md")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Opens `.memory/`, creating it (mode 0755) when missing; a link there is refused.
///
/// # Errors
/// `PathInvalid` when `.memory` is a link; host I/O failures.
pub fn directory(root: BorrowedFd<'_>) -> Result<OwnedFd, TreeError> {
    child(root, DIRECTORY)
}

fn child(parent: BorrowedFd<'_>, name: &str) -> Result<OwnedFd, TreeError> {
    match sys::mkdirat(parent, name, Mode::from_raw_mode(0o755)) {
        Ok(()) | Err(Errno::EXIST) => {}
        Err(_) => return Err(TreeError::Io),
    }
    path::child_dir(parent, name)
}

/// Whether `name` is a regular file of `directory` (never following a link).
fn is_note(directory: BorrowedFd<'_>, name: &str) -> Result<bool, TreeError> {
    match sys::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => {
            if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile {
                Ok(true)
            } else {
                Err(TreeError::PathInvalid)
            }
        }
        Err(Errno::NOENT) => Ok(false),
        Err(_) => Err(TreeError::Io),
    }
}

/// Reads a note's text; `None` when there is none.
///
/// # Errors
/// `PathInvalid` for a name that can't be a note, a link, anything but a regular file, or a note over
/// [`MAX_NOTE_BYTES`]; host I/O failures.
pub fn read(memory: BorrowedFd<'_>, name: &str) -> Result<Option<String>, TreeError> {
    if !valid_name(name) {
        return Err(TreeError::PathInvalid);
    }
    if !is_note(memory, name)? {
        return Ok(None);
    }
    let file = path::child_file(memory, name)?;
    let mut bytes = Vec::new();
    File::from(file)
        .take(MAX_NOTE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| TreeError::Io)?;
    if bytes.len() > MAX_NOTE_BYTES {
        return Err(TreeError::PathInvalid);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| TreeError::PathInvalid)
}

/// Keeps `name`'s current version in `.undo/` (replacing an older kept one); with no current version, drops any
/// kept one and marks the note as new, so an Undo of the note about to be written removes it.
fn keep(memory: BorrowedFd<'_>, name: &str) -> Result<(), TreeError> {
    let undo = child(memory, UNDO)?;
    let mark = new_mark(name);
    if is_note(memory, name)? {
        forget(undo.as_fd(), &mark)?;
        sys::renameat(memory, name, &undo, name).map_err(|_| TreeError::Io)
    } else {
        forget(undo.as_fd(), name)?;
        let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        sys::openat(&undo, mark.as_str(), flags, Mode::from_raw_mode(0o644))
            .map(drop)
            .map_err(|_| TreeError::Io)
    }
}

/// The mark in `.undo/` that says `name` was new at its last change: an Undo then removes it. Without the mark and
/// without a kept version there is nothing to undo, so a second Undo never removes a restored note.
fn new_mark(name: &str) -> String {
    format!("{name}{NEW_SUFFIX}")
}

/// Removes `name` from `directory` when it is there.
fn forget(directory: BorrowedFd<'_>, name: &str) -> Result<(), TreeError> {
    match sys::unlinkat(directory, name, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => Ok(()),
        Err(_) => Err(TreeError::Io),
    }
}

/// Writes a note, keeping its previous version for one Undo. The text is written to a temporary file and renamed
/// into place, so a reader never sees half a note.
///
/// # Errors
/// `PathInvalid` for a name that can't be a note, a text over [`MAX_NOTE_BYTES`], or a link in the way; host I/O
/// failures.
pub fn write(memory: BorrowedFd<'_>, name: &str, text: &str) -> Result<(), TreeError> {
    if !valid_name(name) || text.len() > MAX_NOTE_BYTES {
        return Err(TreeError::PathInvalid);
    }
    let _ = sys::unlinkat(memory, TEMPORARY, AtFlags::empty());
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut part = File::from(
        sys::openat(memory, TEMPORARY, flags, Mode::from_raw_mode(0o644))
            .map_err(|_| TreeError::Io)?,
    );
    let written = part
        .write_all(text.as_bytes())
        .and_then(|()| part.sync_all());
    let placed = written
        .map_err(|_| TreeError::Io)
        .and_then(|()| keep(memory, name))
        .and_then(|()| {
            sys::renameat_with(memory, TEMPORARY, memory, name, RenameFlags::NOREPLACE)
                .map_err(|_| TreeError::Io)
        });
    if placed.is_err() {
        let _ = sys::unlinkat(memory, TEMPORARY, AtFlags::empty());
    }
    placed
}

/// Removes a note, keeping it for one Undo.
///
/// # Errors
/// `NotFound` when there is no such note; `PathInvalid` for a name that can't be a note or a link in the way; host
/// I/O failures.
pub fn remove(memory: BorrowedFd<'_>, name: &str) -> Result<(), TreeError> {
    if !valid_name(name) {
        return Err(TreeError::PathInvalid);
    }
    if !is_note(memory, name)? {
        return Err(TreeError::NotFound);
    }
    keep(memory, name)
}

/// Undoes the last change to a note, once: its kept previous version comes back, or a note that was new at that
/// change is removed. A second Undo finds nothing kept and changes nothing.
///
/// # Errors
/// `NotFound` when there is nothing to undo (no kept version, and the note is not marked new); `PathInvalid` for a
/// name that can't be a note or a link in the way; host I/O failures.
pub fn undo(memory: BorrowedFd<'_>, name: &str) -> Result<Undone, TreeError> {
    if !valid_name(name) {
        return Err(TreeError::PathInvalid);
    }
    let undo = child(memory, UNDO)?;
    if is_note(undo.as_fd(), name)? {
        sys::renameat(&undo, name, memory, name).map_err(|_| TreeError::Io)?;
        return Ok(Undone::Restored);
    }
    let mark = new_mark(name);
    if is_note(undo.as_fd(), &mark)? && is_note(memory, name)? {
        sys::unlinkat(memory, name, AtFlags::empty()).map_err(|_| TreeError::Io)?;
        forget(undo.as_fd(), &mark)?;
        return Ok(Undone::Removed);
    }
    Err(TreeError::NotFound)
}

/// Whether `owner` may name a person's digest folder: a person's id as text (hex digits and dashes, 1 to 64 bytes).
#[must_use]
pub fn valid_owner(owner: &str) -> bool {
    (1..=64).contains(&owner.len())
        && owner
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

/// Opens one person's digest folder, `.tqa/memory/tasks/<owner>/`, creating what is missing; a link at any step is
/// refused.
///
/// # Errors
/// `PathInvalid` for an owner that can't name a folder or a link in the way; host I/O failures.
pub fn tasks_directory(root: BorrowedFd<'_>, owner: &str) -> Result<OwnedFd, TreeError> {
    if !valid_owner(owner) {
        return Err(TreeError::PathInvalid);
    }
    let mut directory = child(root, TASKS[0])?;
    for name in TASKS[1..].iter().copied().chain([owner]) {
        directory = child(directory.as_fd(), name)?;
    }
    Ok(directory)
}

/// Writes a digest, replacing the task's earlier one: to a temporary file, then renamed into place.
///
/// # Errors
/// `PathInvalid` for a name that can't be a note's, a text over [`MAX_DIGEST_BYTES`], or a link in the way; host I/O
/// failures.
pub fn write_digest(tasks: BorrowedFd<'_>, name: &str, text: &str) -> Result<(), TreeError> {
    if !valid_name(name) || text.len() > MAX_DIGEST_BYTES {
        return Err(TreeError::PathInvalid);
    }
    // A link where the digest goes is never written through: a rename replaces the link itself, and the read side
    // refuses one; anything else there that isn't a regular file is refused here.
    match sys::statat(tasks, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile => {
            return Err(TreeError::PathInvalid);
        }
        Ok(_) | Err(Errno::NOENT) => {}
        Err(_) => return Err(TreeError::Io),
    }
    let _ = sys::unlinkat(tasks, DIGEST_TEMPORARY, AtFlags::empty());
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut part = File::from(
        sys::openat(tasks, DIGEST_TEMPORARY, flags, Mode::from_raw_mode(0o644))
            .map_err(|_| TreeError::Io)?,
    );
    let placed = part
        .write_all(text.as_bytes())
        .and_then(|()| part.sync_all())
        .map_err(|_| TreeError::Io)
        .and_then(|()| {
            sys::renameat(tasks, DIGEST_TEMPORARY, tasks, name).map_err(|_| TreeError::Io)
        });
    if placed.is_err() {
        let _ = sys::unlinkat(tasks, DIGEST_TEMPORARY, AtFlags::empty());
    }
    placed
}

/// Reads a digest's text; `None` when the task has none yet.
///
/// # Errors
/// As [`read`].
pub fn read_digest(tasks: BorrowedFd<'_>, name: &str) -> Result<Option<String>, TreeError> {
    read(tasks, name)
}
