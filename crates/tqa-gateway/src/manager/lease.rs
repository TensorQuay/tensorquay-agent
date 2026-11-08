//! The workspace write lease (task 066b, GW-I-031, answers-066 Q1). The lease is the workspace's one live
//! sandbox: its holder is the conversation whose sandbox record exists. Another conversation of the same workspace
//! waits; an idle (or dead: its lease lapsed without heartbeats) holder hands over at once; a holder with an active
//! run, or a pinned one, is never interrupted.
use crate::{GatewayError, events::RunActivity};
use sqlx::PgConnection;
use uuid::Uuid;

/// Another conversation holds the workspace: who, and whether it may be retired for the waiter now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Block {
    pub holder: Uuid,
    pub handoff: bool,
}

/// A workspace's lease as it stands: held only while its holder has a sandbox record (the one definition of "held",
/// used by [`block`] and by the workspace view, task 076g).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Held {
    pub workspace: Uuid,
    pub holder: Uuid,
    /// No heartbeat within the lease's time to live.
    pub expired: bool,
    pub pinned: bool,
}

/// The held leases among `workspaces`. A lease whose holder has no sandbox record is stale and is not held.
///
/// # Errors
/// A store failure.
pub(crate) async fn held(
    connection: &mut PgConnection,
    workspaces: &[Uuid],
) -> Result<Vec<Held>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT w.id AS workspace, w.lease_conversation_id AS \"holder!\", COALESCE(w.lease_expires_at<=CURRENT_TIMESTAMP, true) AS \"expired!\", COALESCE(h.pinned, false) AS \"pinned!\" FROM workspaces w LEFT JOIN conversations h ON h.id=w.lease_conversation_id WHERE w.id = ANY($1) AND w.lease_conversation_id IS NOT NULL AND EXISTS(SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=w.lease_conversation_id)",
        workspaces
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Held {
            workspace: row.workspace,
            holder: row.holder,
            expired: row.expired,
            pinned: row.pinned,
        })
        .collect())
}

/// Whether `conversation` must wait for its workspace: another conversation holds it (see [`held`]).
///
/// # Errors
/// A store failure.
pub(crate) async fn block(
    connection: &mut PgConnection,
    conversation: Uuid,
    caps: Caps,
) -> Result<Option<Block>, GatewayError> {
    let workspace = sqlx::query_scalar!(
        "SELECT workspace_id FROM conversations WHERE id=$1",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(workspace) = workspace else {
        return Ok(None);
    };
    let Some(held) = held(&mut *connection, &[workspace])
        .await?
        .into_iter()
        .find(|held| held.holder != conversation)
    else {
        return Ok(None);
    };
    Ok(Some(Block {
        holder: held.holder,
        handoff: may_hand_over(
            &mut *connection,
            &held,
            (Some((conversation, caps)), caps.taken),
        )
        .await?,
    }))
}

/// Whether `held`'s holder may be retired now for someone else: its lease lapsed (no heartbeat for its time to live: it
/// is presumed dead), or it has no active run, isn't pinned, and has no turn waiting to go out. A sandbox made for a
/// queued turn has no run until its runtime registers and the turn is delivered; handing it over in that window let
/// two tasks of one workspace take it from each other with neither turn delivered (GW-I-247). For a `waiter`, turns go
/// in the order they were accepted: only a turn being sent, or one accepted no later than the waiter's oldest, keeps
/// the holder busy, so its later turns (a lead's wakes, say) can't starve a task that queued first (GW-I-248).
async fn may_hand_over(
    connection: &mut PgConnection,
    held: &Held,
    (waiter, taken): (Option<(Uuid, Caps)>, std::time::Duration),
) -> Result<bool, GatewayError> {
    if held.expired {
        return Ok(true);
    }
    if held.pinned
        || crate::events::run_activity(&mut *connection, held.holder).await? == RunActivity::Active
    {
        return Ok(false);
    }
    // Only a waiter that could start is a waiter here (GW-I-258; GW-U-172 for a Desktop): one held back by its owner's
    // cap or by the Desktop cap would be handed the workspace and not take it. The holder's own sandbox doesn't count
    // against it, since handing over frees it.
    let waiter = match waiter {
        Some((waiter, caps))
            if admits_waiter(&mut *connection, waiter, Some(held.holder), caps).await? =>
        {
            Some(waiter)
        }
        _ => None,
    };
    // A turn in flight is one still to deliver, or one its runtime took whose run hasn't started (delivered after the
    // holder's latest lifecycle event; a live check before 17.0, GW-U-201): handed over then, the turn is lost with the
    // sandbox.
    let undelivered = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM idempotency h WHERE h.conversation_id=$1 AND (h.execution_status='sending' OR \
         (h.execution_status='delivered' AND h.updated_at > CURRENT_TIMESTAMP - make_interval(secs => $3) \
         AND h.updated_at > COALESCE((SELECT MAX(e.created_at) FROM events e WHERE e.conversation_id=$1 \
         AND e.lifecycle IS NOT NULL), '-infinity'::timestamptz) AND COALESCE((SELECT e.lifecycle FROM events e \
         WHERE e.conversation_id=$1 AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1), '') \
         NOT IN ('run.started', 'run.resumed')) OR \
         (h.execution_status='queued' AND ($2::uuid IS NULL OR h.created_at <= COALESCE((SELECT MIN(w.created_at) \
         FROM idempotency w WHERE w.conversation_id=$2 AND w.execution_status='queued'), 'infinity'::timestamptz))))) \
         AS \"undelivered!\"",
        held.holder,
        waiter,
        taken.as_secs_f64()
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(!undelivered)
}

/// Whether `conversation` has a turn in flight that its sandbox must stay for: one being sent, or one its runtime took
/// whose run hasn't started (delivered after its latest lifecycle event, which isn't a run's start: a run that started
/// is active, which every retirement already waits for). The runtime journals the message when it takes it and starts
/// the run from its journal within seconds; a sandbox retired in between strands the turn, which nothing sends again
/// (a live check before 17.0: a helper handed over 70 ms after its first turn reached it, GW-U-201, GW-I-309). Past
/// `taken` (3 x `io_timeout`) it is no longer in flight: a runtime that took a turn and never started it is
/// `reclaim`'s, never kept forever.
///
/// # Errors
/// A store failure.
pub(crate) async fn turn_in_flight(
    connection: &mut PgConnection,
    conversation: Uuid,
    taken: std::time::Duration,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM idempotency i WHERE i.conversation_id=$1 AND (i.execution_status='sending' OR \
         (i.execution_status='delivered' AND i.updated_at > CURRENT_TIMESTAMP - make_interval(secs => $2) \
         AND i.updated_at > COALESCE((SELECT MAX(e.created_at) FROM events e WHERE e.conversation_id=$1 \
         AND e.lifecycle IS NOT NULL), '-infinity'::timestamptz) AND COALESCE((SELECT e.lifecycle FROM events e \
         WHERE e.conversation_id=$1 AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1), '') \
         NOT IN ('run.started', 'run.resumed')))) AS \"in_flight!\"",
        conversation,
        taken.as_secs_f64()
    )
    .fetch_one(connection)
    .await?)
}

/// The tasks whose environments hold `workspace` (any of its tasks with a sandbox), when every one may hand over now,
/// as for a new task's handover; `None` while one may not: a run going, or a pinned one (GW-I-199).
///
/// # Errors
/// A store failure.
pub(crate) async fn retirable(
    connection: &mut PgConnection,
    workspace: Uuid,
    taken: std::time::Duration,
) -> Result<Option<Vec<Uuid>>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT c.id, c.pinned, (w.lease_conversation_id IS NOT DISTINCT FROM c.id AND \
         COALESCE(w.lease_expires_at<=CURRENT_TIMESTAMP, true)) AS \"expired!\" FROM sandbox_instances s \
         JOIN conversations c ON c.id=s.conversation_id JOIN workspaces w ON w.id=c.workspace_id \
         WHERE c.workspace_id=$1",
        workspace
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut holders = Vec::with_capacity(rows.len());
    for row in rows {
        let held = Held {
            workspace,
            holder: row.id,
            expired: row.expired,
            pinned: row.pinned,
        };
        if !may_hand_over(&mut *connection, &held, (None, taken)).await? {
            return Ok(None);
        }
        holders.push(row.id);
    }
    Ok(Some(holders))
}

/// The caps a waiter must fit under to be one, from the limits in force: the per-user cap and the Desktop cap
/// (`desktop.max_sessions`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Caps {
    pub(crate) per_user: i64,
    pub(crate) desktops: i64,
    /// How long a turn its runtime took counts as in flight with no run started (`turn_in_flight`).
    pub(crate) taken: std::time::Duration,
}

impl Caps {
    /// The caps of `limits`; a taken turn is in flight for 3 x `io_timeout`, the bound after which `reclaim` takes a
    /// turn no run confirmed as never taken.
    pub(crate) fn of(
        limits: &crate::settings::limits::Limits,
        io_timeout: std::time::Duration,
    ) -> Self {
        Self {
            per_user: i64::from(limits.per_user),
            desktops: i64::from(limits.desktop_max_sessions),
            taken: io_timeout * 3,
        }
    }
}

/// Whether `waiter` could start a sandbox now if `holder`'s were gone (handing over frees it): its owner is under the
/// per-user cap, and, if its environment declares `desktop`, the live Desktops are under `desktop.max_sessions`. The
/// one rule for "a waiter" in the hand-over (`may_hand_over`), a holder's yield (`yields_to_waiter`) and a start's
/// turn order (`behind_older_waiter`, admission's `behind`): a Desktop waiter that can't start isn't one, or the
/// workspace would sit empty while it waits for a Desktop (gap C, GW-U-172).
///
/// # Errors
/// A store failure.
pub(crate) async fn admits_waiter(
    connection: &mut PgConnection,
    waiter: Uuid,
    holder: Option<Uuid>,
    caps: Caps,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT (SELECT count(*) FROM sandbox_instances s JOIN conversations c ON c.id = s.conversation_id \
         WHERE c.owner_id = (SELECT owner_id FROM conversations WHERE id = $1) AND s.conversation_id IS DISTINCT FROM $2) < $3 \
         AND (NOT EXISTS(SELECT 1 FROM conversations c JOIN templates t ON t.name = c.template \
              WHERE c.id = $1 AND 'desktop' = ANY(t.capabilities)) \
              OR (SELECT count(*) FROM sandbox_instances s JOIN conversations c ON c.id = s.conversation_id \
                  JOIN templates t ON t.name = c.template \
                  WHERE 'desktop' = ANY(t.capabilities) AND s.conversation_id IS DISTINCT FROM $2) < $4) AS \"admits!\"",
        waiter,
        holder,
        caps.per_user,
        caps.desktops
    )
    .fetch_one(&mut *connection)
    .await?)
}

/// Whether any of `candidates` passes [`admits_waiter`].
async fn any_admitted(
    connection: &mut PgConnection,
    candidates: Vec<Uuid>,
    holder: Option<Uuid>,
    caps: Caps,
) -> Result<bool, GatewayError> {
    for candidate in candidates {
        if admits_waiter(&mut *connection, candidate, holder, caps).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `holder`, with no run going, should let a task that queued first on its workspace go before its own message
/// turns: another task of the workspace, with no sandbox yet, has a turn accepted before `holder`'s oldest queued one
/// on `route`, and whose owner's cap admits it (GW-I-258). Its worker then holds those turns and the admission pass
/// hands the workspace over (GW-I-248).
///
/// # Errors
/// A store failure.
pub(crate) async fn yields_to_waiter(
    connection: &mut PgConnection,
    holder: Uuid,
    route: &str,
    caps: Caps,
) -> Result<bool, GatewayError> {
    let candidates = sqlx::query_scalar!(
        "SELECT DISTINCT o.id FROM conversations o JOIN idempotency w ON w.conversation_id=o.id \
         WHERE o.workspace_id=(SELECT workspace_id FROM conversations WHERE id=$1) AND o.id<>$1 AND o.state='active' \
         AND w.execution_status='queued' AND NOT EXISTS(SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=o.id) \
         AND w.created_at < (SELECT MIN(h.created_at) FROM idempotency h WHERE h.conversation_id=$1 \
         AND h.execution_status='queued' AND h.route=$2)",
        holder,
        route
    )
    .fetch_all(&mut *connection)
    .await?;
    any_admitted(connection, candidates, Some(holder), caps).await
}

/// Whether `conversation`, with no sandbox, must let a task that queued first on its workspace go before it: another
/// task of the workspace, with no sandbox either, has a turn accepted before `conversation`'s oldest queued one, and
/// its owner's cap admits it. The admission pass's `behind`, for a worker's own start: once the hand-over retired the
/// holder no one holds the lease, and the holder's worker, with its later turn queued, took the workspace straight back
/// and was handed over again, in a loop (GW-I-251 after a restart). A start with no queued turn of its own (Files, a
/// cancel) is behind any waiter: it would be retired as an idle holder at the next pass (GW-U-170).
///
/// # Errors
/// A store failure.
pub(crate) async fn behind_older_waiter(
    connection: &mut PgConnection,
    conversation: Uuid,
    caps: Caps,
) -> Result<bool, GatewayError> {
    let candidates = sqlx::query_scalar!(
        "SELECT DISTINCT o.id FROM conversations o JOIN idempotency w ON w.conversation_id=o.id \
         WHERE o.workspace_id=(SELECT workspace_id FROM conversations WHERE id=$1) AND o.id<>$1 AND o.state='active' \
         AND w.execution_status='queued' AND NOT EXISTS(SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=o.id) \
         AND w.created_at < COALESCE((SELECT MIN(h.created_at) FROM idempotency h WHERE h.conversation_id=$1 \
         AND h.execution_status='queued'), 'infinity'::timestamptz)",
        conversation
    )
    .fetch_all(&mut *connection)
    .await?;
    any_admitted(connection, candidates, None, caps).await
}

/// Whether another active task on `conversation`'s workspace has a turn queued. A retirement a restart interrupted then
/// was a hand-over's, the workspace passing in acceptance order (GW-I-248), and `conversation`'s queued turns were
/// waiting their turn behind that task, not lost with the sandbox (GW-I-263).
///
/// # Errors
/// A store failure.
pub(crate) async fn others_queued(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM conversations o JOIN idempotency w ON w.conversation_id=o.id \
         WHERE o.workspace_id=(SELECT workspace_id FROM conversations WHERE id=$1) AND o.id<>$1 AND o.state='active' \
         AND w.execution_status='queued') AS \"queued!\"",
        conversation
    )
    .fetch_one(&mut *connection)
    .await?)
}

/// Takes (or keeps) the lease for `conversation`, whose sandbox record is being created or already exists.
/// A holder's own lease is only ever extended here, never shortened: a new sandbox's lease covers its whole start
/// (GW-I-247), and a later admission of the same sandbox mid-start must not cut it back to `lease_ttl`.
///
/// # Errors
/// A store failure.
pub(crate) async fn take(
    connection: &mut PgConnection,
    conversation: Uuid,
    ttl: std::time::Duration,
) -> Result<(), GatewayError> {
    let seconds = ttl.as_secs_f64();
    sqlx::query!(
        "UPDATE workspaces w SET lease_conversation_id=$1, lease_expires_at=CASE WHEN w.lease_conversation_id=$1 AND w.lease_expires_at>CURRENT_TIMESTAMP+make_interval(secs=>$2) THEN w.lease_expires_at ELSE CURRENT_TIMESTAMP+make_interval(secs=>$2) END, updated_at=CURRENT_TIMESTAMP FROM conversations c WHERE c.id=$1 AND w.id=c.workspace_id",
        conversation,
        seconds
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Renews the holder's lease on a heartbeat of its run.
///
/// # Errors
/// A store failure.
pub(crate) async fn renew(
    pool: &sqlx::PgPool,
    conversation: Uuid,
    ttl: std::time::Duration,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE workspaces SET lease_expires_at=CURRENT_TIMESTAMP+make_interval(secs=>$2) WHERE lease_conversation_id=$1",
        conversation,
        ttl.as_secs_f64()
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "lease_tests.rs"]
mod tests;
