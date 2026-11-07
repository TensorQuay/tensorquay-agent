//! HTTP handlers for a run's changes (GW-I-048): the changed files with line counts, and one file's unified diff.
//! Access is the files view's: workspace read access. On Firecracker the live side is read through the runtime.
use super::compare::{self, FileChange, FileDiff};
use super::manifest::{Manifest, scan};
use super::store::{self, Bounds, Place};
use crate::{
    GatewayError,
    app::State,
    auth::{Authz, User},
    files::{
        endpoints::{access, blocking},
        path::{self, WorkspacePath},
    },
};
use axum::{
    Json,
    extract::{Path as UrlPath, Query, State as AppState},
};
use serde::{Deserialize, Serialize};
use std::{os::fd::AsFd, sync::Arc};
use tqa_types::ConversationId;
use uuid::Uuid;

#[derive(Serialize)]
pub(crate) struct RunChanges {
    run_id: String,
    /// Compared with the live workspace: the latest run (or one still running) has no closing snapshot yet.
    live: bool,
    running: bool,
    /// A snapshot stopped at a bound; files beyond it are not compared.
    truncated: bool,
    files: Vec<FileChange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiffQuery {
    path: String,
}

/// The two manifests around a run, the second read from the live workspace when no later snapshot exists.
struct Compared {
    before: Manifest,
    after: Arc<Manifest>,
    place: Place,
    bounds: Bounds,
}

async fn compared(
    state: &State,
    user: &User,
    id: &ConversationId,
    run: &str,
) -> Result<Compared, GatewayError> {
    if run.is_empty()
        || run.len() > 64
        || !run
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(GatewayError::NotFound);
    }
    let workspace = access(state, user.id, id, false).await?;
    let conversation = crate::conversations::uuid(id)?;
    let bounds = crate::io::bounded(
        state.config.io_timeout(),
        &state.cancel,
        store::bounds(&state.pool, workspace, conversation, run),
    )
    .await?;
    let storage_root = state.config.storage_root.clone();
    let (first, last) = (bounds.before, bounds.after);
    let (before, after) = blocking(move || {
        let place = Place::new(&storage_root, workspace);
        Ok((
            place.read(first)?,
            last.map(|last| place.read(last)).transpose()?,
        ))
    })
    .await?;
    let after = match after {
        Some(after) => Arc::new(after),
        None => live_manifest(state, user, (id, workspace), &before).await?,
    };
    Ok(Compared {
        before,
        after,
        place: Place::new(&state.config.storage_root, workspace),
        bounds,
    })
}

/// The workspace's live manifest compared with `before`: one live scan per workspace at a time, bounded like any
/// request (review of 068c). On Firecracker it is read through the runtime of a sandbox on the workspace, the owner's
/// started if none is (ADR-0030 D1).
async fn live_manifest(
    state: &State,
    user: &User,
    (id, workspace): (&ConversationId, Uuid),
    before: &Manifest,
) -> Result<Arc<Manifest>, GatewayError> {
    let mut limits = super::limits(&state.config.files);
    limits.deadline = Some(std::time::Instant::now() + state.config.io_timeout());
    let previous = before.clone();
    if state.config.uses_block_images() {
        let link = crate::files::holder::runtime(state, user, id, (workspace, true)).await?;
        let via = super::linked::Via {
            links: &state.links,
            conversation: &link,
            timeout: state.config.io_timeout(),
            cancel: &state.cancel,
        };
        let place = Place::new(&state.config.storage_root, workspace);
        return super::live::LIVE
            .get(workspace, || async {
                super::linked::scan(&via, Some(&previous), &limits, &place.blobs()).await
            })
            .await;
    }
    let root = state.storage.workspace(workspace).await?;
    let storage_root = state.config.storage_root.clone();
    super::live::LIVE
        .get(workspace, || {
            blocking(move || {
                let place = Place::new(&storage_root, workspace);
                live(&root, &previous, &limits, &place)
            })
        })
        .await
}

fn live(
    root: &std::path::Path,
    before: &Manifest,
    limits: &super::manifest::Limits,
    place: &Place,
) -> Result<Manifest, GatewayError> {
    let root = path::open_root(root)?;
    scan(root.as_fd(), Some(before), limits, &place.blobs())
}

/// `GET /conversations/{id}/runs/{run_id}/changes`: the files the run added, modified or deleted.
///
/// # Errors
/// `404` for an unknown run, `404 changes_unavailable` when it was not recorded, `403`, `409 files_unavailable`,
/// host I/O failures.
pub(crate) async fn run_changes(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath((id, run)): UrlPath<(ConversationId, String)>,
) -> Result<Json<RunChanges>, GatewayError> {
    let compared = compared(&state, &user, &id, &run).await?;
    let live = compared.bounds.after.is_none();
    let running = compared.bounds.running;
    let truncated = compared.before.truncated || compared.after.truncated;
    let files = blocking(move || {
        Ok(compare::changes(
            &compared.before,
            &compared.after,
            &compared.place.blobs(),
        ))
    })
    .await?;
    Ok(Json(RunChanges {
        run_id: run,
        live,
        running,
        truncated,
        files,
    }))
}

/// `GET /conversations/{id}/runs/{run_id}/changes/diff?path=`: one changed file's unified diff.
///
/// # Errors
/// `400 path_invalid`; `404` when the run or the file's change is unknown; as [`run_changes`].
pub(crate) async fn diff(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath((id, run)): UrlPath<(ConversationId, String)>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<FileDiff>, GatewayError> {
    let target = WorkspacePath::parse(&query.path)?;
    if target.is_root() {
        return Err(GatewayError::PathInvalid);
    }
    let path = target.to_string();
    let max = usize::try_from(state.config.files.diff_bytes).unwrap_or(usize::MAX);
    let compared = compared(&state, &user, &id, &run).await?;
    let diff = blocking(move || {
        compare::unified(
            &compared.before,
            &compared.after,
            &compared.place.blobs(),
            &path,
            max,
        )
        .ok_or(GatewayError::NotFound)
    })
    .await?;
    Ok(Json(diff))
}
