//! Delete means gone (PRD §12 item 18): a deleted task or workspace is removed in the background within minutes, its
//! events, stored outputs and files; only the content-free audit row written at the delete stays.
//!
//! The queue is the database: a `conversations` or `workspaces` row in state `deleted` is an outstanding purge, and
//! its purge deletes that row last. Each step is idempotent (a removal that finds nothing is done) and files go before
//! rows, so a purge stopped anywhere, by a failure or a restart, is finished by a later cycle with nothing recorded
//! elsewhere.
//!
//! One supervised task (`state.tasks`, stopped by `state.cancel`), woken by a delete or every minute. A cycle holds one
//! pooled connection, runs one bounded statement at a time on it (each deletes at most [`BATCH`] rows), never holds a
//! transaction across a file removal, and never touches a live task: an active one is never selected, and a deleted one
//! waits while it still has a sandbox or a runtime link.
use crate::{GatewayError, app::State};
use sqlx::PgConnection;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use uuid::Uuid;

/// Rows one statement deletes or changes at most: well inside `io_timeout` on a loaded host.
pub(crate) const BATCH: i64 = 500;
/// Tasks one cycle takes on; a large transcript is about a minute's work.
const TASKS_PER_CYCLE: usize = 4;
/// Workspaces one cycle takes on, after their tasks are gone.
const WORKSPACES_PER_CYCLE: usize = 2;
/// The sweep: a minute after the last cycle, unless a delete wakes it first.
const SWEEP: Duration = Duration::from_mins(1);
/// The pause between cycles while a backlog remains, so a large one still finishes within minutes.
const BACKLOG_PAUSE: Duration = Duration::from_secs(1);
/// Cycles in any one minute at most, wakes included: a target that fails a step every cycle can't loop hot.
pub(crate) const MAX_CYCLES_PER_MINUTE: u32 = 20;

/// The purge's wake-up: a delete calls [`Purge::wake`] after its commit.
#[derive(Default)]
pub(crate) struct Purge {
    wake: Notify,
    held: AtomicBool,
}

impl Purge {
    /// Runs a cycle soon (it stores one wake while a cycle is running).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Holds the background cycles (`true`) or lets them run, so a test can look at a deleted row before it goes;
    /// [`cycle`] still runs when called. There is no endpoint.
    pub fn hold(&self, held: bool) {
        self.held.store(held, Ordering::SeqCst);
        if !held {
            self.wake();
        }
    }
}

/// What one cycle did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cycle {
    /// Tasks purged: their rows and files are gone.
    pub tasks: u32,
    /// Workspaces purged.
    pub workspaces: u32,
    /// Targets not ready yet: a deleted task with a sandbox or a runtime link, or a workspace whose tasks remain.
    pub deferred: u32,
    /// Targets a step failed for; the next cycle tries them again.
    pub failed: u32,
    /// Whether more targets were waiting than one cycle takes on and this one made progress: the next follows soon.
    pub more: bool,
    /// Screenshots past `desktop.screenshot_retention_days` removed this cycle (ADR-0039 section 9).
    pub screenshots: usize,
}

/// Starts the purge on the gateway's supervisor.
pub(crate) fn start(state: &Arc<State>) {
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut window = Instant::now();
        let mut cycles = 0;
        loop {
            if window.elapsed() >= Duration::from_mins(1) {
                window = Instant::now();
                cycles = 0;
            }
            let more = if owned.purge.held.load(Ordering::SeqCst) || cycles >= MAX_CYCLES_PER_MINUTE
            {
                false
            } else {
                cycles += 1;
                background_cycle(&owned).await
            };
            let pause = if cycles >= MAX_CYCLES_PER_MINUTE {
                Duration::from_mins(1).saturating_sub(window.elapsed())
            } else if more {
                BACKLOG_PAUSE
            } else {
                SWEEP
            };
            tokio::select! {biased;
                () = owned.cancel.cancelled() => break,
                () = owned.purge.wake.notified(), if cycles < MAX_CYCLES_PER_MINUTE => {},
                () = tokio::time::sleep(pause) => {},
            }
        }
    });
}

/// One cycle of the background task: whether another should follow soon. Its failure is logged, never content.
async fn background_cycle(state: &State) -> bool {
    match Box::pin(cycle(state)).await {
        Ok(cycle) => cycle.more,
        Err(error) => {
            if !state.cancel.is_cancelled() {
                tracing::warn!(
                    code = error.code(),
                    "purge cycle failed: no connection in time"
                );
            }
            false
        }
    }
}

/// One cycle on one connection: up to [`TASKS_PER_CYCLE`] deleted tasks, oldest first, then up to
/// [`WORKSPACES_PER_CYCLE`] deleted workspaces whose tasks are all gone; then the oldest-waiting gauge, and the
/// desktop screenshots past their retention.
///
/// # Errors
/// The connection could not be had in time. A target's failed step is logged and counted, not returned.
pub(crate) async fn cycle(state: &State) -> Result<Cycle, GatewayError> {
    let mut connection = Box::pin(bounded(state, state.pool.acquire())).await?;
    let connection = &mut *connection;
    let mut cycle = Cycle::default();

    let tasks = bounded(
        state,
        sqlx::query_scalar!(
            "SELECT c.id FROM conversations c WHERE c.state='deleted' \
             AND NOT EXISTS(SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=c.id) \
             ORDER BY c.updated_at, c.id LIMIT $1",
            limit(TASKS_PER_CYCLE + 1)
        )
        .fetch_all(&mut *connection),
    )
    .await?;
    let mut full = tasks.len() > TASKS_PER_CYCLE;
    for task in tasks.into_iter().take(TASKS_PER_CYCLE) {
        let outcome = purge_task(state, connection, task).await;
        count(state, &mut cycle, "task", task, outcome);
    }
    // A deleted task waiting for its sandbox to go is not selected above, so it never holds up the rest.
    let sandboxed = bounded(
        state,
        sqlx::query_scalar!(
            "SELECT count(*) AS \"count!\" FROM conversations c WHERE c.state='deleted' \
             AND EXISTS(SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=c.id)"
        )
        .fetch_one(&mut *connection),
    )
    .await?;
    let sandboxed = u32::try_from(sandboxed).unwrap_or(u32::MAX);
    state
        .telemetry
        .purge_targets("task", "deferred", u64::from(sandboxed));
    cycle.deferred = cycle.deferred.saturating_add(sandboxed);

    let workspaces = bounded(
        state,
        sqlx::query_scalar!(
            "SELECT w.id FROM workspaces w WHERE w.state='deleted' \
             AND NOT EXISTS(SELECT 1 FROM conversations c WHERE c.workspace_id=w.id) \
             ORDER BY w.updated_at, w.id LIMIT $1",
            limit(WORKSPACES_PER_CYCLE + 1)
        )
        .fetch_all(&mut *connection),
    )
    .await?;
    full |= workspaces.len() > WORKSPACES_PER_CYCLE;
    for workspace in workspaces.into_iter().take(WORKSPACES_PER_CYCLE) {
        let outcome = purge_workspace(state, connection, workspace).await;
        count(state, &mut cycle, "workspace", workspace, outcome);
    }

    let oldest = bounded(
        state,
        sqlx::query_scalar!(
            "SELECT COALESCE(EXTRACT(EPOCH FROM CURRENT_TIMESTAMP - LEAST( \
               (SELECT min(updated_at) FROM conversations WHERE state='deleted'), \
               (SELECT min(updated_at) FROM workspaces WHERE state='deleted')))::double precision, 0) AS \"seconds!\""
        )
        .fetch_one(&mut *connection),
    )
    .await?;
    state.telemetry.purge_oldest(oldest.max(0.0));
    // The desktop's old screenshots, page by page past any that can't be removed; those are tried again next cycle.
    match crate::screen::purge_expired(state).await {
        Ok(screenshots) => cycle.screenshots = screenshots,
        Err(error) => tracing::warn!(
            code = error.code(),
            "the screenshot retention purge failed; retried next cycle"
        ),
    }
    cycle.more = full && cycle.tasks + cycle.workspaces > 0;
    Ok(cycle)
}

/// How one target ended.
enum Outcome {
    Done(Counts),
    Deferred,
}

/// What a finished target's purge removed, for its log line.
#[derive(Default)]
struct Counts {
    events: u64,
    rows: u64,
}

/// A failed step: its name for the log, never content.
type Failed = (&'static str, GatewayError);

fn count(
    state: &State,
    cycle: &mut Cycle,
    kind: &'static str,
    id: Uuid,
    outcome: Result<Outcome, Failed>,
) {
    match outcome {
        Ok(Outcome::Done(counts)) => {
            if kind == "task" {
                cycle.tasks += 1;
                tracing::info!(conversation_id = %id, events = counts.events, rows = counts.rows, "deleted task purged");
            } else {
                cycle.workspaces += 1;
                tracing::info!(workspace_id = %id, rows = counts.rows, "deleted workspace purged");
            }
            state.telemetry.purge_targets(kind, "done", 1);
        }
        Ok(Outcome::Deferred) => {
            cycle.deferred += 1;
            state.telemetry.purge_targets(kind, "deferred", 1);
        }
        Err((step, error)) => {
            cycle.failed += 1;
            if kind == "task" {
                tracing::warn!(conversation_id = %id, step, code = error.code(), "purge step failed; retried next cycle");
            } else {
                tracing::warn!(workspace_id = %id, step, code = error.code(), "purge step failed; retried next cycle");
            }
            state.telemetry.purge_targets(kind, "failed", 1);
        }
    }
}

/// Runs one statement within `io_timeout`, stopping at shutdown.
async fn bounded<T>(
    state: &State,
    statement: impl Future<Output = Result<T, sqlx::Error>>,
) -> Result<T, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(statement.await?)
    })
    .await
}

fn limit(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// Repeats a batched statement (`$2` is its batch size) until a batch changes fewer rows than [`BATCH`].
macro_rules! batches {
    ($state:expr, $connection:expr, $step:literal, $sql:literal, $target:expr) => {{
        let mut total = 0_u64;
        loop {
            let changed = bounded(
                $state,
                sqlx::query!($sql, $target, BATCH).execute(&mut *$connection),
            )
            .await
            .map_err(|error| ($step, error))?
            .rows_affected();
            total += changed;
            if changed < BATCH.unsigned_abs() {
                break total;
            }
        }
    }};
}

/// Purges one deleted task: its files, its snapshots' manifests and rows, its transcript and other rows, then the row.
async fn purge_task(
    state: &State,
    connection: &mut PgConnection,
    task: Uuid,
) -> Result<Outcome, Failed> {
    let Some(owner) = ready_task(state, connection, task).await? else {
        return Ok(Outcome::Deferred);
    };
    state
        .storage
        .remove_task_files(task)
        .await
        .map_err(|error| ("files", error))?;
    let mut counts = Counts {
        rows: task_snapshots(state, connection, task).await?,
        events: task_events(state, connection, task).await?,
    };
    counts.rows += task_rows(state, connection, task).await?;
    counts.rows += team_log(state, connection, task).await?;
    detach_kept(state, connection, task, owner).await?;
    // The row last, under the same guard: channel memberships and the stored answers about it cascade, usage detaches.
    let gone = bounded(
        state,
        sqlx::query!(
            "DELETE FROM conversations c WHERE c.id=$1 AND c.state='deleted' \
             AND NOT EXISTS(SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=c.id)",
            task
        )
        .execute(&mut *connection),
    )
    .await
    .map_err(|error| ("row", error))?
    .rows_affected();
    Ok(if gone == 1 {
        Outcome::Done(counts)
    } else {
        Outcome::Deferred
    })
}

/// The task's owner when it may be purged now: still deleted, no sandbox, and no runtime link (the runtime token went
/// at the delete, so the sandbox is being retired; a later cycle finds it gone). Never a live task.
async fn ready_task(
    state: &State,
    connection: &mut PgConnection,
    task: Uuid,
) -> Result<Option<Uuid>, Failed> {
    let owner = bounded(
        state,
        sqlx::query_scalar!(
            "SELECT c.owner_id FROM conversations c WHERE c.id=$1 AND c.state='deleted' \
             AND NOT EXISTS(SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=c.id)",
            task
        )
        .fetch_optional(&mut *connection),
    )
    .await
    .map_err(|error| ("guard", error))?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    let id: tqa_types::ConversationId = task
        .to_string()
        .parse()
        .map_err(|_| ("guard", GatewayError::Io))?;
    if state.links.connected(&id) {
        return Ok(None);
    }
    Ok(Some(owner))
}

/// Removes the task's run snapshots: each manifest file, then the rows. How many rows went.
async fn task_snapshots(
    state: &State,
    connection: &mut PgConnection,
    task: Uuid,
) -> Result<u64, Failed> {
    let mut removed = 0;
    loop {
        let snapshots = bounded(
            state,
            sqlx::query!(
                "SELECT id, workspace_id FROM workspace_snapshots WHERE conversation_id=$1 LIMIT $2",
                task,
                BATCH
            )
            .fetch_all(&mut *connection),
        )
        .await
        .map_err(|error| ("snapshots", error))?;
        if snapshots.is_empty() {
            break;
        }
        for snapshot in &snapshots {
            state
                .storage
                .remove_snapshot(snapshot.workspace_id, snapshot.id)
                .await
                .map_err(|error| ("snapshot files", error))?;
        }
        let ids: Vec<Uuid> = snapshots.iter().map(|snapshot| snapshot.id).collect();
        removed += bounded(
            state,
            sqlx::query!("DELETE FROM workspace_snapshots WHERE id = ANY($1)", &ids)
                .execute(&mut *connection),
        )
        .await
        .map_err(|error| ("snapshots", error))?
        .rows_affected();
    }

    Ok(removed)
}

/// Removes the task's transcript. How many events went.
async fn task_events(
    state: &State,
    connection: &mut PgConnection,
    task: Uuid,
) -> Result<u64, Failed> {
    let mut removed = 0;
    // The transcript by sequence ranges on its primary key: each statement ends at the batch's last sequence number.
    loop {
        let cut = bounded(
            state,
            sqlx::query_scalar!(
                "SELECT seq FROM events WHERE conversation_id=$1 ORDER BY seq OFFSET $2 LIMIT 1",
                task,
                BATCH - 1
            )
            .fetch_optional(&mut *connection),
        )
        .await
        .map_err(|error| ("events", error))?;
        removed += bounded(
            state,
            sqlx::query!(
                "DELETE FROM events WHERE conversation_id=$1 AND ($2::bigint IS NULL OR seq <= $2)",
                task,
                cut
            )
            .execute(&mut *connection),
        )
        .await
        .map_err(|error| ("events", error))?
        .rows_affected();
        if cut.is_none() {
            break;
        }
    }

    Ok(removed)
}

/// Removes the task's stored answers and held messages, push operations and runtime tokens. How many rows went.
async fn task_rows(
    state: &State,
    connection: &mut PgConnection,
    task: Uuid,
) -> Result<u64, Failed> {
    let mut removed = 0;
    removed += batches!(
        state,
        connection,
        "idempotency",
        "DELETE FROM idempotency WHERE ctid = ANY(ARRAY(SELECT ctid FROM idempotency WHERE conversation_id=$1 LIMIT $2))",
        task
    );
    removed += batches!(
        state,
        connection,
        "idempotency",
        "DELETE FROM idempotency WHERE ctid = ANY(ARRAY(SELECT ctid FROM idempotency WHERE about_conversation=$1 LIMIT $2))",
        task
    );
    removed += batches!(
        state,
        connection,
        "runtime tokens",
        "DELETE FROM runtime_token_reservations WHERE ctid = ANY(ARRAY(SELECT ctid FROM runtime_token_reservations WHERE conversation_id=$1 LIMIT $2))",
        task
    );
    removed += batches!(
        state,
        connection,
        "runtime tokens",
        "DELETE FROM runtime_tokens WHERE ctid = ANY(ARRAY(SELECT ctid FROM runtime_tokens WHERE conversation_id=$1 LIMIT $2))",
        task
    );

    Ok(removed)
}

/// The log of a team the task led, and its channel (GW-I-108: deleting the lead's task deletes the team's messages).
/// The team's row references its channel, not the other way round, so the lead's row going takes the team but would
/// leave its channel and messages: they go here, the messages in batches, then the channel, which takes the team, its
/// members' rows and their places in the log with it. How many messages went.
async fn team_log(state: &State, connection: &mut PgConnection, task: Uuid) -> Result<u64, Failed> {
    let removed = batches!(
        state,
        connection,
        "team log",
        "DELETE FROM channel_messages WHERE ctid = ANY(ARRAY(SELECT m.ctid FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id WHERE t.lead_conversation=$1 LIMIT $2))",
        task
    );
    bounded(
        state,
        sqlx::query!(
            "DELETE FROM channels c USING agent_teams t WHERE t.channel_id = c.id AND c.kind = 'agent_team' AND t.lead_conversation=$1",
            task
        )
        .execute(&mut *connection),
    )
    .await
    .map_err(|error| ("team log", error))?;
    Ok(removed)
}

/// What stays loses the task: its usage counts for its owner, its channel posts lose their author name (the title), and
/// a lease it held is released.
async fn detach_kept(
    state: &State,
    connection: &mut PgConnection,
    task: Uuid,
    owner: Uuid,
) -> Result<(), Failed> {
    loop {
        let changed = bounded(
            state,
            sqlx::query!(
                "UPDATE usage SET user_id=$2 WHERE ctid = ANY(ARRAY(SELECT ctid FROM usage WHERE conversation_id=$1 AND user_id IS NULL LIMIT $3))",
                task,
                owner,
                BATCH
            )
            .execute(&mut *connection),
        )
        .await
        .map_err(|error| ("usage", error))?
        .rows_affected();
        if changed < BATCH.unsigned_abs() {
            break;
        }
    }
    let author = task.to_string();
    batches!(
        state,
        connection,
        "channel posts",
        "UPDATE channel_messages SET author_name=NULL WHERE ctid = ANY(ARRAY(SELECT ctid FROM channel_messages WHERE author_kind='conversation' AND author_id=$1 AND author_name IS NOT NULL LIMIT $2))",
        author.as_str()
    );
    bounded(
        state,
        sqlx::query!(
            "UPDATE workspaces SET lease_conversation_id=NULL, lease_expires_at=NULL WHERE lease_conversation_id=$1",
            task
        )
        .execute(&mut *connection),
    )
    .await
    .map_err(|error| ("lease", error))?;

    Ok(())
}

/// Purges one deleted workspace whose tasks are gone: its files and recorded changes, then its rows, then the row.
async fn purge_workspace(
    state: &State,
    connection: &mut PgConnection,
    workspace: Uuid,
) -> Result<Outcome, Failed> {
    let ready = bounded(
        state,
        sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM workspaces w WHERE w.id=$1 AND w.state='deleted' \
             AND NOT EXISTS(SELECT 1 FROM conversations c WHERE c.workspace_id=w.id)) AS \"ready!\"",
            workspace
        )
        .fetch_one(&mut *connection),
    )
    .await
    .map_err(|error| ("guard", error))?;
    if !ready {
        return Ok(Outcome::Deferred);
    }
    let mut counts = Counts::default();
    state
        .storage
        .remove_workspace_tree(workspace)
        .await
        .map_err(|error| ("files", error))?;
    state
        .storage
        .remove_changes(workspace)
        .await
        .map_err(|error| ("changes", error))?;
    counts.rows += batches!(
        state,
        connection,
        "snapshots",
        "DELETE FROM workspace_snapshots WHERE ctid = ANY(ARRAY(SELECT ctid FROM workspace_snapshots WHERE workspace_id=$1 LIMIT $2))",
        workspace
    );
    counts.rows += batches!(
        state,
        connection,
        "shares",
        "DELETE FROM workspace_shares WHERE ctid = ANY(ARRAY(SELECT ctid FROM workspace_shares WHERE workspace_id=$1 LIMIT $2))",
        workspace
    );
    // Its forks are copies of their own: they stay and stop naming it (with no origin, whether the copy was consistent
    // with it no longer applies: the two are set together).
    batches!(
        state,
        connection,
        "forks",
        "UPDATE workspaces SET forked_from=NULL, fork_consistent=NULL WHERE ctid = ANY(ARRAY(SELECT ctid FROM workspaces WHERE forked_from=$1 LIMIT $2))",
        workspace
    );
    let gone = bounded(
        state,
        sqlx::query!(
            "DELETE FROM workspaces w WHERE w.id=$1 AND w.state='deleted' \
             AND NOT EXISTS(SELECT 1 FROM conversations c WHERE c.workspace_id=w.id)",
            workspace
        )
        .execute(&mut *connection),
    )
    .await
    .map_err(|error| ("row", error))?
    .rows_affected();
    Ok(if gone == 1 {
        Outcome::Done(counts)
    } else {
        Outcome::Deferred
    })
}
