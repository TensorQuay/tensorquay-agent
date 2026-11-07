//! The workspace service on the runtime link (revision 7, ADR-0030): the gateway asks the runtime that mounts a
//! workspace to list, read, upload, stat, snapshot and copy it. A request is correlated by a per-link `request_id` the
//! gateway never reuses; a reply may come in parts and is never stored or forwarded to clients. Both sides are strict.
//! Checks that need the request (items within its limit, an upload's offset) belong to the gateway, not to these types.

use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{
    ErrorCode, FrameError, MAX_WORKSPACE_BLOB_HASHES, MAX_WORKSPACE_STAT_PATHS,
    WORKSPACE_CHUNK_BYTES,
};

/// Largest request path, in bytes: the workspace tree's bound, as the files API parses it.
const MAX_REQUEST_PATH_BYTES: usize = 1024;
/// Largest reply path, in bytes: tools can create deeper paths than the files API accepts.
const MAX_REPLY_PATH_BYTES: usize = 4096;
/// Largest directory entry name, in bytes.
const MAX_NAME_BYTES: usize = 255;
/// Largest sanitised upload name, in bytes.
const MAX_UPLOAD_NAME_BYTES: usize = 128;
/// Largest error message, in bytes.
const MAX_MESSAGE_BYTES: usize = 512;
/// Base64 characters of one full chunk.
const MAX_CHUNK_BASE64: usize = WORKSPACE_CHUNK_BYTES.div_ceil(3) * 4;

/// One workspace operation the gateway asks of the runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum WorkspaceOp {
    /// One page of a directory: directories first, then names in order.
    List {
        /// A canonical workspace-relative directory; `""` is the root.
        #[serde(deserialize_with = "request_path")]
        #[schemars(length(max = 1024))]
        path: String,
        /// The last name of the previous page.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional_name"
        )]
        #[schemars(length(min = 1, max = 255))]
        after: Option<String>,
        /// 1 through 500 entries.
        #[schemars(range(min = 1, max = 500))]
        #[serde(deserialize_with = "list_limit")]
        limit: u32,
    },
    /// One chunk of a regular file.
    Read {
        /// A canonical workspace-relative file.
        #[serde(deserialize_with = "request_path")]
        #[schemars(length(max = 1024))]
        path: String,
        /// The first byte to read.
        offset: u64,
        /// 1 through [`WORKSPACE_CHUNK_BYTES`] bytes.
        #[serde(deserialize_with = "chunk_bytes")]
        #[schemars(range(min = 1, max = 131_072))]
        max_bytes: u32,
    },
    /// One chunk of an upload into `uploads/`.
    Upload {
        /// The upload, never reused on a link.
        upload_id: u64,
        /// The gateway's sanitised name: one component, not hidden, within 128 bytes.
        #[serde(deserialize_with = "upload_name")]
        #[schemars(length(min = 1, max = 128))]
        name: String,
        /// Where this chunk starts.
        offset: u64,
        /// Standard padded base64 of at most one chunk; empty only when it ends the upload.
        #[serde(deserialize_with = "data")]
        #[schemars(schema_with = "base64_schema")]
        data: String,
        /// Whether this chunk ends the upload.
        last: bool,
    },
    /// Whether each path exists, and its kind and size.
    Stat {
        /// 1 through [`MAX_WORKSPACE_STAT_PATHS`] canonical paths.
        #[serde(deserialize_with = "stat_paths")]
        #[schemars(length(min = 1, max = 64))]
        paths: Vec<String>,
    },
    /// The workspace's snapshot records, in parts.
    Manifest {
        /// 1 through 100 000 files.
        #[schemars(range(min = 1, max = 100_000))]
        max_files: u32,
        /// 100 through 1 000 000 walked entries.
        #[schemars(range(min = 100, max = 1_000_000))]
        max_entries: u32,
        /// 1024 through 16 MiB: files this small are hashed and may be copied.
        #[schemars(range(min = 1024, max = 16_777_216))]
        max_text_bytes: u32,
    },
    /// The content of files by their SHA-256, in chunks.
    Blobs {
        /// 1 through [`MAX_WORKSPACE_BLOB_HASHES`] distinct lowercase hex hashes.
        #[serde(deserialize_with = "blob_hashes")]
        #[schemars(schema_with = "hashes_schema")]
        hashes: Vec<String>,
    },
}

/// One part of the runtime's reply to a workspace request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReply {
    /// The request this part answers.
    pub request_id: u64,
    /// The part's index, from 0.
    #[schemars(range(max = 4_294_967_295_u32))]
    pub part: u32,
    /// Whether this part ends the reply.
    pub last: bool,
    /// The part's content.
    pub result: WorkspaceResult,
}

/// The content of one reply part.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum WorkspaceResult {
    /// Directory entries; a page may span parts.
    Listing {
        /// The part's entries.
        items: Vec<ListItem>,
        /// The name to continue after, on the last part only.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional_name"
        )]
        #[schemars(length(min = 1, max = 255))]
        next_after: Option<String>,
    },
    /// One chunk of a file; the gateway derives the etag and whether it is text.
    Content {
        /// The file's size.
        size: u64,
        /// Modification time, in Unix seconds.
        modified: i64,
        /// The file's inode.
        inode: u64,
        /// Standard padded base64 of at most one chunk.
        #[serde(deserialize_with = "data")]
        #[schemars(schema_with = "base64_schema")]
        data: String,
        /// Whether the chunk reaches the end of the file.
        eof: bool,
    },
    /// An upload chunk was written.
    Uploaded {
        /// The part file's size after this chunk.
        received: u64,
        /// The stored file, when the chunk ended the upload.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<UploadedFile>,
    },
    /// One item per requested path, in order.
    Stats {
        /// At most [`MAX_WORKSPACE_STAT_PATHS`] items; no path is echoed back.
        #[schemars(length(max = 64))]
        #[serde(deserialize_with = "stat_items")]
        items: Vec<StatItem>,
    },
    /// Snapshot records; the manifest may span parts.
    ManifestPart {
        /// The part's records.
        entries: Vec<ManifestEntry>,
        /// Whether a bound stopped the walk; on the last part only.
        truncated: bool,
    },
    /// One chunk of one requested blob.
    BlobPart {
        /// The blob's SHA-256, lowercase hex.
        #[serde(deserialize_with = "hash")]
        #[schemars(schema_with = "hash_schema")]
        hash: String,
        /// Where this chunk starts in the blob.
        offset: u64,
        /// Standard padded base64 of at most one chunk.
        #[serde(deserialize_with = "data")]
        #[schemars(schema_with = "base64_schema")]
        data: String,
        /// Whether this chunk ends the blob.
        complete: bool,
    },
    /// The request failed; this part ends it.
    Error {
        /// A stable code the gateway maps to its HTTP errors.
        code: WorkspaceErrorCode,
        /// A bounded diagnostic for logs, never shown to clients.
        #[serde(deserialize_with = "message")]
        #[schemars(length(max = 512))]
        message: String,
        /// Whether a retry may succeed; only for `busy`.
        retryable: bool,
    },
}

/// A directory entry's kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link, never followed.
    Link,
    /// Anything else.
    Other,
}

/// One directory entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListItem {
    /// One path component within 255 bytes.
    #[serde(deserialize_with = "entry_name")]
    #[schemars(length(min = 1, max = 255))]
    pub name: String,
    /// The entry's kind.
    pub kind: EntryKind,
    /// Its size in bytes.
    pub size: u64,
    /// Modification time, in Unix seconds.
    pub modified: i64,
}

/// A stored upload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UploadedFile {
    /// `uploads/<name>`.
    #[schemars(length(max = 136))]
    pub path: String,
    /// The final free name within 128 bytes.
    #[serde(deserialize_with = "upload_name")]
    #[schemars(length(min = 1, max = 128))]
    pub name: String,
    /// The stored size, equal to the bytes received.
    pub size: u64,
}

/// One stat answer: the kind and size are present exactly when the path exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatItem {
    /// Whether the path exists.
    pub exists: bool,
    /// The entry's kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<EntryKind>,
    /// The entry's size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// One snapshot record: the workspace tree's record without the gateway's copy state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    /// A canonical workspace-relative path within 4096 bytes, outside `.tqa`.
    #[serde(deserialize_with = "manifest_path")]
    #[schemars(length(min = 1, max = 4096))]
    pub path: String,
    /// The file's size.
    pub size: u64,
    /// The change stamp: modification and status-change seconds and nanoseconds, and the inode.
    pub stamp: (i64, i64, i64, i64, u64),
    /// The SHA-256 of a small file, lowercase hex.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_hash"
    )]
    #[schemars(schema_with = "hash_schema")]
    pub hash: Option<String>,
    /// Whether the file is text.
    pub text: bool,
}

/// Why a workspace request failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WorkspaceErrorCode {
    /// The path does not exist.
    NotFound,
    /// A directory was expected.
    NotADirectory,
    /// A size or count bound was passed.
    TooLarge,
    /// The path is not a canonical workspace path, or leaves the workspace.
    InvalidPath,
    /// The uploads space is full.
    UploadsFull,
    /// The runtime is busy; retry.
    Busy,
    /// The gateway cancelled the request.
    Cancelled,
    /// Anything else.
    Internal,
}

impl WorkspaceOp {
    /// Checks an empty upload chunk only ends an upload, and the manifest bounds.
    pub(crate) fn check(&self) -> Result<(), FrameError> {
        match self {
            Self::Upload { data, last, .. } if data.is_empty() && !last => Err(invalid(
                "$.op.data",
                "an empty upload chunk must end the upload",
            )),
            Self::Manifest {
                max_files,
                max_entries,
                max_text_bytes,
            } if !(1..=100_000).contains(max_files)
                || !(100..=1_000_000).contains(max_entries)
                || !(1024..=16_777_216).contains(max_text_bytes) =>
            {
                Err(invalid(
                    "$.op",
                    "expected 1 through 100000 files, 100 through 1000000 entries and 1024 through 16777216 text bytes",
                ))
            }
            _ => Ok(()),
        }
    }
}

impl WorkspaceReply {
    /// Checks what one field alone cannot, within this part.
    pub(crate) fn check(&self) -> Result<(), FrameError> {
        match &self.result {
            WorkspaceResult::Error {
                code, retryable, ..
            } => {
                if !self.last {
                    return Err(invalid("$.last", "an error ends the request"));
                }
                if *retryable && *code != WorkspaceErrorCode::Busy {
                    return Err(invalid("$.result.retryable", "only busy is retryable"));
                }
            }
            WorkspaceResult::ManifestPart {
                truncated: true, ..
            } if !self.last => {
                return Err(invalid(
                    "$.result.truncated",
                    "only the last part is truncated",
                ));
            }
            WorkspaceResult::Listing {
                next_after: Some(_),
                ..
            } if !self.last => {
                return Err(invalid(
                    "$.result.next_after",
                    "only the last part names the next page",
                ));
            }
            WorkspaceResult::Uploaded {
                received,
                file: Some(file),
            } if file.size != *received || file.path != format!("uploads/{}", file.name) => {
                return Err(invalid(
                    "$.result.file",
                    "an uploaded file is uploads/<name> with the received size",
                ));
            }
            WorkspaceResult::Stats { items }
                if items.iter().any(|item| {
                    item.kind.is_some() != item.exists || item.size.is_some() != item.exists
                }) =>
            {
                return Err(invalid(
                    "$.result.items",
                    "a kind and size are present exactly when the path exists",
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

fn invalid(path: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: path.into(),
        detail: detail.into(),
    }
}

/// One path component: nonempty, not `.` or `..`, without `/` or NUL.
fn component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\0'])
}
/// A canonical relative path: components joined by single slashes, with no leading or trailing one.
fn canonical(path: &str) -> bool {
    path.split('/').all(component)
}

fn request_path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let path = String::deserialize(deserializer)?;
    check_request_path(&path).map_err(D::Error::custom)?;
    Ok(path)
}
fn check_request_path(path: &str) -> Result<(), &'static str> {
    if path.len() > MAX_REQUEST_PATH_BYTES || !(path.is_empty() || canonical(path)) {
        return Err(
            "expected a canonical workspace-relative path within 1024 bytes; the root is \"\"",
        );
    }
    Ok(())
}
fn stat_paths<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let paths = Vec::<String>::deserialize(deserializer)?;
    if !(1..=MAX_WORKSPACE_STAT_PATHS).contains(&paths.len()) {
        return Err(D::Error::custom("expected 1 through 64 paths"));
    }
    for path in &paths {
        check_request_path(path).map_err(D::Error::custom)?;
    }
    Ok(paths)
}
fn manifest_path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let path = String::deserialize(deserializer)?;
    if path.len() > MAX_REPLY_PATH_BYTES
        || !canonical(&path)
        || path == ".tqa"
        || path.starts_with(".tqa/")
    {
        return Err(D::Error::custom(
            "expected a canonical workspace-relative path within 4096 bytes, outside .tqa",
        ));
    }
    Ok(path)
}
fn entry_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if name.len() > MAX_NAME_BYTES || !component(&name) {
        return Err(D::Error::custom(
            "expected one path component within 255 bytes",
        ));
    }
    Ok(name)
}
fn optional_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    entry_name(deserializer).map(Some)
}
fn upload_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if name.len() > MAX_UPLOAD_NAME_BYTES || !component(&name) || name.starts_with('.') {
        return Err(D::Error::custom(
            "expected one visible path component within 128 bytes",
        ));
    }
    Ok(name)
}
fn list_limit<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let limit = u32::deserialize(deserializer)?;
    if !(1..=500).contains(&limit) {
        return Err(D::Error::custom("expected a limit of 1 through 500"));
    }
    Ok(limit)
}
fn chunk_bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let bytes = u32::deserialize(deserializer)?;
    if bytes == 0 || bytes as usize > WORKSPACE_CHUNK_BYTES {
        return Err(D::Error::custom("expected 1 through 131072 bytes"));
    }
    Ok(bytes)
}
fn message<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let message = String::deserialize(deserializer)?;
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(D::Error::custom("expected a message within 512 bytes"));
    }
    Ok(message)
}
fn stat_items<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<StatItem>, D::Error> {
    let items = Vec::<StatItem>::deserialize(deserializer)?;
    if items.len() > MAX_WORKSPACE_STAT_PATHS {
        return Err(D::Error::custom("expected at most 64 items"));
    }
    Ok(items)
}

/// The decoded length of standard padded base64, or `None` for anything else.
fn base64_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let padding = bytes.iter().rev().take_while(|byte| **byte == b'=').count();
    let body = &bytes[..bytes.len() - padding];
    if padding > 2
        || !body
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
    {
        return None;
    }
    Some(bytes.len() / 4 * 3 - padding)
}
fn data<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if base64_len(&text).is_none_or(|length| length > WORKSPACE_CHUNK_BYTES) {
        return Err(D::Error::custom(
            "expected standard padded base64 of at most 131072 bytes",
        ));
    }
    Ok(text)
}
fn base64_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string","pattern":"^[A-Za-z0-9+/]*={0,2}$","maxLength":MAX_CHUNK_BASE64})
}

fn is_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn hash<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if !is_hash(&text) {
        return Err(D::Error::custom(
            "expected a SHA-256 in 64 lowercase hex characters",
        ));
    }
    Ok(text)
}
fn optional_hash<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    hash(deserializer).map(Some)
}
fn blob_hashes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let hashes = Vec::<String>::deserialize(deserializer)?;
    if !(1..=MAX_WORKSPACE_BLOB_HASHES).contains(&hashes.len())
        || !hashes.iter().all(|hash| is_hash(hash))
        || hashes
            .iter()
            .enumerate()
            .any(|(index, hash)| hashes[..index].contains(hash))
    {
        return Err(D::Error::custom(
            "expected 1 through 256 distinct SHA-256 hashes in lowercase hex",
        ));
    }
    Ok(hashes)
}
fn hash_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string","pattern":"^[0-9a-f]{64}$"})
}
fn hashes_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"array","minItems":1,"maxItems":MAX_WORKSPACE_BLOB_HASHES,
        "items":{"type":"string","pattern":"^[0-9a-f]{64}$"}})
}
