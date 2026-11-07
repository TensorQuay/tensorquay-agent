//! `[skills]`: the install tier of read-only skills, an operator directory every Docker sandbox mounts at
//! `/skills/install`. Each user's own tier lives under the storage root and needs no configuration. Here the path is
//! checked as written; at startup [`crate::skills::resolve_install_tier`] resolves it and every protected location,
//! so a symlink cannot hide an overlap.
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::GatewayError;

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct SkillsInput {
    /// An absolute host directory; in Compose it is bound into the gateway at the same path (deploy/compose/skills.yml).
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Skills {
    /// The install tier as configured; `None` means no install tier.
    pub root: Option<PathBuf>,
}

/// # Errors
/// Refuses a root that is relative, the filesystem root, not UTF-8, longer than 4096 bytes, traverses with `..`, or
/// overlaps the storage root in either direction.
pub(super) fn skills(input: SkillsInput, storage_root: &Path) -> Result<Skills, GatewayError> {
    let Some(root) = input.root else {
        return Ok(Skills::default());
    };
    let valid = root.is_absolute()
        && root.parent().is_some()
        && root
            .to_str()
            .is_some_and(|value| value.len() <= 4096 && !value.contains('\0'))
        && !root.components().any(|part| part == Component::ParentDir);
    if !valid {
        return Err(GatewayError::Configuration(
            "skills.root must be an absolute, non-root directory without traversal",
        ));
    }
    if root.starts_with(storage_root) || storage_root.starts_with(&root) {
        return Err(GatewayError::Configuration(
            "skills.root must not overlap the storage root",
        ));
    }
    Ok(Skills { root: Some(root) })
}
