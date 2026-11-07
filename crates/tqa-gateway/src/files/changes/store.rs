//! Snapshot records and files: rows in `workspace_snapshots`, manifests and copies under
//! `<storage>/changes/<workspace>/`, and which snapshots bound a run.
use super::blobs::Blobs;
use super::manifest::Manifest;
use crate::GatewayError;
use sqlx::PgPool;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// One workspace's changes directory: `snapshots/<id>.json` and `blobs/<sha256>`.
pub(crate) struct Place {
    directory: PathBuf,
}

impl Place {
    pub(crate) fn new(storage_root: &Path, workspace: Uuid) -> Self {
        Self {
            directory: storage_root.join("changes").join(workspace.to_string()),
        }
    }

    pub(crate) fn blobs(&self) -> Blobs {
        Blobs::new(self.directory.join("blobs"))
    }

    fn manifest_path(&self, snapshot: Uuid) -> PathBuf {
        self.directory
            .join("snapshots")
            .join(format!("{snapshot}.json"))
    }

    /// # Errors
    /// Host I/O failures.
    pub(crate) fn write(&self, snapshot: Uuid, manifest: &Manifest) -> Result<(), GatewayError> {
        let path = self.manifest_path(snapshot);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_vec(manifest)?)?;
        Ok(())
    }

    /// # Errors
    /// `changes_unavailable` when the manifest is gone; host I/O failures.
    pub(crate) fn read(&self, snapshot: Uuid) -> Result<Manifest, GatewayError> {
        let bytes = std::fs::read(self.manifest_path(snapshot)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GatewayError::ChangesUnavailable
            } else {
                GatewayError::from(error)
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|_| GatewayError::ChangesUnavailable)
    }

    /// Keeps the manifests of `kept` and the copies they reference; removes everything else.
    ///
    /// # Errors
    /// Host I/O failures.
    pub(crate) fn prune(&self, kept: &[Uuid]) -> Result<(), GatewayError> {
        let mut referenced = HashSet::new();
        for snapshot in kept {
            if let Ok(manifest) = self.read(*snapshot) {
                referenced.extend(
                    manifest
                        .files
                        .into_values()
                        .filter_map(|record| record.copied.then_some(record.hash).flatten()),
                );
            }
        }
        let keep: HashSet<String> = kept.iter().map(|id| format!("{id}.json")).collect();
        let entries = std::fs::read_dir(self.directory.join("snapshots"))
            .into_iter()
            .flatten()
            .collect::<Result<Vec<_>, _>>()?;
        for entry in entries.into_iter().filter(|entry| {
            !entry
                .file_name()
                .to_str()
                .is_some_and(|name| keep.contains(name))
        }) {
            std::fs::remove_file(entry.path())?;
        }
        self.blobs().prune(&referenced)
    }
}

/// The latest snapshot of a workspace, whose records the next scan reuses.
///
/// # Errors
/// Database failures.
pub(crate) async fn latest(pool: &PgPool, workspace: Uuid) -> Result<Option<Uuid>, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT id FROM workspace_snapshots WHERE workspace_id=$1 ORDER BY taken_at DESC LIMIT 1",
        workspace
    )
    .fetch_optional(pool)
    .await?)
}

/// The delivering conversation's last event seq.
///
/// # Errors
/// Database failures.
pub(crate) async fn last_seq(pool: &PgPool, conversation: Uuid) -> Result<i64, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT MAX(seq) FROM events WHERE conversation_id=$1",
        conversation
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0))
}

pub(crate) struct Taken {
    pub id: Uuid,
    pub workspace: Uuid,
    pub conversation: Uuid,
    pub after_seq: i64,
    pub files: usize,
    pub truncated: bool,
}

/// Records a snapshot and returns the ids to keep (the newest `kept`), removing the older rows.
///
/// # Errors
/// Database failures.
pub(crate) async fn record(
    pool: &PgPool,
    taken: &Taken,
    kept: usize,
) -> Result<Vec<Uuid>, GatewayError> {
    let files = i32::try_from(taken.files).unwrap_or(i32::MAX);
    let kept = i64::try_from(kept).unwrap_or(i64::MAX);
    let mut transaction = pool.begin().await?;
    sqlx::query!(
        "INSERT INTO workspace_snapshots (id,workspace_id,conversation_id,after_seq,files,truncated) VALUES ($1,$2,$3,$4,$5,$6)",
        taken.id, taken.workspace, taken.conversation, taken.after_seq, files, taken.truncated
    )
    .execute(&mut *transaction)
    .await?;
    let keep = sqlx::query_scalar!(
        "SELECT id FROM workspace_snapshots WHERE workspace_id=$1 ORDER BY taken_at DESC LIMIT $2",
        taken.workspace,
        kept
    )
    .fetch_all(&mut *transaction)
    .await?;
    sqlx::query!(
        "DELETE FROM workspace_snapshots WHERE workspace_id=$1 AND NOT (id = ANY($2))",
        taken.workspace,
        &keep
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(keep)
}

/// Workspaces deleted since their changes were recorded, among `candidates`.
///
/// # Errors
/// Database failures.
pub(crate) async fn deleted(pool: &PgPool, candidates: &[Uuid]) -> Result<Vec<Uuid>, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT id FROM workspaces WHERE id = ANY($1) AND state='deleted'",
        candidates
    )
    .fetch_all(pool)
    .await?)
}

/// The snapshots around one run.
pub(crate) struct Bounds {
    pub before: Uuid,
    /// `None`: compare with the live workspace (the latest run, or one still running).
    pub after: Option<Uuid>,
    pub running: bool,
}

/// Finds the snapshot taken before `run` started and the first one taken after it ended.
///
/// # Errors
/// `404` for a run the conversation never started; `changes_unavailable` when no snapshot precedes it; database
/// failures.
pub(crate) async fn bounds(
    pool: &PgPool,
    workspace: Uuid,
    conversation: Uuid,
    run: &str,
) -> Result<Bounds, GatewayError> {
    let marks = sqlx::query!(
        r#"SELECT MIN(seq) FILTER (WHERE lifecycle='run.started') AS "started",
                  MAX(seq) FILTER (WHERE lifecycle IN ('run.completed','run.failed','run.cancelled')) AS "ended"
           FROM events WHERE conversation_id=$1 AND (run_id=$2 OR frame->'event'->>'run_id'=$2)"#,
        conversation,
        run
    )
    .fetch_one(pool)
    .await?;
    let started = marks.started.ok_or(GatewayError::NotFound)?;
    let before = sqlx::query!(
        "SELECT id,taken_at FROM workspace_snapshots WHERE conversation_id=$1 AND after_seq<$2 ORDER BY after_seq DESC, taken_at DESC LIMIT 1",
        conversation,
        started
    )
    .fetch_optional(pool)
    .await?
    .ok_or(GatewayError::ChangesUnavailable)?;
    let Some(ended) = marks.ended else {
        return Ok(Bounds {
            before: before.id,
            after: None,
            running: true,
        });
    };
    // Any task's next delivery on the workspace closes the run (066's lease orders them); a snapshot this task took
    // while the run was still going is not its end.
    let after = sqlx::query_scalar!(
        "SELECT id FROM workspace_snapshots WHERE workspace_id=$1 AND taken_at>$2 AND NOT (conversation_id=$3 AND after_seq<$4) ORDER BY taken_at LIMIT 1",
        workspace,
        before.taken_at,
        conversation,
        ended
    )
    .fetch_optional(pool)
    .await?;
    Ok(Bounds {
        before: before.id,
        after,
        running: false,
    })
}
