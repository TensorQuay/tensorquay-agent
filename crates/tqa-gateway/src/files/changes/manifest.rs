//! The run-changes manifest: the walker shared with the runtime (`tqa_tools::workspace_tree::manifest`, ADR-0030),
//! with the gateway's content-addressed blob store receiving the copies of small text files.
use super::blobs::Blobs;
use crate::GatewayError;
use std::os::fd::BorrowedFd;
pub(crate) use tqa_tools::workspace_tree::manifest::{Limits, Manifest, Record};

#[cfg(test)]
#[path = "manifest_tests.rs"]
pub(super) mod tests;

/// Records the workspace below `root`, storing small text files as blobs so a later diff can show them.
///
/// # Errors
/// Host I/O failures, including a blob that cannot be stored.
pub(crate) fn scan(
    root: BorrowedFd<'_>,
    previous: Option<&Manifest>,
    limits: &Limits,
    blobs: &Blobs,
) -> Result<Manifest, GatewayError> {
    tqa_tools::workspace_tree::manifest::scan(root, previous, limits, &mut |bytes: &[u8]| {
        blobs.put(bytes).map(|_| ())
    })
}
