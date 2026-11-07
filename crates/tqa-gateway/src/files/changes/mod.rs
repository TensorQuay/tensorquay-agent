//! Run changes (task 068c, answers-068 Q2): the gateway snapshots a task's workspace when it delivers a user message,
//! before the run can write, and a run's changes are the difference between its snapshot and the next one (or the
//! live workspace for the latest run). Bounded by `[files]`. A block-image workspace is read through its runtime
//! (ADR-0030, `linked`).
pub(crate) mod blobs;
pub(crate) mod compare;
mod endpoints;
mod linked;
mod live;
pub(crate) mod manifest;
mod store;

pub(crate) use endpoints::{diff, run_changes};

use crate::{
    GatewayConfig, GatewayError, files::endpoints::blocking, files::path, manager::Services,
};
use manifest::{Limits, scan};
use std::{os::fd::AsFd, time::Duration};
use store::{Place, Taken};
use tqa_types::{ClientFrame, ConversationId};
use uuid::Uuid;

pub(crate) fn limits(config: &crate::files::FilesConfig) -> Limits {
    Limits {
        files: config.snapshot_files,
        entries: config.snapshot_entries,
        deadline: None,
        text_bytes: config.snapshot_text_bytes,
        copy_bytes: config.snapshot_copy_bytes,
    }
}

/// Called by the sandbox manager just before it hands `frame` to the runtime. A user message's workspace is recorded
/// first; a failed or slow snapshot is logged and never holds the message back.
pub(crate) async fn before_delivery(
    services: &Services,
    config: &GatewayConfig,
    id: &ConversationId,
    frame: &ClientFrame,
) {
    if !matches!(frame, ClientFrame::UserMessage { .. }) {
        return;
    }
    let bound = Duration::from_secs(config.files.snapshot_seconds);
    match tokio::time::timeout(bound, snapshot(services, config, id)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(conversation = %id, %error, "workspace snapshot failed; delivering without it");
        }
        Err(_) => {
            tracing::warn!(conversation = %id, "workspace snapshot exceeded its bound; delivering without it");
        }
    }
}

async fn snapshot(
    services: &Services,
    config: &GatewayConfig,
    id: &ConversationId,
) -> Result<(), GatewayError> {
    let conversation = crate::conversations::uuid(id)?;
    let workspace = crate::workspaces::of_conversation(&services.pool, conversation)
        .await?
        .id;
    let previous = store::latest(&services.pool, workspace).await?;
    let after_seq = store::last_seq(&services.pool, conversation).await?;
    let mut limits = limits(&config.files);
    // The walk stops by itself inside the delivery bound, so a slow workspace is recorded truncated rather than
    // abandoned (the timeout around the snapshot cannot stop the blocking thread).
    let walk = Duration::from_secs(config.files.snapshot_seconds) * 3 / 4;
    limits.deadline = Some(std::time::Instant::now() + walk);
    let storage_root = config.storage_root.clone();
    let (snapshot, manifest) = if config.uses_block_images() {
        // The message is about to be delivered, so the conversation's runtime is live (ADR-0030).
        let read_root = storage_root.clone();
        let previous = blocking(move || {
            Ok(previous.and_then(|previous| Place::new(&read_root, workspace).read(previous).ok()))
        })
        .await?;
        let via = linked::Via {
            links: &services.links,
            conversation: id,
            timeout: walk,
            cancel: &services.cancel,
        };
        let place = Place::new(&storage_root, workspace);
        let manifest = linked::scan(&via, previous.as_ref(), &limits, &place.blobs()).await?;
        let snapshot = Uuid::now_v7();
        let written = manifest.clone();
        blocking(move || Place::new(&storage_root, workspace).write(snapshot, &written)).await?;
        (snapshot, manifest)
    } else {
        let root = services.storage.workspace(workspace).await?;
        blocking(move || {
            let place = Place::new(&storage_root, workspace);
            let previous = previous.and_then(|previous| place.read(previous).ok());
            let root = path::open_root(&root)?;
            let manifest = scan(root.as_fd(), previous.as_ref(), &limits, &place.blobs())?;
            let snapshot = Uuid::now_v7();
            place.write(snapshot, &manifest)?;
            Ok((snapshot, manifest))
        })
        .await?
    };
    let taken = Taken {
        id: snapshot,
        workspace,
        conversation,
        after_seq,
        files: manifest.files.len(),
        truncated: manifest.truncated,
    };
    let kept = store::record(&services.pool, &taken, config.files.snapshots_kept).await?;
    let storage_root = config.storage_root.clone();
    blocking(move || Place::new(&storage_root, workspace).prune(&kept)).await?;
    sweep(services, config).await
}

/// Removes the recorded changes of deleted workspaces.
async fn sweep(services: &Services, config: &GatewayConfig) -> Result<(), GatewayError> {
    let directory = config.storage_root.join("changes");
    let listed = directory.clone();
    let candidates = blocking(move || {
        let Ok(entries) = std::fs::read_dir(&listed) else {
            return Ok(Vec::new());
        };
        Ok(entries
            .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<Uuid>().ok())
            .collect::<Vec<_>>())
    })
    .await?;
    let deleted = store::deleted(&services.pool, &candidates).await?;
    blocking(move || {
        for workspace in deleted {
            std::fs::remove_dir_all(directory.join(workspace.to_string()))?;
        }
        Ok(())
    })
    .await
}
