//! The screenshot upload (ADR-0039 section 9, revision 14): the runtime sends a PNG to the gateway's screenshot store in
//! `screen.upload` chunks keyed by the PNG's SHA-256, and the gateway answers `screen.stored`, `ok` or why not. One
//! upload is in flight per sandbox, and its chunks arrive in order.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most raw bytes one chunk carries (its base64 stays within the link's frame bound).
pub const MAX_UPLOAD_CHUNK_BYTES: usize = 96 * 1024;
/// The most base64 characters one chunk's `data` holds: [`MAX_UPLOAD_CHUNK_BYTES`] encoded.
pub const MAX_UPLOAD_CHUNK_CHARS: usize = MAX_UPLOAD_CHUNK_BYTES / 3 * 4;

/// Why the gateway didn't keep a screenshot; a closed list.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoreRefusal {
    /// Past 512 KiB raw, refused as soon as the running total passes it.
    TooLarge,
    /// The conversation's state quota can't take it.
    QuotaFull,
    /// Not a PNG, or its SHA-256 doesn't match its id.
    BadImage,
    /// Anything else: a chunk out of order, a store failure.
    StoreFailed,
}

/// Whether a chunk's `data` is base64 within the bound: the standard alphabet, padded, at most
/// [`MAX_UPLOAD_CHUNK_CHARS`] characters, and not empty.
pub(crate) fn valid_chunk(data: &str) -> bool {
    !data.is_empty()
        && data.len() <= MAX_UPLOAD_CHUNK_CHARS
        && data.len().is_multiple_of(4)
        && data
            .trim_end_matches('=')
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
        && data.len() - data.trim_end_matches('=').len() <= 2
}
