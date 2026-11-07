//! `[host_keys]` (ADR-0037 §6): where the install CA for keys to web addresses lives. A deployment mounts the install key
//! and the secrets seed read-only, so the CA can't sit beside them (GW-I-179); it has its own private directory, by
//! default `<storage.root>/host-ca`, which no sandbox mounts.
use crate::GatewayError;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct HostKeysInput {
    ca_directory: Option<PathBuf>,
}

/// The CA's directory: the configured one, or `<storage.root>/host-ca`.
///
/// # Errors
/// A relative path, one with `..`, or one inside a directory a sandbox mounts (the storage root's workspaces, runtime
/// state and skills, and the installation's skills), where a task could read the CA's private key.
pub(super) fn ca_directory(
    input: HostKeysInput,
    storage_root: &Path,
    skills_install: Option<&Path>,
) -> Result<PathBuf, GatewayError> {
    let Some(directory) = input.ca_directory else {
        return Ok(storage_root.join("host-ca"));
    };
    if !directory.is_absolute()
        || directory
            .components()
            .any(|part| part == Component::ParentDir)
    {
        return Err(GatewayError::Configuration(
            "host_keys.ca_directory must be an absolute path without ..",
        ));
    }
    let mounted = ["workspaces", "runtime-state", "skills"]
        .map(|name| storage_root.join(name))
        .into_iter()
        .chain(skills_install.map(Path::to_path_buf));
    if mounted
        .into_iter()
        .any(|place| directory.starts_with(place))
    {
        return Err(GatewayError::Configuration(
            "host_keys.ca_directory must not be inside a directory a sandbox mounts (workspaces, runtime state, skills)",
        ));
    }
    Ok(directory)
}

#[cfg(test)]
#[path = "host_keys_tests.rs"]
mod tests;
