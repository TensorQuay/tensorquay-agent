//! Workspace requests over the runtime link (ADR-0030, GW-I-081). The gateway assigns each request an id that is never
//! reused, keeps it pending for its link only, and collects its reply parts. A part is matched only to a pending id of
//! the same conversation and link generation; anything else is dropped and counted, never shown to another request.
//! A request that ends early (timeout, the gateway's cancel, or the HTTP client leaving) sends `workspace.cancel`.
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tqa_types::{ClientFrame, ConversationId, WorkspaceOp, WorkspaceReply};
use uuid::Uuid;

use super::registry::{Delivery, LinkRegistry};
use crate::GatewayError;

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;

/// Parts a listing may take: 500 names of 255 bytes, each escaped to six bytes a byte (about 790 KiB), cut at
/// `WORKSPACE_PART_BYTES` (192 KiB), is 5 parts; more is a runaway runtime.
const LISTING_PARTS: usize = 8;

/// Hashes asked for in one blobs request at most.
pub(crate) const BLOB_BATCH_HASHES: usize = 64;
/// Bytes asked for in one blobs request at most (the sizes come from the manifest).
pub(crate) const BLOB_BATCH_BYTES: u64 = 4 * 1024 * 1024;
/// Parts a blobs reply may take beyond one per hash: 4 MiB in 128 KiB chunks is 32 more.
const BLOB_EXTRA_PARTS: usize = 32;

/// A manifest entry on the wire at most: a 4096-byte path, a hash, and the other fields.
const MAX_ENTRY_BYTES: usize = 4096 + 64 + 128;
/// What a reply's envelope and each item cost beyond their strings, in the estimate of [`weight`].
const OVERHEAD: usize = 64;

/// How much of a reply the gateway holds before checking it: parts, and estimated bytes. Every part is buffered
/// before its reply is checked, so both bound what a runtime can make the gateway hold (reviews R1 of slices 2 and
/// 4/5): about what a legitimate reply to the request needs, never the part bound times a full frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Bound {
    pub parts: usize,
    pub bytes: usize,
}

pub(super) fn bound(op: &WorkspaceOp) -> Bound {
    let frame = tqa_types::MAX_FRAME_BYTES;
    match op {
        WorkspaceOp::List { .. } => Bound {
            parts: LISTING_PARTS,
            bytes: LISTING_PARTS * tqa_types::WORKSPACE_PART_BYTES,
        },
        // Every part carries at least one entry (only a sole part may be empty).
        WorkspaceOp::Manifest { max_files, .. } => {
            let files = usize::try_from(*max_files).unwrap_or(usize::MAX);
            Bound {
                parts: files.saturating_add(1),
                bytes: files.saturating_mul(MAX_ENTRY_BYTES).max(frame),
            }
        }
        // Base64 of one batch, plus each part's envelope.
        WorkspaceOp::Blobs { hashes } => {
            let parts = hashes.len() + BLOB_EXTRA_PARTS;
            let data = usize::try_from(BLOB_BATCH_BYTES)
                .unwrap_or(usize::MAX)
                .div_ceil(3)
                * 4;
            Bound {
                parts,
                bytes: data + parts * (OVERHEAD + 64),
            }
        }
        // A chunk, a stat, an upload step: one part each.
        _ => Bound {
            parts: 1,
            bytes: frame,
        },
    }
}

/// A reply part's size on the wire, estimated from its strings and a fixed cost per item.
fn weight(reply: &WorkspaceReply) -> usize {
    use tqa_types::WorkspaceResult;
    OVERHEAD
        + match &reply.result {
            WorkspaceResult::Listing { items, .. } => {
                items.iter().map(|item| OVERHEAD + item.name.len()).sum()
            }
            WorkspaceResult::Content { data, .. } | WorkspaceResult::BlobPart { data, .. } => {
                data.len() + 64
            }
            WorkspaceResult::Uploaded { file, .. } => file
                .as_ref()
                .map_or(0, |file| file.path.len() + file.name.len()),
            WorkspaceResult::Stats { items } => items.len() * OVERHEAD,
            WorkspaceResult::ManifestPart { entries, .. } => entries
                .iter()
                .map(|entry| 128 + entry.path.len() + entry.hash.as_ref().map_or(0, String::len))
                .sum(),
            WorkspaceResult::Error { message, .. } => message.len(),
            _ => tqa_types::MAX_FRAME_BYTES,
        }
}
/// Parts queued for a request before its reader takes them; a runtime that floods past it loses the request.
const PART_QUEUE: usize = 16;

struct Pending {
    conversation: ConversationId,
    generation: Uuid,
    parts: mpsc::Sender<WorkspaceReply>,
}

/// The pending workspace requests of every link.
pub(crate) struct WorkspaceRequests {
    /// The next id: one counter for the gateway, so no id repeats on any link.
    next: AtomicU64,
    pending: Mutex<HashMap<u64, Pending>>,
    /// Pending requests allowed per link (`[files] link_requests`).
    per_link: usize,
    /// Replies dropped: unknown or finished ids, another link's ids, or a flooded request.
    dropped: AtomicU64,
    /// The next upload id: one counter for the gateway, so no upload id repeats on any link.
    uploads: AtomicU64,
}

impl WorkspaceRequests {
    pub(crate) fn new(per_link: usize) -> Self {
        Self {
            next: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            per_link,
            dropped: AtomicU64::new(0),
            uploads: AtomicU64::new(1),
        }
    }

    /// A new upload's id (ADR-0030): never reused on any link.
    pub(crate) fn upload_id(&self) -> u64 {
        self.uploads.fetch_add(1, Ordering::Relaxed)
    }

    /// Replies dropped since start.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn open(
        &self,
        conversation: &ConversationId,
        generation: Uuid,
    ) -> Result<(u64, mpsc::Receiver<WorkspaceReply>), GatewayError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let open = pending
            .values()
            .filter(|request| {
                &request.conversation == conversation && request.generation == generation
            })
            .count();
        if open >= self.per_link {
            return Err(GatewayError::Busy);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (parts, receiver) = mpsc::channel(PART_QUEUE);
        pending.insert(
            id,
            Pending {
                conversation: conversation.clone(),
                generation,
                parts,
            },
        );
        Ok((id, receiver))
    }

    fn close(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }

    /// Hands a reply part to its request when it is pending on this very link; drops and counts it otherwise.
    pub(crate) fn route(
        &self,
        conversation: &ConversationId,
        generation: Uuid,
        reply: WorkspaceReply,
    ) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        let request_id = reply.request_id;
        let delivered = match pending.get(&request_id) {
            Some(request)
                if &request.conversation == conversation && request.generation == generation =>
            {
                // A runtime that floods the request loses it: it ends as an invalid reply.
                let sent = request.parts.try_send(reply).is_ok();
                if !sent {
                    pending.remove(&request_id);
                }
                sent
            }
            _ => false,
        };
        drop(pending);
        if !delivered {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(request_id, "a workspace reply was dropped");
        }
    }
}

/// A request's parts up to the one marked last, within `bound`: refused as soon as a part passes it.
async fn collect(
    parts: &mut mpsc::Receiver<WorkspaceReply>,
    bound: Bound,
) -> Result<Vec<WorkspaceReply>, GatewayError> {
    let mut replies = Vec::new();
    let mut bytes: usize = 0;
    while replies.len() < bound.parts {
        let reply = parts.recv().await.ok_or(GatewayError::UpstreamInvalid)?;
        bytes = bytes.saturating_add(weight(&reply));
        if bytes > bound.bytes {
            return Err(GatewayError::UpstreamInvalid);
        }
        let last = reply.last;
        replies.push(reply);
        if last {
            return Ok(replies);
        }
    }
    Err(GatewayError::UpstreamInvalid)
}

/// Ends a request: it stops being pending, and the runtime is told to stop unless the reply was complete.
struct Open<'a> {
    links: &'a LinkRegistry,
    conversation: &'a ConversationId,
    id: u64,
    complete: bool,
}

impl Drop for Open<'_> {
    fn drop(&mut self) {
        self.links.workspace.close(self.id);
        if !self.complete
            && let Ok(permit) = self.links.reserve(self.conversation)
        {
            let (sent, _) = tokio::sync::oneshot::channel();
            permit.send(Delivery {
                frame: ClientFrame::WorkspaceCancel {
                    request_id: self.id,
                },
                sent,
            });
        }
    }
}

impl LinkRegistry {
    /// Sends one workspace request to the conversation's runtime and returns its reply parts, the last one ending
    /// them. Their content is not checked here (`files::remote` does that in the request's context).
    ///
    /// # Errors
    /// `409 sandbox_not_ready` without a live link or when it drops; `429 busy` past `link_requests`; `504` after
    /// `timeout`; `503 restarting` on the gateway's cancel; `502 upstream_invalid` for a runaway or flooding runtime.
    pub(crate) async fn workspace(
        &self,
        conversation: &ConversationId,
        op: WorkspaceOp,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Vec<WorkspaceReply>, GatewayError> {
        let (generation, link) = self.link(conversation)?;
        let (id, mut parts) = self.workspace.open(conversation, generation)?;
        let bound = bound(&op);
        let mut open = Open {
            links: self,
            conversation,
            id,
            complete: false,
        };
        let exchange = async {
            self.deliver(
                conversation,
                ClientFrame::WorkspaceRequest { request_id: id, op },
            )
            .await?;
            collect(&mut parts, bound).await
        };
        let result = tokio::select! {
            result = tokio::time::timeout(timeout, exchange) => result.unwrap_or(Err(GatewayError::Timeout)),
            () = link.cancelled() => Err(GatewayError::Conflict("sandbox_not_ready")),
            () = cancel.cancelled() => Err(GatewayError::Cancelled),
        };
        open.complete = result.is_ok();
        result
    }
}
