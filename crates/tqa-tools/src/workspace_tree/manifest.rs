//! A workspace manifest: every regular file's size, change stamp and, for small files, content hash, walked with the
//! same no-follow directory descriptors as the file view. Small UTF-8 files are handed to a store callback so a later
//! diff can show them; the gateway stores them as blobs, the runtime (ADR-0030) reports their hashes.
use super::{
    TreeError,
    path::{self, Kind},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    os::{
        fd::{AsFd, BorrowedFd},
        unix::fs::MetadataExt,
    },
};

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;

/// Left out at the root: other tasks' private state. The person's uploads are walked: a run that edits or deletes one
/// changed it. Which of their files a run's changes list is decided when two manifests are compared: a file added
/// under the uploads directory is the person's own upload (uploads never overwrite), never the run's change.
const SKIPPED_AT_ROOT: [&str; 1] = [".tqa"];
/// Left out at any depth: version-control internals, installed packages and well-known generated caches (they are noise
/// in every task's changes).
const SKIPPED: [&str; 8] = [
    ".git",
    "node_modules",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".venv",
];
/// Deeper trees are recorded in part, like a workspace over the file bound.
const MAX_DEPTH: usize = 64;

/// What a workspace held at one moment: each regular file's record, by path.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Manifest {
    /// Workspace-relative path to its record, in path order.
    pub files: BTreeMap<String, Record>,
    /// A bound stopped the walk; files beyond it are not recorded.
    pub truncated: bool,
}

/// One regular file in a manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Record {
    /// Size in bytes.
    pub size: u64,
    /// Modification and change times (seconds, nanoseconds) and the inode: an equal stamp means an unchanged file.
    pub stamp: (i64, i64, i64, i64, u64),
    /// SHA-256 of the content, for files up to the text bound.
    pub hash: Option<String>,
    /// UTF-8 without NUL.
    pub text: bool,
    /// A copy is stored under `hash`.
    pub copied: bool,
}

/// The bounds of one walk.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Files recorded; the manifest is truncated beyond them.
    pub files: usize,
    /// Directory entries of any kind listed during the walk (the work bound; files, links, FIFOs and directories).
    pub entries: usize,
    /// Files up to this size are hashed and, when UTF-8 text, handed to the store.
    pub text_bytes: u64,
    /// Bytes handed to the store per walk; later text files are recorded without a copy.
    pub copy_bytes: u64,
    /// The walk stops here and marks the manifest truncated.
    pub deadline: Option<std::time::Instant>,
}

/// Records the workspace below `root`. `previous` lets unchanged files keep their record without being read again.
/// `store` receives each small UTF-8 file's bytes while the copy budget lasts; its error ends the walk.
///
/// # Errors
/// Host I/O failures and `store`'s errors; entries that vanish or turn into links during the walk are skipped.
pub fn scan<E: From<TreeError>>(
    root: BorrowedFd<'_>,
    previous: Option<&Manifest>,
    limits: &Limits,
    store: &mut dyn FnMut(&[u8]) -> Result<(), E>,
) -> Result<Manifest, E> {
    let mut walk = Walk {
        manifest: Manifest::default(),
        previous,
        limits,
        store,
        copied: 0,
        listed: 0,
    };
    let start = path::walk(root, &[], |_| ())?;
    walk.directory(start.as_fd(), "", 0)?;
    Ok(walk.manifest)
}

/// Lowercase hexadecimal, the form hashes take in manifests and blob names.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

struct Walk<'a, E> {
    manifest: Manifest,
    previous: Option<&'a Manifest>,
    limits: &'a Limits,
    store: &'a mut dyn FnMut(&[u8]) -> Result<(), E>,
    copied: u64,
    /// Entries of any kind listed so far, against `limits.entries`.
    listed: usize,
}

impl<E: From<TreeError>> Walk<'_, E> {
    fn directory(
        &mut self,
        directory: BorrowedFd<'_>,
        prefix: &str,
        depth: usize,
    ) -> Result<(), E> {
        if depth > MAX_DEPTH || self.late() {
            self.manifest.truncated = true;
            return Ok(());
        }
        // Every listed entry counts, whatever it is: a tree of empty directories or links is work too.
        let budget = self.limits.entries.saturating_sub(self.listed);
        let (mut entries, more) = path::list_at_most(directory, budget)?;
        self.listed += entries.len();
        if more {
            self.manifest.truncated = true;
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            if self.late() {
                self.manifest.truncated = true;
            }
            if self.manifest.truncated {
                break;
            }
            let name = entry.name.as_str();
            if SKIPPED.contains(&name) || (depth == 0 && SKIPPED_AT_ROOT.contains(&name)) {
                continue;
            }
            // An upload still being written is nobody's change yet.
            if depth == 1
                && prefix == super::upload::DIRECTORY
                && name.starts_with(super::upload::TEMPORARY_PREFIX)
            {
                continue;
            }
            let path = if prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            match entry.kind {
                Kind::Dir => self.child(directory, name, &path, depth)?,
                Kind::File => self.file(directory, name, path)?,
                Kind::Link | Kind::Other => {}
            }
        }
        Ok(())
    }

    fn late(&self) -> bool {
        self.limits
            .deadline
            .is_some_and(|deadline| std::time::Instant::now() >= deadline)
    }

    /// A directory swapped for a link after listing is refused by the no-follow open and skipped.
    fn child(
        &mut self,
        directory: BorrowedFd<'_>,
        name: &str,
        path: &str,
        depth: usize,
    ) -> Result<(), E> {
        match path::child_dir(directory, name) {
            Ok(child) => self.directory(child.as_fd(), path, depth + 1),
            Err(_) => Ok(()),
        }
    }

    fn file(&mut self, directory: BorrowedFd<'_>, name: &str, path: String) -> Result<(), E> {
        if self.manifest.files.len() >= self.limits.files {
            self.manifest.truncated = true;
            return Ok(());
        }
        let Ok(file) = path::child_file(directory, name) else {
            return Ok(());
        };
        let file = std::fs::File::from(file);
        let metadata = file.metadata().map_err(|_| TreeError::Io)?;
        let size = metadata.len();
        let stamp = (
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
            metadata.ino(),
        );
        if let Some(record) = self
            .previous
            .and_then(|previous| previous.files.get(&path))
            .filter(|record| record.size == size && record.stamp == stamp)
        {
            self.manifest.files.insert(path, record.clone());
            return Ok(());
        }
        let mut record = Record {
            size,
            stamp,
            hash: None,
            text: false,
            copied: false,
        };
        if size <= self.limits.text_bytes {
            let mut bytes = Vec::new();
            file.take(self.limits.text_bytes + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| TreeError::Io)?;
            // A file that grew past the bound while being read is recorded like any large file.
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= self.limits.text_bytes {
                record.hash = Some(hex(&Sha256::digest(&bytes)));
                record.text = !bytes.contains(&0) && std::str::from_utf8(&bytes).is_ok();
                record.copied = record.text && self.copy(&bytes)?;
            }
        }
        self.manifest.files.insert(path, record);
        Ok(())
    }

    /// Stores a copy while the snapshot's copy budget lasts.
    fn copy(&mut self, bytes: &[u8]) -> Result<bool, E> {
        let cost = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if self.copied.saturating_add(cost) > self.limits.copy_bytes {
            return Ok(false);
        }
        (self.store)(bytes)?;
        self.copied += cost;
        Ok(true)
    }
}
