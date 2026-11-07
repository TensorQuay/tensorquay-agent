//! The file view of a block-image workspace, read through its runtime's workspace service (ADR-0030): the same pages,
//! previews and downloads as the directory on Docker. Every reply is checked by [`super::remote`] in its request's
//! context, and the gateway never sends a request outside the wire bounds, since the runtime refuses such a frame.
use std::sync::Arc;

use base64::Engine as _;
use futures::Stream;
use tqa_types::{ConversationId, EntryKind, ListItem, WORKSPACE_CHUNK_BYTES, WorkspaceOp};

use super::{path::WorkspacePath, remote};
use crate::{GatewayError, app::State};

/// The longest name the runtime accepts after `after`.
const MAX_NAME_BYTES: usize = 255;

/// The directory view's name for an entry kind.
pub(super) const fn kind(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "file",
        EntryKind::Dir => "dir",
        EntryKind::Link => "link",
        _ => "other",
    }
}

/// Whether `name` is one path component the runtime accepts as `after`.
fn name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= MAX_NAME_BYTES
        && !name.contains(['/', '\0'])
}

/// One page of a directory: its items and the name to continue after. An `after` that names no entry gives an empty
/// page, as on Docker.
///
/// # Errors
/// The runtime's outcome, `502 upstream_invalid`, or the link's errors.
pub(super) async fn list(
    state: &State,
    link: &ConversationId,
    target: &WorkspacePath,
    after: Option<String>,
    limit: usize,
) -> Result<(Vec<ListItem>, Option<String>), GatewayError> {
    if after.as_deref().is_some_and(|after| !name(after)) {
        return Ok((Vec::new(), None));
    }
    let op = WorkspaceOp::List {
        path: target.to_string(),
        after,
        limit: u32::try_from(limit).map_err(|_| GatewayError::Io)?,
    };
    let parts = state
        .links
        .workspace(link, op, state.config.io_timeout(), &state.cancel)
        .await?;
    remote::listing(parts, limit, target.is_root())
}

/// One chunk at `offset`.
async fn chunk(
    state: &State,
    link: &ConversationId,
    path: &str,
    offset: u64,
    max_bytes: usize,
) -> Result<remote::Chunk, GatewayError> {
    let op = WorkspaceOp::Read {
        path: path.to_owned(),
        offset,
        max_bytes: u32::try_from(max_bytes).map_err(|_| GatewayError::Io)?,
    };
    let parts = state
        .links
        .workspace(link, op, state.config.io_timeout(), &state.cancel)
        .await?;
    remote::content(parts, offset, max_bytes)
}

/// A file's metadata and its first bytes.
pub(super) struct Head {
    pub size: u64,
    pub modified: i64,
    pub inode: u64,
    pub bytes: Vec<u8>,
    /// The bytes reach the end of the file.
    pub eof: bool,
}

/// Up to `limit` bytes from the start of a regular file, in chunks.
///
/// # Errors
/// As [`list`]; a file that changes between chunks (another size or inode, or it ends early) is
/// `409 file_changed`, retryable: a race with the task writing it, not a protocol violation (review R2).
pub(super) async fn head(
    state: &State,
    link: &ConversationId,
    target: &WorkspacePath,
    limit: u64,
) -> Result<Head, GatewayError> {
    let path = target.to_string();
    let first = chunk(state, link, &path, 0, bounded(limit, 0)).await?;
    let mut head = Head {
        size: first.size,
        modified: first.modified,
        inode: first.inode,
        eof: first.eof,
        bytes: first.data,
    };
    while !head.eof {
        let offset = u64::try_from(head.bytes.len()).map_err(|_| GatewayError::Io)?;
        let want = bounded(limit, offset);
        if want == 0 {
            break;
        }
        let next = chunk(state, link, &path, offset, want).await?;
        if (next.size, next.inode) != (head.size, head.inode) || next.data.is_empty() {
            return Err(GatewayError::FileChanged);
        }
        head.eof = next.eof;
        head.bytes.extend(next.data);
    }
    Ok(head)
}

/// The next read's size: one chunk at most, and never past `limit`.
fn bounded(limit: u64, offset: u64) -> usize {
    usize::try_from(limit.saturating_sub(offset)).map_or(WORKSPACE_CHUNK_BYTES, |left| {
        left.min(WORKSPACE_CHUNK_BYTES)
    })
}

/// A download: the file's size and a stream of its bytes, read chunk by chunk as the client takes them. Refused
/// before any byte is sent when the file is over `limit`.
///
/// # Errors
/// `413 body_too_large`, or as [`head`].
pub(super) async fn download(
    state: Arc<State>,
    link: ConversationId,
    target: &WorkspacePath,
    limit: u64,
) -> Result<
    (
        u64,
        impl Stream<Item = Result<Vec<u8>, GatewayError>> + Send + 'static,
    ),
    GatewayError,
> {
    let path = target.to_string();
    let first = chunk(&state, &link, &path, 0, WORKSPACE_CHUNK_BYTES).await?;
    if first.size > limit {
        return Err(GatewayError::BodyTooLarge);
    }
    let (size, inode) = (first.size, first.inode);
    let start = u64::try_from(first.data.len()).map_err(|_| GatewayError::Io)?;
    let rest = futures::stream::try_unfold(
        (state, link, path, start, first.eof),
        move |(state, link, path, offset, eof)| async move {
            if eof {
                return Ok(None);
            }
            let next = chunk(&state, &link, &path, offset, WORKSPACE_CHUNK_BYTES).await?;
            if (next.size, next.inode) != (size, inode) || next.data.is_empty() {
                return Err(GatewayError::FileChanged);
            }
            let read = u64::try_from(next.data.len()).map_err(|_| GatewayError::Io)?;
            Ok(Some((
                next.data,
                (state, link, path, offset + read, next.eof),
            )))
        },
    );
    let stream =
        futures::StreamExt::chain(futures::stream::once(async move { Ok(first.data) }), rest);
    Ok((size, stream))
}

/// Whether every path is a regular file.
///
/// # Errors
/// As [`list`].
pub(super) async fn all_files(
    state: &State,
    link: &ConversationId,
    paths: &[WorkspacePath],
) -> Result<bool, GatewayError> {
    for batch in paths.chunks(tqa_types::MAX_WORKSPACE_STAT_PATHS) {
        let op = WorkspaceOp::Stat {
            paths: batch.iter().map(ToString::to_string).collect(),
        };
        let parts = state
            .links
            .workspace(link, op, state.config.io_timeout(), &state.cancel)
            .await?;
        let items = remote::stats(parts, batch.len())?;
        if !items
            .iter()
            .all(|item| item.exists && item.kind == Some(EntryKind::File))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Pages of `uploads/` read to count its usage; a directory needing more is refused rather than undercounted.
const USAGE_PAGES: usize = 20;
/// Entries per page when counting.
const USAGE_PAGE: usize = 500;

/// Bytes of regular files in `uploads/` (none when it does not exist yet). Advisory (review note 3): the runtime reports
/// the sizes, and code in the sandbox can change them; the workspace image's size is what really bounds a microVM
/// workspace.
///
/// # Errors
/// `409 uploads_full` past [`USAGE_PAGES`] pages; as [`list`] otherwise.
async fn usage(state: &State, link: &ConversationId) -> Result<u64, GatewayError> {
    let directory = WorkspacePath::parse(super::upload::DIRECTORY)?;
    let mut after = None;
    let mut used: u64 = 0;
    for _ in 0..USAGE_PAGES {
        let (items, next) = match list(state, link, &directory, after, USAGE_PAGE).await {
            Ok(page) => page,
            Err(GatewayError::NotFound) => return Ok(0),
            Err(error) => return Err(error),
        };
        used = items
            .iter()
            .filter(|item| item.kind == EntryKind::File)
            .fold(used, |used, item| used.saturating_add(item.size));
        match next {
            None => return Ok(used),
            some => after = some,
        }
    }
    Err(GatewayError::Conflict("uploads_full"))
}

/// An upload body read ahead of the chunks sent, within the per-file limit and the workspace's upload space.
struct Incoming<S> {
    body: S,
    buffer: Vec<u8>,
    received: u64,
    ended: bool,
    /// The most `received` may reach: the per-file limit, and the space left for uploads.
    per_file: u64,
    space: u64,
}

impl<S, E> Incoming<S>
where
    S: Stream<Item = Result<bytes::Bytes, E>> + Unpin,
{
    /// Reads until a byte past one chunk is held, or the body ends: the next chunk is then known to be last or not.
    async fn fill(&mut self) -> Result<(), GatewayError> {
        while self.buffer.len() <= WORKSPACE_CHUNK_BYTES && !self.ended {
            self.read().await?;
        }
        Ok(())
    }

    /// Whether more of the body may be read ahead while a chunk is in flight: up to one more chunk.
    fn room(&self) -> bool {
        !self.ended && self.buffer.len() <= WORKSPACE_CHUNK_BYTES
    }

    /// Reads one piece of the body, within the limits. A client that leaves shows here as a body error.
    async fn read(&mut self) -> Result<(), GatewayError> {
        use futures::StreamExt as _;
        let Some(bytes) = self.body.next().await else {
            self.ended = true;
            return Ok(());
        };
        let bytes =
            bytes.map_err(|_| GatewayError::BadRequest("the upload body could not be read"))?;
        self.received = self
            .received
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if self.received > self.per_file {
            return Err(GatewayError::UploadTooLarge);
        }
        if self.received > self.space {
            return Err(GatewayError::Conflict("uploads_full"));
        }
        self.buffer.extend_from_slice(&bytes);
        Ok(())
    }

    /// The next chunk to send, and whether it is the last.
    fn chunk(&mut self) -> (Vec<u8>, bool) {
        let data = self
            .buffer
            .drain(..self.buffer.len().min(WORKSPACE_CHUNK_BYTES))
            .collect();
        (data, self.ended && self.buffer.is_empty())
    }
}

/// Stores an upload through the runtime: the body in chunks of at most one wire chunk, in order, the last one marked,
/// within the per-file limit and the workspace's upload space. The runtime names the file as Docker would.
///
/// # Errors
/// `413 upload_too_large`, `409 uploads_full`, `400` for an unreadable body; as [`list`] otherwise.
pub(super) async fn upload<S, E>(
    state: &State,
    link: &ConversationId,
    name: &str,
    body: S,
    limits: super::upload::Limits,
) -> Result<tqa_types::UploadedFile, GatewayError>
where
    S: Stream<Item = Result<bytes::Bytes, E>> + Unpin,
{
    let used = usage(state, link).await?;
    let mut incoming = Incoming {
        body,
        buffer: Vec::new(),
        received: 0,
        ended: false,
        per_file: limits.per_file,
        space: limits.per_workspace.saturating_sub(used),
    };
    if used > limits.per_workspace {
        return Err(GatewayError::Conflict("uploads_full"));
    }
    let upload_id = state.links.workspace.upload_id();
    let mut offset = 0;
    loop {
        incoming.fill().await?;
        let (data, last) = incoming.chunk();
        let length = u64::try_from(data.len()).map_err(|_| GatewayError::Io)?;
        let op = WorkspaceOp::Upload {
            upload_id,
            name: name.to_owned(),
            offset,
            data: base64::engine::general_purpose::STANDARD.encode(&data),
            last,
        };
        // The body is read ahead meanwhile, so a client that leaves ends the upload (and cancels the chunk) at once.
        let request = state
            .links
            .workspace(link, op, state.config.io_timeout(), &state.cancel);
        tokio::pin!(request);
        let parts = loop {
            tokio::select! {
                parts = &mut request => break parts?,
                read = incoming.read(), if incoming.room() => read?,
            }
        };
        if let Some(stored) = remote::uploaded(parts, offset, length, last)? {
            return Ok(stored);
        }
        offset += length;
    }
}
