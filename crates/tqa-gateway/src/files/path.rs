//! Workspace-relative paths and no-follow directory walks, shared with the runtime through `tqa-tools`
//! (`workspace_tree`, ADR-0030). This module keeps the gateway's names and maps the shared errors to HTTP outcomes.
pub(crate) use tqa_tools::workspace_tree::path::{Kind, WorkspacePath, hide_private};

use rustix::fs::Stat;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::path::Path;
use tqa_tools::workspace_tree::path as tree;

/// [`tree::open_root`] with the gateway's error.
pub(crate) fn open_root(root: &Path) -> Result<OwnedFd, GatewayError> {
    Ok(tree::open_root(root)?)
}

/// [`tree::open_dir`] with the gateway's error.
pub(crate) fn open_dir(
    root: BorrowedFd<'_>,
    path: &WorkspacePath,
) -> Result<OwnedFd, GatewayError> {
    Ok(tree::open_dir(root, path)?)
}

/// [`tree::open_file`] with the gateway's error.
pub(crate) fn open_file(
    root: BorrowedFd<'_>,
    path: &WorkspacePath,
) -> Result<(OwnedFd, Stat), GatewayError> {
    Ok(tree::open_file(root, path)?)
}

/// [`tree::list`] with the gateway's error.
pub(crate) fn list(directory: BorrowedFd<'_>) -> Result<Vec<tree::Entry>, GatewayError> {
    Ok(tree::list(directory)?)
}

use crate::GatewayError;
use tqa_tools::workspace_tree::TreeError;

impl From<TreeError> for GatewayError {
    /// The same outcomes as before the move: `400 path_invalid`, `404`, or host I/O.
    fn from(error: TreeError) -> Self {
        match error {
            TreeError::PathInvalid => Self::PathInvalid,
            TreeError::NotFound => Self::NotFound,
            TreeError::Io => Self::Io,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TEST: GW-I-013
    #[test]
    fn gw_i_047_tree_errors_keep_their_http_outcomes() {
        for (tree, gateway) in [
            (TreeError::PathInvalid, GatewayError::PathInvalid),
            (TreeError::NotFound, GatewayError::NotFound),
            (TreeError::Io, GatewayError::Io),
        ] {
            assert_eq!(GatewayError::from(tree), gateway);
        }
    }
}
