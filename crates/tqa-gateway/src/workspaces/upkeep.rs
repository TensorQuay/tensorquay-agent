//! Workspace upkeep (task 066c): measuring a workspace once no sandbox has it mounted, the retention sweep, and
//! recovering forks a stopped gateway left half-copied.
use super::copy::{self, Limits};
use crate::{GatewayError, app::State, storage::Storage};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

/// Retention runs this often when it is on.
const SWEEP: Duration = Duration::from_hours(1);
/// Workspaces one sweep deletes at most.
const SWEEP_BATCH: i64 = 100;

/// The bytes charged for the tree at `path` while a sandbox writes to it: `Some` within `bytes`, `None` past it
/// (or past the entry or depth bound; the walk stops there). Blocking: run it on the blocking pool.
///
/// # Errors
/// The walk's I/O failure; the caller measures again later.
pub(crate) fn charged_within(path: &std::path::Path, bytes: u64) -> std::io::Result<Option<u64>> {
    match copy::measure_within(
        path,
        Limits {
            bytes,
            entries: super::fork::MAX_ENTRIES,
        },
    ) {
        Ok(charged) => Ok(Some(charged)),
        Err(copy::CopyError::Exceeded) => Ok(None),
        Err(copy::CopyError::Io(error)) => Err(error),
    }
}

/// Measures `conversation`'s workspace in the background and records its size (the quota check reads it).
pub(crate) fn measure_later(
    pool: &PgPool,
    storage: &Storage,
    tasks: &tokio_util::task::TaskTracker,
    conversation: Uuid,
) {
    let (pool, storage) = (pool.clone(), storage.clone());
    tasks.spawn(async move {
        let Ok(Some(workspace)) =
            sqlx::query_scalar!("SELECT workspace_id FROM conversations WHERE id=$1", conversation)
                .fetch_optional(&pool)
                .await
        else {
            return;
        };
        let bytes = if storage.block_images() {
            // A microVM workspace is an image the gateway never reads: its allocation on the host disk is the measure.
            storage.image_allocation(workspace).await
        } else {
            let root = storage.workspace_path(workspace);
            let limits = Limits {
                bytes: u64::MAX,
                entries: super::fork::MAX_ENTRIES,
            };
            tokio::task::spawn_blocking(move || copy::measure(&root, limits))
                .await
                .ok()
                .and_then(Result::ok)
        };
        let Some(bytes) = bytes else {
            tracing::warn!(workspace_id = %workspace, "workspace size could not be measured");
            return;
        };
        let size = i64::try_from(bytes).unwrap_or(i64::MAX);
        if sqlx::query!(
            "UPDATE workspaces SET size_bytes=$2, size_measured_at=CURRENT_TIMESTAMP WHERE id=$1 AND state='ready'",
            workspace,
            size
        )
        .execute(&pool)
        .await
        .is_err()
        {
            tracing::warn!(workspace_id = %workspace, "workspace size could not be recorded");
        }
    });
}

/// Deletes up to one batch of workspaces unused for the retention period, with no active task and no sandbox.
/// Returns how many were deleted; nothing when retention is off.
///
/// # Errors
/// A store or storage failure.
pub(crate) async fn retention(state: &State) -> Result<u64, GatewayError> {
    let Some(period) = state.config.workspaces.retention else {
        return Ok(0);
    };
    let seconds = period.as_secs_f64();
    let candidates = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT w.id FROM workspaces w WHERE w.state='ready' AND w.lease_conversation_id IS NULL AND COALESCE(w.last_used_at, w.created_at) < CURRENT_TIMESTAMP - make_interval(secs=>$1) AND NOT EXISTS(SELECT 1 FROM conversations c WHERE c.workspace_id=w.id AND c.state='active') ORDER BY COALESCE(w.last_used_at, w.created_at) LIMIT $2",
            seconds,
            SWEEP_BATCH
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let mut deleted = 0;
    for workspace in candidates {
        // Checked again under the row lock: a task may have been bound since the list was read.
        let gone = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            let mut transaction = state.pool.begin().await?;
            let changed = sqlx::query!(
                "UPDATE workspaces w SET state='deleted', updated_at=CURRENT_TIMESTAMP WHERE w.id=$1 AND w.state='ready' AND w.lease_conversation_id IS NULL AND COALESCE(w.last_used_at, w.created_at) < CURRENT_TIMESTAMP - make_interval(secs=>$2) AND NOT EXISTS(SELECT 1 FROM conversations c WHERE c.workspace_id=w.id AND c.state='active')",
                workspace,
                seconds
            )
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if changed == 1 {
                sqlx::query!("DELETE FROM workspace_shares WHERE workspace_id=$1", workspace)
                    .execute(&mut *transaction)
                    .await?;
                crate::settings::audit_system(&mut transaction, "workspaces", &workspace.to_string(), "delete")
                    .await?;
            }
            transaction.commit().await?;
            Ok(changed == 1)
        })
        .await?;
        if gone {
            state.purge.wake();
            state.storage.remove_workspace_tree(workspace).await?;
            tracing::info!(workspace_id = %workspace, "unused workspace deleted by retention");
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Runs the retention sweep hourly while it is on.
pub(crate) fn start(state: &Arc<State>) {
    if state.config.workspaces.retention.is_none() {
        return;
    }
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut interval = tokio::time::interval(SWEEP);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;
                () = owned.cancel.cancelled() => break,
                _ = interval.tick() => if retention(&owned).await.is_err() && !owned.cancel.is_cancelled() {
                    tracing::warn!("workspace retention sweep failed");
                },
            }
        }
    });
}

/// At start-up no fork copy is still running: one the gateway was copying when it stopped has failed, and its
/// partial copy is removed.
///
/// # Errors
/// A store or storage failure.
pub(crate) async fn recover(pool: &PgPool, storage: &Storage) -> Result<(), GatewayError> {
    let failed = sqlx::query_scalar!(
        "UPDATE workspaces SET state='failed', updated_at=CURRENT_TIMESTAMP WHERE state='forking' RETURNING id"
    )
    .fetch_all(pool)
    .await?;
    for workspace in failed {
        storage.remove_workspace_tree(workspace).await?;
        if !storage.block_images() {
            storage.workspace(workspace).await?;
        }
        tracing::warn!(workspace_id = %workspace, "interrupted workspace fork marked failed");
    }
    Ok(())
}
