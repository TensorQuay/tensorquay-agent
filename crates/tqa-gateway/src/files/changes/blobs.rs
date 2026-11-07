//! Content-addressed copies of snapshotted text files: `<sha256 hex>` under the workspace's changes directory, so a
//! file that did not change between snapshots is stored once. Gateway storage, never inside the workspace.
use crate::GatewayError;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, io::Write, path::PathBuf};
use tqa_tools::workspace_tree::manifest::hex;

pub(crate) struct Blobs {
    directory: PathBuf,
}

impl Blobs {
    pub(crate) const fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// Stores `bytes` once and returns their hash.
    ///
    /// # Errors
    /// Host I/O failures.
    pub(crate) fn put(&self, bytes: &[u8]) -> Result<String, GatewayError> {
        let hash = hex(&Sha256::digest(bytes));
        let path = self.directory.join(&hash);
        if path.is_file() {
            return Ok(hash);
        }
        fs::create_dir_all(&self.directory)?;
        // Written aside and renamed, so a reader never sees a partial copy.
        let temporary = self
            .directory
            .join(format!(".{hash}.{}", uuid::Uuid::now_v7()));
        let mut file = fs::File::create_new(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        Ok(hash)
    }

    /// Whether a copy of `hash` is stored.
    pub(crate) fn contains(&self, hash: &str) -> bool {
        hash.len() == 64
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && self.directory.join(hash).is_file()
    }

    /// Reads one copy.
    ///
    /// # Errors
    /// `404` for anything but a stored hash; host I/O failures.
    pub(crate) fn get(&self, hash: &str) -> Result<Vec<u8>, GatewayError> {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(GatewayError::NotFound);
        }
        fs::read(self.directory.join(hash)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GatewayError::NotFound
            } else {
                GatewayError::from(error)
            }
        })
    }

    /// Removes every copy (and interrupted write) that `keep` does not name.
    ///
    /// # Errors
    /// Host I/O failures.
    pub(crate) fn prune(&self, keep: &HashSet<String>) -> Result<(), GatewayError> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            let keep = entry
                .file_name()
                .to_str()
                .is_some_and(|name| keep.contains(name));
            if !keep {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}
