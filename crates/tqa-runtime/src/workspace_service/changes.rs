//! Run changes (ADR-0030, RT-I-027): the workspace manifest from the walker the gateway uses on Docker, and the
//! bytes of small text files the gateway wants to copy. Only hashes of this runtime's last manifest are served, each
//! file re-read without following links and re-hashed first, so a blob is always what its name says.
use std::{
    collections::HashMap,
    io::Read,
    os::fd::AsFd,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::Engine as _;
use sha2::{Digest, Sha256};
use tqa_tools::workspace_tree::{
    TreeError,
    manifest::{self, Limits, hex},
    path as tree,
};
use tqa_types::{ManifestEntry, WORKSPACE_CHUNK_BYTES, WorkspaceErrorCode, WorkspaceResult};

use super::{cut, error};

/// A walk stops here and reports a truncated manifest (the gateway bounds its whole snapshot too).
const DEADLINE: Duration = Duration::from_mins(2);
/// Bytes read for one blob at most: the largest text bound the gateway may ask for.
const MAX_BLOB_BYTES: u64 = 16 * 1024 * 1024;
/// Bytes one blobs reply carries at most (the gateway asks for far less per request).
const MAX_REPLY_BYTES: u64 = 32 * 1024 * 1024;

/// The last manifest's hashes, each with one path that had it.
#[derive(Default)]
pub(crate) struct Memory(Mutex<HashMap<String, String>>);

/// The manifest within the gateway's bounds, cut into parts; its hashes are remembered for [`blobs`].
pub(super) fn manifest(
    root: &Path,
    bounds: (u32, u32, u32),
    memory: &Memory,
) -> Result<Vec<WorkspaceResult>, TreeError> {
    let (files, entries, text_bytes) = bounds;
    let limits = Limits {
        files: usize::try_from(files).unwrap_or(usize::MAX),
        entries: usize::try_from(entries).unwrap_or(usize::MAX),
        text_bytes: u64::from(text_bytes),
        // The runtime copies nothing: the gateway asks for the bytes it wants.
        copy_bytes: 0,
        deadline: Some(Instant::now() + DEADLINE),
    };
    let root = tree::open_root(root)?;
    let walked = manifest::scan::<TreeError>(root.as_fd(), None, &limits, &mut |_| Ok(()))?;
    let mut hashes = HashMap::new();
    let listed: Vec<ManifestEntry> = walked
        .files
        .into_iter()
        .map(|(path, record)| {
            if let Some(hash) = &record.hash {
                hashes.entry(hash.clone()).or_insert_with(|| path.clone());
            }
            ManifestEntry {
                path,
                size: record.size,
                stamp: record.stamp,
                hash: record.hash,
                text: record.text,
            }
        })
        .collect();
    if let Ok(mut memory) = memory.0.lock() {
        *memory = hashes;
    }
    let pieces = cut(listed);
    let count = pieces.len();
    Ok(pieces
        .into_iter()
        .enumerate()
        .map(|(index, entries)| WorkspaceResult::ManifestPart {
            entries,
            truncated: walked.truncated && index + 1 == count,
        })
        .collect())
}

/// The file at a manifest path, read without following any link, if it still hashes to `hash`.
fn current(root: &Path, path: &str, hash: &str) -> Option<Vec<u8>> {
    let names: Vec<String> = path.split('/').map(str::to_owned).collect();
    let (name, parents) = names.split_last()?;
    let root = tree::open_root(root).ok()?;
    let directory = tree::walk(root.as_fd(), parents, |_| ()).ok()?;
    let file = std::fs::File::from(tree::child_file(directory.as_fd(), name).ok()?);
    let mut bytes = Vec::new();
    file.take(MAX_BLOB_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (hex(&Sha256::digest(&bytes)) == hash).then_some(bytes)
}

/// The requested blobs this runtime can still produce, in order, in chunks; one `not_found` error when none.
pub(super) fn blobs(root: &Path, hashes: &[String], memory: &Memory) -> Vec<WorkspaceResult> {
    let known: HashMap<String, String> = memory
        .0
        .lock()
        .map(|memory| memory.clone())
        .unwrap_or_default();
    let mut parts = Vec::new();
    let mut total: u64 = 0;
    for hash in hashes {
        let Some(bytes) = known.get(hash).and_then(|path| current(root, path, hash)) else {
            continue;
        };
        total = total.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if total > MAX_REPLY_BYTES {
            break;
        }
        let chunks: Vec<&[u8]> = if bytes.is_empty() {
            vec![&[]]
        } else {
            bytes.chunks(WORKSPACE_CHUNK_BYTES).collect()
        };
        let count = chunks.len();
        let mut offset = 0;
        for (index, chunk) in chunks.into_iter().enumerate() {
            parts.push(WorkspaceResult::BlobPart {
                hash: hash.clone(),
                offset,
                data: base64::engine::general_purpose::STANDARD.encode(chunk),
                complete: index + 1 == count,
            });
            offset += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        }
    }
    if parts.is_empty() {
        return vec![error(
            WorkspaceErrorCode::NotFound,
            "none of the blobs is available",
        )];
    }
    parts
}
