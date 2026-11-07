//! Snapshots of a block-image workspace (ADR-0030, GW-I-084): the runtime walks it with the shared walker and sends
//! the bytes of the text files the gateway wants to copy. The gateway keeps copies it already has, asks for the rest
//! within the copy budget in bounded batches, and stores only bytes whose SHA-256 is their name (SEC-027); the
//! manifest then goes where a Docker snapshot's goes.
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tqa_types::{ConversationId, WorkspaceOp};

use super::{
    blobs::Blobs,
    manifest::{Limits, Manifest},
};
use crate::{
    GatewayError,
    files::remote,
    link::{BLOB_BATCH_BYTES, BLOB_BATCH_HASHES, LinkRegistry},
};

#[cfg(test)]
#[path = "linked_tests.rs"]
mod tests;

/// The runtime link a snapshot is read through, and its bounds.
pub(crate) struct Via<'a> {
    pub links: &'a LinkRegistry,
    pub conversation: &'a ConversationId,
    pub timeout: Duration,
    pub cancel: &'a CancellationToken,
}

impl Via<'_> {
    async fn ask(&self, op: WorkspaceOp) -> Result<Vec<tqa_types::WorkspaceReply>, GatewayError> {
        self.links
            .workspace(self.conversation, op, self.timeout, self.cancel)
            .await
    }
}

/// The workspace's manifest through its runtime, with copies of its small text files as far as `limits` allow. An
/// unchanged file keeps the previous snapshot's copy.
///
/// # Errors
/// The link's and the runtime's errors, `502 upstream_invalid`, blob storage failures.
pub(crate) async fn scan(
    via: &Via<'_>,
    previous: Option<&Manifest>,
    limits: &Limits,
    blobs: &Blobs,
) -> Result<Manifest, GatewayError> {
    let op = WorkspaceOp::Manifest {
        max_files: u32::try_from(limits.files).map_err(|_| GatewayError::Io)?,
        max_entries: u32::try_from(limits.entries).map_err(|_| GatewayError::Io)?,
        max_text_bytes: u32::try_from(limits.text_bytes).map_err(|_| GatewayError::Io)?,
    };
    let mut manifest = remote::manifest(via.ask(op).await?, limits.files, limits.text_bytes)?;
    let wanted = copies(&mut manifest, previous, limits.copy_bytes, blobs);
    for batch in batches(&wanted) {
        let op = WorkspaceOp::Blobs {
            hashes: batch.iter().map(|(hash, _)| hash.clone()).collect(),
        };
        let budget = batch.iter().map(|(_, size)| size).sum();
        let requested: Vec<String> = batch.iter().map(|(hash, _)| hash.clone()).collect();
        let verified = remote::blobs(via.ask(op).await?, &requested, budget)?;
        keep(&mut manifest, blobs, verified)?;
    }
    Ok(manifest)
}

/// Stores verified copies and marks every record with their hashes as copied.
fn keep(
    manifest: &mut Manifest,
    blobs: &Blobs,
    verified: Vec<(String, Vec<u8>)>,
) -> Result<(), GatewayError> {
    for (hash, bytes) in verified {
        blobs.put(&bytes)?;
        manifest
            .files
            .values_mut()
            .filter(|record| record.hash.as_deref() == Some(hash.as_str()))
            .for_each(|record| record.copied = true);
    }
    Ok(())
}

/// Marks what the snapshot already has a copy of, and returns the text files still to copy (hash and size, in path
/// order, each hash once) within the copy budget, which counts every copy as the walker does; one larger than a batch
/// is left without a copy.
fn copies(
    manifest: &mut Manifest,
    previous: Option<&Manifest>,
    budget: u64,
    blobs: &Blobs,
) -> Vec<(String, u64)> {
    let mut spent: u64 = 0;
    let mut wanted: Vec<(String, u64)> = Vec::new();
    for (path, record) in &mut manifest.files {
        if let Some(earlier) = previous.and_then(|previous| previous.files.get(path))
            && (earlier.size, earlier.stamp, &earlier.hash)
                == (record.size, record.stamp, &record.hash)
        {
            record.copied = earlier.copied;
            continue;
        }
        let Some(hash) = record.hash.clone().filter(|_| record.text) else {
            continue;
        };
        // A copy larger than one batch is not asked for (a reply's parts are bounded per batch): recorded without a
        // copy, like a file past the budget.
        if record.size > BLOB_BATCH_BYTES || spent.saturating_add(record.size) > budget {
            continue;
        }
        spent += record.size;
        if blobs.contains(&hash) {
            record.copied = true;
        } else if !wanted.iter().any(|(known, _)| *known == hash) {
            wanted.push((hash, record.size));
        }
    }
    wanted
}

/// The wanted copies in requests of at most [`BLOB_BATCH_HASHES`] hashes and [`BLOB_BATCH_BYTES`] bytes.
fn batches(wanted: &[(String, u64)]) -> Vec<&[(String, u64)]> {
    let mut batches = Vec::new();
    let (mut start, mut bytes) = (0, 0_u64);
    for (index, (_, size)) in wanted.iter().enumerate() {
        let full =
            index - start == BLOB_BATCH_HASHES || bytes.saturating_add(*size) > BLOB_BATCH_BYTES;
        if full && index > start {
            batches.push(&wanted[start..index]);
            (start, bytes) = (index, 0);
        }
        bytes = bytes.saturating_add(*size);
    }
    if start < wanted.len() {
        batches.push(&wanted[start..]);
    }
    batches
}
