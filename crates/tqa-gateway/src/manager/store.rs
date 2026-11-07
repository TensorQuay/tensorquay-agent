use super::SandboxManager;
use crate::GatewayError;
use sqlx::{Postgres, Transaction};
use tqa_types::ConversationId;
use uuid::Uuid;

/// What a new sandbox's lease adds to its start's own bound (create budget and readiness wait): the admission and the
/// first heartbeat's own delays.
const START_LEASE_MARGIN: std::time::Duration = std::time::Duration::from_secs(30);

/// The outcome of admitting accepted work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Admission {
    /// A sandbox exists or may be created now.
    Admitted,
    /// The sandbox is retiring; its worker starts a new one for the queued work when it is gone.
    Retiring,
    /// No slot is free (or the owner is at the per-user cap): the work waits in the fair queue.
    Waiting,
    /// Another task's sandbox holds the workspace's write lease (066b); `handoff` says it may be retired now.
    WaitingForWorkspace(super::lease::Block),
}

impl SandboxManager {
    pub(super) async fn reset_record(&self, id: Uuid) -> Result<(), GatewayError> {
        self.bounded(async {
            sqlx::query!("UPDATE sandbox_instances SET sandbox_id=NULL,phase='starting',updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1", id)
                .execute(&self.services.pool).await?;
            Ok(())
        }).await
    }

    /// Admits accepted work for a conversation. While its sandbox is retiring the work is accepted and queued:
    /// the row lock keeps the retiring worker from deleting the record before this transaction commits, and that
    /// worker schedules a new sandbox for queued work once retirement ends.
    ///
    /// With `Admitted` for a new sandbox comes its worker's slot, taken before the record (`None` for one that exists):
    /// hold it until the worker is scheduled. A full slot table leaves the work `Waiting`, unrecorded, for the admission
    /// pass, as for a full host. A record without a slot was a stored message answered 429 that no worker or admission
    /// pass would serve (PERF-020 on c16).
    ///
    /// # Errors
    /// Fails only on storage errors: a full host queues the work (`Waiting`) instead of refusing it.
    pub(crate) async fn admit(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        id: Uuid,
    ) -> Result<(Admission, Option<super::SlotHold>), GatewayError> {
        // Serialize the small capacity decision across action keys and process restart boundaries.
        crate::db::queue(transaction, crate::db::Queue::Key(74_271_012)).await?;
        let phase = sqlx::query_scalar!(
            "SELECT phase FROM sandbox_instances WHERE conversation_id=$1 FOR UPDATE",
            id
        )
        .fetch_optional(&mut **transaction)
        .await?;
        if phase.as_deref() == Some("destroying") {
            return Ok((Admission::Retiring, None));
        }
        let mut held = None;
        if phase.is_none() {
            // One read-write mount per workspace: a held lease comes before any slot (066b).
            let caps = super::lease::Caps::of(
                &self.services.settings.current().limits,
                self.config.io_timeout(),
            );
            if let Some(block) = super::lease::block(transaction, id, caps).await? {
                return Ok((Admission::WaitingForWorkspace(block), None));
            }
            // Turns on one workspace go in acceptance order here too, not only in the admission pass (GW-I-251).
            if super::lease::behind_older_waiter(transaction, id, caps).await? {
                return Ok((Admission::Waiting, None));
            }
            let count = sqlx::query_scalar!("SELECT count(*) FROM sandbox_instances")
                .fetch_one(&mut **transaction)
                .await?
                .unwrap_or(0);
            let owned = sqlx::query_scalar!("SELECT count(*) FROM sandbox_instances s JOIN conversations c ON c.id=s.conversation_id WHERE c.owner_id=(SELECT owner_id FROM conversations WHERE id=$1)", id)
                .fetch_one(&mut **transaction)
                .await?
                .unwrap_or(0);
            // A desktop environment also waits for a free Desktop (`desktop.max_sessions`, ADR-0039).
            let desktop = sqlx::query!("SELECT EXISTS(SELECT 1 FROM conversations c JOIN templates t ON t.name=c.template WHERE c.id=$1 AND 'desktop'=ANY(t.capabilities)) AS \"desktop!\", (SELECT count(*) FROM sandbox_instances s JOIN conversations c ON c.id=s.conversation_id JOIN templates t ON t.name=c.template WHERE 'desktop'=ANY(t.capabilities)) AS \"live!\"", id)
                .fetch_one(&mut **transaction)
                .await?;
            let limits = self.services.settings.current().limits;
            if count >= i64::from(limits.slots)
                || owned >= i64::from(limits.per_user)
                || (desktop.desktop && desktop.live >= i64::from(limits.desktop_max_sessions))
            {
                return Ok((Admission::Waiting, None));
            }
            let Some(slot) = self.hold_slot(id)? else {
                return Ok((Admission::Waiting, None));
            };
            held = Some(slot);
            // The sandbox keeps the timers it was admitted with; a later override applies to the next one (ADR-0028).
            sqlx::query!(
                "INSERT INTO sandbox_instances (conversation_id,idle_seconds,max_lifetime_seconds) VALUES ($1,$2,$3)",
                id,
                i32::try_from(limits.idle_seconds).map_err(|_| GatewayError::StoreUnavailable)?,
                i32::try_from(limits.max_lifetime_seconds).map_err(|_| GatewayError::StoreUnavailable)?
            )
            .execute(&mut **transaction)
            .await?;
        }
        sqlx::query!("UPDATE sandbox_instances SET last_action_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1",id).execute(&mut **transaction).await?;
        // A new sandbox's lease covers its whole start: nothing renews it until the runtime's first heartbeat, and a
        // start may take its create budget and readiness wait (GW-I-247, a slow start isn't handed over mid-start).
        let ttl = if phase.is_none() {
            self.config.workspaces.lease_ttl.max(
                self.config.sandbox.create_budget()
                    + self.config.sandbox.ready()
                    + START_LEASE_MARGIN,
            )
        } else {
            self.config.workspaces.lease_ttl
        };
        super::lease::take(transaction, id, ttl).await?;
        Ok((Admission::Admitted, held))
    }

    pub(super) async fn admit_conversation(&self, id: &ConversationId) -> Result<(), GatewayError> {
        self.admit_in(id, false).await.map(|_| ())
    }

    /// Admits a conversation from the slot queue only while it still has queued work, checked under the admission
    /// lock: false when a cancel that committed first withdrew it (GW-I-173). A cancel after this finds the slot
    /// granted.
    pub(super) async fn admit_waiting_work(
        &self,
        id: &ConversationId,
    ) -> Result<bool, GatewayError> {
        self.admit_in(id, true).await
    }

    async fn admit_in(&self, id: &ConversationId, needs_work: bool) -> Result<bool, GatewayError> {
        let id = crate::conversations::uuid(id)?;
        self.bounded(async {
            let mut tx = self.services.pool.begin().await?;
            let state =
                sqlx::query_scalar!("SELECT state FROM conversations WHERE id=$1 FOR SHARE", id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or(GatewayError::NotFound)?;
            if state != "active" {
                return Err(GatewayError::Conflict("conversation_deleted"));
            }
            if needs_work && !Self::still_waiting(&mut tx, id).await? {
                return Ok(false);
            }
            // A new instance cannot start until the retiring owner has released the record, or while the work
            // waits for a slot; the admission task starts it later.
            if self.admit(&mut tx, id).await?.0 != Admission::Admitted {
                return Err(GatewayError::Busy);
            }
            tx.commit().await?;
            Ok(true)
        })
        .await
    }

    /// Whether the conversation still has queued work, under the admission lock: the conversation's lock, then the
    /// admission lock, as `mark_closing` takes them (`admit` takes the admission lock again, which a transaction
    /// already holding it passes).
    async fn still_waiting(
        tx: &mut Transaction<'_, Postgres>,
        id: Uuid,
    ) -> Result<bool, GatewayError> {
        crate::db::queue(tx, crate::db::Queue::Key(74_271_012)).await?;
        Ok(sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM idempotency WHERE conversation_id=$1 AND execution_status='queued')",
            id
        )
        .fetch_one(&mut **tx)
        .await?
        .unwrap_or(false))
    }

    pub(super) async fn mark_closing(
        &self,
        id: &ConversationId,
        idle: Option<&super::idle::Idle>,
    ) -> Result<bool, GatewayError> {
        let id = crate::conversations::uuid(id)?;
        let action = idle.and_then(|idle| idle.action);
        let sequence = idle
            .map(|idle| crate::wire::db_sequence(idle.sequence))
            .transpose()?;
        self.bounded(async {
            let mut tx=self.services.pool.begin().await?;
            // Match append's conversation lock before the admission lock. A new event or action
            // invalidates this idle observation before retirement can close admission.
            sqlx::query!("SELECT id FROM conversations WHERE id=$1 FOR UPDATE", id).fetch_one(&mut *tx).await?;
            crate::db::queue(&mut tx, crate::db::Queue::Key(74_271_012)).await?;
            let row=sqlx::query!("UPDATE sandbox_instances SET phase='destroying',updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1 AND ($2::timestamptz IS NULL OR (last_action_at=$2 AND $3=COALESCE((SELECT MAX(seq) FROM events WHERE conversation_id=$1),0) AND NOT EXISTS (SELECT 1 FROM idempotency WHERE conversation_id=$1 AND execution_status='queued'))) RETURNING conversation_id",id,action,sequence).fetch_optional(&mut *tx).await?;
            tx.commit().await?;
            Ok(row.is_some())
        }).await
    }

    pub(super) async fn forget(&self, id: &ConversationId) -> Result<(), GatewayError> {
        let id = crate::conversations::uuid(id)?;
        self.bounded(async {
            let mut transaction = self.services.pool.begin().await?;
            sqlx::query!("DELETE FROM sandbox_instances WHERE conversation_id=$1", id)
                .execute(&mut *transaction)
                .await?;
            // The workspace's one read-write mount is gone: its lease is free (066b).
            sqlx::query!("UPDATE workspaces SET lease_conversation_id=NULL, lease_expires_at=NULL WHERE lease_conversation_id=$1", id)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            Ok(())
        })
        .await?;
        // No sandbox has the workspace mounted now: measure it for the quota (066c).
        crate::workspaces::measure_later(
            &self.services.pool,
            &self.services.storage,
            &self.services.tasks,
            id,
        );
        // A slot is free: waiting work may start now.
        self.nudge_admission();
        Ok(())
    }
}
