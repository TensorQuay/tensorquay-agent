//! Canonical path resolution for existing files and new descendants.
use crate::ToolError;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Maximum encoded path length accepted at the tool boundary.
pub const MAX_PATH_BYTES: usize = 4096;
/// Maximum path components before any filesystem traversal.
pub const MAX_PATH_COMPONENTS: usize = 64;

/// An existing absolute directory, retained in canonical form.
#[derive(Clone, Debug)]
pub struct WorkspaceRoot(PathBuf);

/// Below this much space available to the sandbox user, the workspace counts as full: a large write fails with ENOSPC
/// while a few blocks remain.
const SPACE_LOW_BYTES: u64 = 1 << 20;

/// Out of space: less than [`SPACE_LOW_BYTES`] available, or no inodes left. A filesystem that reports no inode
/// counts at all (btrfs) is never full by inodes.
const fn space_low(available_bytes: u64, inodes: u64, available_inodes: u64) -> bool {
    available_bytes < SPACE_LOW_BYTES || (inodes > 0 && available_inodes == 0)
}

impl WorkspaceRoot {
    /// Opens an absolute, existing directory and resolves its symlinks.
    ///
    /// # Errors
    /// Rejects relative, missing, inaccessible and non-directory roots.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ToolError> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(ToolError::PathInvalid("root must be absolute".into()));
        }
        let root = path.canonicalize()?;
        if !root.is_dir() {
            return Err(ToolError::PathInvalid("root must be a directory".into()));
        }
        Ok(Self(root))
    }

    /// Whether the workspace's filesystem is out of space for the sandbox user, as the kernel reports it (`statvfs`):
    /// see `space_low`. An unanswerable query is `false`: a full workspace is only ever claimed, never guessed. It
    /// blocks, so async callers run it off their threads with a bound.
    #[must_use]
    pub fn space_exhausted(&self) -> bool {
        rustix::fs::statvfs(&self.0).is_ok_and(|stats| {
            space_low(
                stats.f_bavail.saturating_mul(stats.f_frsize),
                stats.f_files,
                stats.f_favail,
            )
        })
    }

    /// Returns the canonical workspace directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Resolves symlinks before parent components and permits new descendants.
    ///
    /// # Errors
    /// Rejects malformed, overly deep, dangling, inaccessible and escaping paths.
    pub fn resolve(&self, user_path: &str) -> Result<PathBuf, ToolError> {
        if user_path.is_empty()
            || user_path.contains('\0')
            || user_path.len() > MAX_PATH_BYTES
            || user_path.split('/').filter(|part| !part.is_empty()).count() > MAX_PATH_COMPONENTS
        {
            return Err(ToolError::PathInvalid(
                "path length or components invalid".into(),
            ));
        }
        let path = Path::new(user_path);
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.0)
                .map_err(|_| ToolError::PathOutsideWorkspace)?
        } else {
            path
        };
        let mut resolved = self.0.clone();
        for component in relative.components() {
            match component {
                Component::CurDir | Component::RootDir | Component::Prefix(_) => continue,
                Component::ParentDir => {
                    resolved.pop();
                }
                Component::Normal(part) => {
                    resolved.push(part);
                    resolved = existing_prefix(resolved)?;
                }
            }
            if !resolved.starts_with(&self.0) {
                return Err(ToolError::PathOutsideWorkspace);
            }
        }
        Ok(resolved)
    }
}

fn existing_prefix(path: PathBuf) -> Result<PathBuf, ToolError> {
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(path.canonicalize()?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod space_tests {
    use super::{SPACE_LOW_BYTES, space_low};

    #[test]
    fn tool_u_026_space_is_low_below_the_margin_or_without_free_inodes() {
        assert!(space_low(0, 1000, 10));
        // A large write fails while a few blocks remain: that is out of space too.
        assert!(space_low(SPACE_LOW_BYTES - 1, 1000, 10));
        assert!(!space_low(SPACE_LOW_BYTES, 1000, 10));
        // No inodes left, with plenty of blocks (many small files).
        assert!(space_low(u64::MAX, 1000, 0));
        // A filesystem that reports no inode counts at all (btrfs) is never full by inodes.
        assert!(!space_low(u64::MAX, 0, 0));
    }
}
