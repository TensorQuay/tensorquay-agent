//! `POST /api/v1/workspaces/{id}/fork {name}` (task 066c, answers-066 Q5): anyone who may read a workspace copies it
//! into a new workspace of their own. The copy runs in the background (202, `forking`, then `ready` or `failed`),
//! bounded by the quota and never waiting for the write lease; a copy taken while a sandbox had the source mounted
//! is marked `consistent: false`, as files may have been mid-change. On the microVM backend the copy is of the
//! workspace's block image, never mounted or read as a filesystem (ARCHITECTURE §4.5, §4.6).
use super::{
    access,
    copy::{self, CopyError, Limits},
    endpoints::{Workspace, fetch, id},
    image_copy, valid_name,
};
use crate::{GatewayError, app::State, auth::Authz, request::ApiJson};
use axum::{
    Json,
    extract::{Path, State as AppState},
    http::StatusCode,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

/// Directory entries one fork (or measurement) may walk.
pub(super) const MAX_ENTRIES: u64 = 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ForkBody {
    name: String,
}

/// # Errors
/// An invalid name, `404`, `403` without access, `409 workspace_not_ready` for a source still forking or failed, or
/// a store or storage failure.
pub(crate) async fn fork(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(source): Path<String>,
    ApiJson(body): ApiJson<ForkBody>,
) -> Result<(StatusCode, Json<Workspace>), GatewayError> {
    let source = id(&source)?;
    let name = valid_name(&body.name)?.to_owned();
    let fork = Uuid::now_v7();
    // A microVM workspace is one image file the copy creates; a Docker one is a directory created now.
    let block_images = state.config.uses_block_images();
    let path = if block_images {
        None
    } else {
        Some(state.storage.workspace(fork).await?)
    };
    let created = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let (quota, _) = begin(
            &mut transaction,
            user.id,
            source,
            (fork, &name, super::Kind::Standard),
        )
        .await?;
        transaction.commit().await?;
        Ok((
            fetch(&state, &user, fork).await?,
            crate::workspaces::quota(&state.config, quota),
        ))
    })
    .await;
    let (view, quota) = match created {
        Ok(created) => created,
        Err(error) => {
            if let Some(path) = &path {
                state.storage.remove_workspace(path).await?;
            }
            return Err(error);
        }
    };
    tracing::info!(user_id = %user.id, workspace_id = %fork, source_id = %source, "workspace fork started");
    copy(&state, (source, fork), quota).await?;
    Ok((StatusCode::ACCEPTED, Json(view)))
}

/// Records `fork` as a copy of `source` being made for `owner`, in the caller's transaction, and answers the fork's
/// stored quota (its source's) and whether the copy is consistent (no sandbox had the source mounted). Any access may fork (a read share included); the copy starts with [`copy`] once the
/// transaction commits. A copy of a workspace a sandbox has mounted is marked `consistent: false`.
///
/// # Errors
/// `404`, `403` without access, `409 workspace_not_ready` for a source still forking or failed, storage failure.
pub(crate) async fn begin(
    connection: &mut sqlx::PgConnection,
    owner: Uuid,
    source: Uuid,
    (fork, name, kind): (Uuid, &str, super::Kind),
) -> Result<(Option<i64>, bool), GatewayError> {
    access(connection, owner, source).await?;
    let row = sqlx::query!(
        "SELECT state, source_url, quota_bytes, EXISTS(SELECT 1 FROM sandbox_instances s JOIN conversations c ON c.id=s.conversation_id WHERE c.workspace_id=w.id) AS \"live!\" FROM workspaces w WHERE id=$1 FOR SHARE",
        source
    )
    .fetch_one(&mut *connection)
    .await?;
    if row.state != "ready" {
        return Err(GatewayError::Conflict("workspace_not_ready"));
    }
    sqlx::query!(
        // A fork keeps its source's quota (the environment sizes design).
        "INSERT INTO workspaces (id, owner_id, name, source_url, state, forked_from, fork_consistent, quota_bytes, kind) VALUES ($1,$2,$3,$4,'forking',$5,$6,$7,$8)",
        fork, owner, name, row.source_url, source, !row.live, row.quota_bytes, kind.as_str()
    )
    .execute(&mut *connection)
    .await?;
    Ok((row.quota_bytes, !row.live))
}

/// Copies `source` into `fork` in the background, as the fork route does, then marks the fork `ready` or `failed`.
///
/// # Errors
/// Storage failure preparing the copy.
pub(crate) async fn copy(
    state: &Arc<State>,
    (source, fork): (Uuid, Uuid),
    quota: u64,
) -> Result<(), GatewayError> {
    if state.config.uses_block_images() {
        copy_image_in_background(state, (source, fork), quota).await
    } else {
        copy_in_background(state, (source, fork), quota);
        Ok(())
    }
}

/// Copies `source` into `fork`, then marks the fork `ready` with its size, or `failed` with nothing left in it.
fn copy_in_background(state: &Arc<State>, (source, fork): (Uuid, Uuid), quota: u64) {
    let owned = state.clone();
    let (from, to) = (
        state.storage.workspace_path(source),
        state.storage.workspace_path(fork),
    );
    let limits = Limits {
        bytes: quota,
        entries: MAX_ENTRIES,
    };
    state.tasks.spawn(async move {
        let copied = tokio::task::spawn_blocking(move || copy::copy_tree(&from, &to, limits)).await;
        let result = match copied {
            Ok(Ok(bytes)) => finish(&owned, fork, Some(bytes)).await,
            Ok(Err(error)) => {
                let reason = if matches!(error, CopyError::Exceeded) {
                    "quota"
                } else {
                    "io"
                };
                tracing::warn!(workspace_id = %fork, reason, "workspace fork failed");
                finish(&owned, fork, None).await
            }
            Err(_) => finish(&owned, fork, None).await,
        };
        if result.is_err() {
            tracing::warn!(workspace_id = %fork, "workspace fork outcome could not be recorded");
        }
    });
}

/// Copies the source's image into the fork's (ARCHITECTURE §4.6), then records the outcome as for a directory copy.
async fn copy_image_in_background(
    state: &Arc<State>,
    (source, fork): (Uuid, Uuid),
    quota: u64,
) -> Result<(), GatewayError> {
    let (from, to) = (
        state.storage.workspace_image(source).await?,
        state.storage.workspace_image(fork).await?,
    );
    let owned = state.clone();
    state.tasks.spawn(async move {
        let copied =
            tokio::task::spawn_blocking(move || image_copy::copy_image(&from, &to, quota)).await;
        let outcome = match copied {
            // A workspace never started has no image yet: the fork is empty, and its first start creates one.
            Ok(Ok(allocated)) => Some(allocated.unwrap_or(0)),
            Ok(Err(error)) => {
                let reason = if matches!(error, CopyError::Exceeded) {
                    "quota"
                } else {
                    "io"
                };
                tracing::warn!(workspace_id = %fork, reason, "workspace image fork failed");
                None
            }
            Err(_) => None,
        };
        if finish(&owned, fork, outcome).await.is_err() {
            tracing::warn!(workspace_id = %fork, "workspace fork outcome could not be recorded");
        }
    });
    Ok(())
}

/// Records the copy's outcome, then starts an agent team member waiting on this copy (ADR-0034): its first turn
/// goes now, or it fails with a notice to its lead.
async fn finish(state: &State, fork: Uuid, copied: Option<u64>) -> Result<(), GatewayError> {
    record(state, fork, copied).await?;
    if let Err(error) = crate::teams::first_turn::copy_ended(state, fork).await {
        tracing::warn!(workspace_id = %fork, code = error.code(), "a helper's first turn wasn't sent after its copy");
    }
    Ok(())
}

async fn record(state: &State, fork: Uuid, copied: Option<u64>) -> Result<(), GatewayError> {
    let Some(bytes) = copied else {
        // Nothing is left behind: the partial copy goes. A directory fork keeps its empty directory for a later
        // delete; an image fork never had one.
        state.storage.remove_workspace_tree(fork).await?;
        if !state.config.uses_block_images() {
            state.storage.workspace(fork).await?;
        }
        sqlx::query!("UPDATE workspaces SET state='failed', updated_at=CURRENT_TIMESTAMP WHERE id=$1 AND state='forking'", fork)
            .execute(&state.pool)
            .await?;
        return Ok(());
    };
    let size = i64::try_from(bytes).unwrap_or(i64::MAX);
    sqlx::query!(
        "UPDATE workspaces SET state='ready', size_bytes=$2, size_measured_at=CURRENT_TIMESTAMP, updated_at=CURRENT_TIMESTAMP WHERE id=$1 AND state='forking'",
        fork,
        size
    )
    .execute(&state.pool)
    .await?;
    tracing::info!(workspace_id = %fork, size_bytes = size, "workspace fork ready");
    Ok(())
}
