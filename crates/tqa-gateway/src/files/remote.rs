//! A runtime's workspace replies checked in the context of their request (ADR-0030, GW-U-045). The runtime is inside
//! the sandbox and may be model-controlled, so every reply is untrusted: anything outside the request's bounds is
//! refused fail-closed as `502 upstream_invalid`, logged by reason only, never with the runtime's message.
use base64::Engine as _;
use sha2::Digest as _;
use tqa_tools::workspace_tree::manifest::{Manifest, Record, hex};
use tqa_types::{
    ListItem, StatItem, UploadedFile, WorkspaceErrorCode, WorkspaceReply, WorkspaceResult,
};

use crate::GatewayError;

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;

/// The workspace's private directory, never listed at the root.
const PRIVATE: &str = ".tqa";

/// One chunk of a file as the runtime read it.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Chunk {
    pub size: u64,
    pub modified: i64,
    pub inode: u64,
    pub data: Vec<u8>,
    pub eof: bool,
}

fn invalid(reason: &'static str) -> GatewayError {
    tracing::warn!(reason, "a runtime workspace reply was refused");
    GatewayError::UpstreamInvalid
}

/// The gateway's outcome for the runtime's error code.
fn failure(code: WorkspaceErrorCode) -> GatewayError {
    match code {
        // As on Docker, a file where a directory was expected is not found.
        WorkspaceErrorCode::NotFound | WorkspaceErrorCode::NotADirectory => GatewayError::NotFound,
        WorkspaceErrorCode::InvalidPath => GatewayError::PathInvalid,
        WorkspaceErrorCode::TooLarge => GatewayError::BodyTooLarge,
        WorkspaceErrorCode::UploadsFull => GatewayError::Conflict("uploads_full"),
        WorkspaceErrorCode::Busy => GatewayError::Busy,
        WorkspaceErrorCode::Cancelled => GatewayError::Cancelled,
        _ => GatewayError::Io,
    }
}

/// The parts' results in order: numbered from 0, only the final part `last`, and an error only as the final part,
/// which then answers the whole request.
fn results(parts: Vec<WorkspaceReply>) -> Result<Vec<WorkspaceResult>, GatewayError> {
    let count = parts.len();
    if count == 0 {
        return Err(invalid("no reply"));
    }
    let mut results = Vec::with_capacity(count);
    for (index, part) in parts.into_iter().enumerate() {
        let final_part = index + 1 == count;
        if usize::try_from(part.part).ok() != Some(index) || part.last != final_part {
            return Err(invalid("parts out of order"));
        }
        if let WorkspaceResult::Error { code, .. } = part.result {
            return Err(if final_part {
                failure(code)
            } else {
                invalid("an error before the last part")
            });
        }
        results.push(part.result);
    }
    Ok(results)
}

/// A `list` reply: its parts concatenated, at most `limit` items, directories first then names in order, no name
/// twice, no `.tqa` at the root, and `next_after` (on the last part only) naming the last item.
///
/// # Errors
/// `upstream_invalid`, or the runtime's error.
pub(crate) fn listing(
    parts: Vec<WorkspaceReply>,
    limit: usize,
    root: bool,
) -> Result<(Vec<ListItem>, Option<String>), GatewayError> {
    let results = results(parts)?;
    let count = results.len();
    let mut items: Vec<ListItem> = Vec::new();
    let mut next = None;
    for (index, result) in results.into_iter().enumerate() {
        let WorkspaceResult::Listing {
            items: part,
            next_after,
        } = result
        else {
            return Err(invalid("not a listing"));
        };
        if next_after.is_some() && index + 1 != count {
            return Err(invalid("next_after before the last part"));
        }
        next = next_after;
        items.extend(part);
    }
    if items.len() > limit {
        return Err(invalid("more items than asked for"));
    }
    let key = |item: &ListItem| (item.kind != tqa_types::EntryKind::Dir, item.name.clone());
    if items.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        return Err(invalid("items out of order"));
    }
    if root && items.iter().any(|item| item.name == PRIVATE) {
        return Err(invalid("the private directory listed"));
    }
    if next
        .as_ref()
        .is_some_and(|next| items.last().is_none_or(|last| &last.name != next))
    {
        return Err(invalid("next_after is not the last item"));
    }
    Ok((items, next))
}

/// A `read` reply at `offset`: one chunk of at most `max_bytes`, ending within the size it reports, and `eof`
/// exactly when it reaches that size.
///
/// # Errors
/// `upstream_invalid`, or the runtime's error.
pub(crate) fn content(
    parts: Vec<WorkspaceReply>,
    offset: u64,
    max_bytes: usize,
) -> Result<Chunk, GatewayError> {
    let mut results = results(parts)?;
    let (Some(result), true) = (results.pop(), results.is_empty()) else {
        return Err(invalid("a read in more than one part"));
    };
    let WorkspaceResult::Content {
        size,
        modified,
        inode,
        data,
        eof,
    } = result
    else {
        return Err(invalid("not content"));
    };
    let data = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| invalid("content is not base64"))?;
    if data.len() > max_bytes {
        return Err(invalid("more bytes than asked for"));
    }
    let end = u64::try_from(data.len())
        .ok()
        .and_then(|read| offset.checked_add(read));
    if end.is_none_or(|end| end > size || eof != (end == size)) {
        return Err(invalid("a chunk disagrees with the file's size"));
    }
    Ok(Chunk {
        size,
        modified,
        inode,
        data,
        eof,
    })
}

/// A `stat` reply: one item per requested path, in order, each with a kind and size exactly when it exists.
///
/// # Errors
/// `upstream_invalid`, or the runtime's error.
pub(crate) fn stats(
    parts: Vec<WorkspaceReply>,
    count: usize,
) -> Result<Vec<StatItem>, GatewayError> {
    let mut results = results(parts)?;
    let (Some(WorkspaceResult::Stats { items }), true) = (results.pop(), results.is_empty()) else {
        return Err(invalid("not one stats part"));
    };
    if items.len() != count {
        return Err(invalid("stats do not match the paths"));
    }
    let consistent = |item: &StatItem| {
        if item.exists {
            item.kind.is_some() && item.size.is_some()
        } else {
            item.kind.is_none() && item.size.is_none()
        }
    };
    if !items.iter().all(consistent) {
        return Err(invalid("stat presence disagrees"));
    }
    Ok(items)
}

/// An `upload` reply to the chunk at `offset` of `length` bytes: the part file now holds exactly `offset + length`
/// bytes, and only the `last` chunk names the stored file, `uploads/<name>`, as large as everything received.
///
/// # Errors
/// `upstream_invalid`, or the runtime's error.
pub(crate) fn uploaded(
    parts: Vec<WorkspaceReply>,
    offset: u64,
    length: u64,
    last: bool,
) -> Result<Option<UploadedFile>, GatewayError> {
    let mut results = results(parts)?;
    let (Some(WorkspaceResult::Uploaded { received, file }), true) =
        (results.pop(), results.is_empty())
    else {
        return Err(invalid("not one uploaded part"));
    };
    if Some(received) != offset.checked_add(length) || file.is_some() != last {
        return Err(invalid("the upload is not where the chunk left it"));
    }
    if file.as_ref().is_some_and(|file| {
        file.size != received || file.path != format!("{}/{}", super::upload::DIRECTORY, file.name)
    }) {
        return Err(invalid("the stored upload is not the one received"));
    }
    Ok(file)
}

/// A `manifest` reply: its parts' entries in strictly increasing path order, at most `max_files`, a hash only on files
/// within `max_text_bytes` (one within it may lack a hash: it grew while it was read), no empty part among several,
/// and `truncated` only on the last part. The records carry no copies yet.
///
/// # Errors
/// `upstream_invalid`, or the runtime's error.
pub(crate) fn manifest(
    parts: Vec<WorkspaceReply>,
    max_files: usize,
    max_text_bytes: u64,
) -> Result<Manifest, GatewayError> {
    let results = results(parts)?;
    let count = results.len();
    let mut manifest = Manifest::default();
    let mut last_path: Option<String> = None;
    for (index, result) in results.into_iter().enumerate() {
        let WorkspaceResult::ManifestPart { entries, truncated } = result else {
            return Err(invalid("not a manifest"));
        };
        if (entries.is_empty() && count > 1) || (truncated && index + 1 != count) {
            return Err(invalid("a manifest part out of place"));
        }
        manifest.truncated = truncated;
        for entry in entries {
            if last_path.as_ref().is_some_and(|last| *last >= entry.path)
                || manifest.files.len() >= max_files
                || (entry.hash.is_some() && entry.size > max_text_bytes)
            {
                return Err(invalid("a manifest entry out of bounds"));
            }
            last_path = Some(entry.path.clone());
            manifest.files.insert(
                entry.path,
                Record {
                    size: entry.size,
                    stamp: entry.stamp,
                    hash: entry.hash,
                    text: entry.text,
                    copied: false,
                },
            );
        }
    }
    Ok(manifest)
}

/// A `blobs` reply for `requested`: each blob's chunks contiguous from offset 0 until complete, blobs in the requested
/// order (one the runtime could not produce is left out), within `budget` bytes. Only blobs whose SHA-256 is their
/// name are returned; a forged one is dropped and logged (SEC-027). One `not_found` error means no blob at all.
///
/// # Errors
/// `upstream_invalid`, or the runtime's other errors.
pub(crate) fn blobs(
    parts: Vec<WorkspaceReply>,
    requested: &[String],
    budget: u64,
) -> Result<Vec<(String, Vec<u8>)>, GatewayError> {
    let results = match results(parts) {
        Err(GatewayError::NotFound) => return Ok(Vec::new()),
        other => other?,
    };
    let mut kept = Vec::new();
    // The next requested hash a blob may start at, and the blob being assembled.
    let mut next = 0;
    let mut open: Option<(String, Vec<u8>)> = None;
    let mut total: u64 = 0;
    for result in results {
        let WorkspaceResult::BlobPart {
            hash,
            offset,
            data,
            complete,
        } = result
        else {
            return Err(invalid("not a blob"));
        };
        let data = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| invalid("a blob is not base64"))?;
        total = total.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        if total > budget {
            return Err(invalid("more blob bytes than asked for"));
        }
        let (name, mut bytes) = match open.take() {
            Some((name, bytes)) if name == hash => (name, bytes),
            Some(_) => return Err(invalid("a blob left unfinished")),
            None => {
                let at = requested[next..]
                    .iter()
                    .position(|wanted| *wanted == hash)
                    .ok_or_else(|| invalid("a blob not asked for, or out of order"))?;
                next += at + 1;
                (hash, Vec::new())
            }
        };
        if u64::try_from(bytes.len()).ok() != Some(offset) {
            return Err(invalid("a gap or overlap in a blob"));
        }
        bytes.extend_from_slice(&data);
        if !complete {
            open = Some((name, bytes));
        } else if hex(&sha2::Sha256::digest(&bytes)) == name {
            kept.push((name, bytes));
        } else {
            tracing::warn!(
                "a runtime sent a blob whose bytes do not match its hash; it was not stored"
            );
        }
    }
    if open.is_some() {
        return Err(invalid("a blob left unfinished"));
    }
    Ok(kept)
}
