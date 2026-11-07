//! The install tier of read-only skills, resolved once at startup. It is refused when it could expose the gateway's
//! storage, the installation's secret files or its configuration to sandboxes: the root and every protected location
//! are canonicalised before comparison, so a symlink cannot hide an overlap (ARCHITECTURE §4.5).
use std::path::{Path, PathBuf};

use crate::GatewayError;

#[cfg(all(test, unix))]
#[path = "skills_tests.rs"]
mod tests;

/// The protected locations: the storage root, and the directories holding the install key, the optional seed file and
/// the model registry (which names `key_ref`s and internal hosts, though no secret values).
pub(crate) fn protected(storage_root: &Path, files: &[&Path]) -> Vec<PathBuf> {
    let mut protected = vec![storage_root.to_path_buf()];
    for file in files {
        let parent = file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        protected.push(parent.unwrap_or(Path::new(".")).to_path_buf());
    }
    protected
}

/// Resolves the configured root, which must be an existing directory, and refuses any overlap, in either direction,
/// with a protected location. A protected location that cannot be resolved is refused too.
///
/// # Errors
/// `Configuration` for a missing root, a root that is not a directory, or an overlap.
pub(crate) async fn resolve_install_tier(
    root: &Path,
    protected: &[PathBuf],
) -> Result<PathBuf, GatewayError> {
    let missing = || GatewayError::Configuration("skills.root must be an existing directory");
    let root = tokio::fs::canonicalize(root).await.map_err(|_| missing())?;
    if !tokio::fs::metadata(&root)
        .await
        .map_err(|_| missing())?
        .is_dir()
    {
        return Err(missing());
    }
    for location in protected {
        let location = tokio::fs::canonicalize(location).await.map_err(|_| {
            GatewayError::Configuration(
                "a location the skills root is checked against cannot be resolved",
            )
        })?;
        if location.starts_with(&root) || root.starts_with(&location) {
            return Err(GatewayError::Configuration(
                "the skills root must not expose the installation's storage, secrets or configuration",
            ));
        }
    }
    Ok(root)
}
