use crate::GatewayError;
use serde::Deserialize;
use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};

/// Non-secret file locations supplied by the host, separately from the credential database.
#[derive(Clone, Debug)]
pub struct SecretFiles {
    pub(super) install_key: PathBuf,
    pub(super) seed: Option<PathBuf>,
}

impl SecretFiles {
    /// Validates absolute, traversal-free file paths without reading either file.
    ///
    /// # Errors
    /// Rejects empty, non-Unicode, root or oversized paths and parent traversal.
    pub fn new(install_key: PathBuf, seed: Option<PathBuf>) -> Result<Self, GatewayError> {
        validate(&install_key)?;
        if let Some(seed) = &seed {
            validate(seed)?;
        }
        Ok(Self { install_key, seed })
    }

    /// The install key's and the optional seed file's locations, for checks that must keep them out of sandboxes.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &std::path::Path> {
        std::iter::once(self.install_key.as_path()).chain(self.seed.as_deref())
    }
}

/// TOML-configurable secret-store bounds; defaults permit 4096 values of at most 8192 bytes each.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SecretStoreConfig {
    /// Per-operation deadline in seconds, default 10, maximum 300.
    pub io_timeout: u64,
    /// Maximum plaintext value bytes, default 8192, maximum 65536.
    pub value_bytes: usize,
    /// Maximum seed file bytes, default 1 MiB, maximum 16 MiB.
    pub seed_bytes: usize,
    /// Maximum stored references, default 4096, maximum 16384.
    pub references: u32,
    /// How long secret read records are kept, in days: default 30, 1 through 365 (vault slice 2).
    pub read_audit_days: u32,
    /// How long administration audit records are kept, in days: default 365, 1 through 3650.
    pub admin_audit_days: u32,
    /// Read records waiting to be written; past it a record is dropped and counted, never the read. Default 1024,
    /// 1 through 65536.
    pub read_audit_queue: usize,
}
impl Default for SecretStoreConfig {
    fn default() -> Self {
        Self {
            io_timeout: 10,
            value_bytes: 8192,
            seed_bytes: 1024 * 1024,
            references: 4096,
            read_audit_days: 30,
            admin_audit_days: 365,
            read_audit_queue: 1024,
        }
    }
}
impl SecretStoreConfig {
    pub(crate) fn validate(&self) -> Result<(), GatewayError> {
        if !(1..=300).contains(&self.io_timeout)
            || !(1..=65536).contains(&self.value_bytes)
            || !(1..=16 * 1024 * 1024).contains(&self.seed_bytes)
            || !(1..=16384).contains(&self.references)
        {
            return Err(GatewayError::Configuration(
                "secret-store bounds are invalid",
            ));
        }
        if !(1..=365).contains(&self.read_audit_days) {
            return Err(GatewayError::Configuration(
                "secret_store.read_audit_days must be 1 through 365",
            ));
        }
        if !(1..=3650).contains(&self.admin_audit_days) {
            return Err(GatewayError::Configuration(
                "secret_store.admin_audit_days must be 1 through 3650",
            ));
        }
        if !(1..=65536).contains(&self.read_audit_queue) {
            return Err(GatewayError::Configuration(
                "secret_store.read_audit_queue must be 1 through 65536",
            ));
        }
        Ok(())
    }
    pub(super) fn timeout(&self) -> Duration {
        Duration::from_secs(self.io_timeout)
    }
    pub(super) fn value(&self, value: &str) -> Result<(), GatewayError> {
        if value.is_empty() || value.len() > self.value_bytes {
            return Err(GatewayError::BadRequest("secret value exceeds its bounds"));
        }
        Ok(())
    }
}
fn validate(path: &Path) -> Result<(), GatewayError> {
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .to_str()
            .is_none_or(|value| value.len() > 4096 || value.contains('\0'))
        || path.components().any(|part| part == Component::ParentDir)
    {
        return Err(GatewayError::Configuration(
            "secret files require absolute bounded file paths without traversal",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
