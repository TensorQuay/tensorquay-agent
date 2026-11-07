//! The worker's side of "send after this" (GW-I-129/130, GW-U-078): messages wait while the task's run is in progress
//! and go out as one turn when it ends; a run that fails or is stopped drops them. A turn the runtime refuses as busy
//! (it journals a run's end just before its run task has finished, or a message arrives before the run it started is
//! journaled) is put back and waits for the run's end and a short backoff, never retried in a tight loop.
use super::{SandboxManager, Slot};
use crate::{GatewayError, actions::held};
use std::{
    sync::{Mutex, PoisonError, atomic::Ordering},
    time::Duration,
};
use tokio::time::Instant;
use tqa_sandbox::ReadinessSource as _;
use tqa_types::{ClientFrame, ConversationId};

/// The first wait after a busy refusal, and the longest.
const FIRST_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(4);

/// A turn handed to the runtime and not yet seen to start a run.
struct Turn {
    /// Its messages' rows, to put back on a busy refusal.
    rows: Vec<(String, uuid::Uuid)>,
    /// The latest lifecycle event's sequence when it went out: a later one means the runtime took it.
    lifecycle: i64,
    /// When it went out: past the bound without a new lifecycle event, messages stop waiting for it.
    at: Instant,
    /// Its host keys' delivery, whose placeholders go if the runtime doesn't take it (ADR-0037 §3).
    delivery: Option<uuid::Uuid>,
}

/// One conversation's turn in flight and its busy backoff.
#[derive(Default)]
pub(super) struct Turns {
    state: Mutex<TurnState>,
}

#[derive(Default)]
struct TurnState {
    turn: Option<Turn>,
    /// Messages wait until then after a busy refusal.
    not_before: Option<Instant>,
    backoff: Option<Duration>,
    /// Since when the link has been ready without a break; `None` while it isn't.
    ready_since: Option<Instant>,
    /// When unconfirmed messages were last looked for.
    checked: Option<Instant>,
}

impl Turns {
    fn lock(&self) -> std::sync::MutexGuard<'_, TurnState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// No turn in flight: nothing sent that a busy refusal would put back.
    pub(super) fn idle(&self) -> bool {
        self.lock().turn.is_none()
    }

    /// A turn in flight with `rows`, as a delivery records it (for the slot table's tests).
    #[cfg(test)]
    pub(super) fn hold_for_test(&self, rows: Vec<(String, uuid::Uuid)>) {
        self.lock().turn = Some(Turn {
            rows,
            lifecycle: 0,
            at: Instant::now(),
            delivery: None,
        });
    }
}

/// A queued message's text and attachments, from its stored frame.
fn part(bytes: Option<&[u8]>) -> Result<held::Part, GatewayError> {
    let bytes = bytes.ok_or(GatewayError::StoreUnavailable)?;
    match ClientFrame::parse(bytes).map_err(|_| GatewayError::StoreUnavailable)? {
        ClientFrame::UserMessage {
            text,
            attachments,
            lead_brief,
            team_wake,
            ..
        } => Ok(held::Part {
            text,
            attachments,
            lead_brief,
            team_wake,
        }),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

/// A message claimed for a turn.
struct Claimed {
    key: String,
    user: uuid::Uuid,
    ordinal: i64,
    held: bool,
}

/// The latest run lifecycle event: its sequence, its kind and when the gateway stored it.
struct Latest {
    seq: i64,
    kind: String,
    at: time::OffsetDateTime,
}

impl SandboxManager {
    /// The link saw the runtime refuse a message as busy: `true` when it was this worker's turn, which the next pass
    /// puts back; `false` for anything else (the refusal then reaches clients as before).
    pub(crate) fn busy_refused(&self, id: &ConversationId) -> bool {
        let Ok(slots) = self.slots.lock() else {
            return false;
        };
        let Some(slot) = slots.get(id) else {
            return false;
        };
        if slot.turns.lock().turn.is_none() {
            return false;
        }
        slot.busy.store(true, Ordering::Release);
        slot.notify.notify_one();
        true
    }

    /// Delivers what waits: every queued action in order, except that messages wait while the run is in progress (or
    /// a turn just sent hasn't started one yet, or a busy backoff runs), and then go out together as one turn.
    /// Returns whether anything was delivered.
    pub(super) async fn deliver_held(
        &self,
        id: &ConversationId,
        slot: &Slot,
    ) -> Result<bool, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        let messages = held::route(id);
        self.put_back(id, slot).await?;
        self.reclaim(id, slot).await?;
        // The common pass has nothing queued: one indexed lookup, as before holding existed.
        let waiting = self.bounded(async {
            Ok(sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM idempotency WHERE conversation_id=$1 AND execution_status='queued')", conversation)
                .fetch_one(&self.services.pool).await?.unwrap_or(false))
        }).await?;
        if !waiting {
            return Ok(false);
        }
        let latest = self.latest_lifecycle(conversation).await?;
        if let Some(latest) = &latest {
            self.drop_after(id, conversation, &messages, latest).await?;
        }
        let mut hold = latest
            .as_ref()
            .is_some_and(|latest| matches!(latest.kind.as_str(), "run.started" | "run.resumed"))
            || self.backing_off(slot, latest.as_ref())
            // Take over (ADR-0039 section 6): no queued turn starts until the hand-back, which wakes them.
            || crate::screen::taken_over(id);
        if hold {
            self.mark_waiting(id, conversation, &messages).await?;
        } else if self
            .bounded(async {
                let mut connection = self.services.pool.acquire().await?;
                let caps = super::lease::Caps::of(
                    &self.services.settings.current().limits,
                    self.config.io_timeout(),
                );
                super::lease::yields_to_waiter(&mut connection, conversation, &messages, caps).await
            })
            .await?
        {
            // A task that queued first waits on this workspace: this one's turns wait their turn, and the admission
            // pass hands the workspace over (GW-I-248). Other queued actions still go.
            hold = true;
            self.nudge_admission();
        }
        let mut delivered = false;
        loop {
            let queued = self.bounded(async {
                Ok(sqlx::query!("SELECT key,user_id,route,queued_frame FROM idempotency WHERE conversation_id=$1 AND execution_status='queued' AND (NOT $2 OR route<>$3) ORDER BY ordinal LIMIT 1", conversation, hold, messages)
                    .fetch_optional(&self.services.pool).await?)
            }).await?;
            let Some(queued) = queued else {
                return Ok(delivered);
            };
            if queued.route == messages {
                let lifecycle = latest.as_ref().map_or(0, |latest| latest.seq);
                // Boxed: minting the turn's host keys enlarges its future (clippy::large_futures).
                Box::pin(self.deliver_turn(id, slot, lifecycle)).await?;
                hold = true;
            } else {
                let bytes = queued.queued_frame.ok_or(GatewayError::StoreUnavailable)?;
                let frame =
                    ClientFrame::parse(&bytes).map_err(|_| GatewayError::StoreUnavailable)?;
                self.deliver_one(id, frame, (&queued.key, queued.user_id))
                    .await?;
            }
            delivered = true;
        }
    }

    async fn deliver_one(
        &self,
        id: &ConversationId,
        frame: ClientFrame,
        (key, user): (&str, uuid::Uuid),
    ) -> Result<(), GatewayError> {
        crate::files::changes::before_delivery(&self.services, &self.config, id, &frame).await;
        self.bounded(self.services.links.deliver(id, frame)).await?;
        self.bounded(async {
            sqlx::query!("UPDATE idempotency SET execution_status='delivered',updated_at=CURRENT_TIMESTAMP WHERE key=$1 AND user_id=$2 AND execution_status='queued'", key, user)
                .execute(&self.services.pool).await?;
            Ok(())
        })
        .await
    }

    /// Every waiting message that fits, as one user turn, in order; the rest wait for the next turn. The messages are
    /// claimed (`sending`) in the transaction that decides they go out, so a cancel either wins before it (they never
    /// go out) or finds them sent (409): never "cancelled" for a message the agent got.
    async fn deliver_turn(
        &self,
        id: &ConversationId,
        slot: &Slot,
        lifecycle: i64,
    ) -> Result<(), GatewayError> {
        let Some((rows, frame)) = self.claim_turn(id).await? else {
            return Ok(());
        };
        let keys: Vec<String> = rows.iter().map(|row| row.key.clone()).collect();
        let users: Vec<uuid::Uuid> = rows.iter().map(|row| row.user).collect();
        let (frame, delivery) = self.with_host_keys(id, frame, &users).await;
        // The turn is in flight before the runtime can answer it: a busy refusal right away still finds it.
        slot.turns.lock().turn = Some(Turn {
            rows: keys.iter().cloned().zip(users.iter().copied()).collect(),
            lifecycle,
            at: Instant::now(),
            delivery,
        });
        crate::files::changes::before_delivery(&self.services, &self.config, id, &frame).await;
        let delivered = self.bounded(self.services.links.deliver(id, frame)).await;
        let status = if delivered.is_ok() {
            "delivered"
        } else {
            "queued"
        };
        if delivered.is_err() {
            slot.turns.lock().turn = None;
            self.drop_host_keys(delivery).await;
        }
        self.bounded(async {
            sqlx::query!("UPDATE idempotency SET execution_status=$3,updated_at=CURRENT_TIMESTAMP WHERE (key,user_id) IN (SELECT * FROM unnest($1::text[],$2::uuid[])) AND execution_status='sending'", &keys, &users, status)
                .execute(&self.services.pool).await?;
            Ok(())
        }).await?;
        delivered?;
        // Only a turn that waited is news to the clients (an ordinary message to an idle task isn't).
        if rows.iter().any(|row| row.held) {
            let sent = rows.iter().map(|row| row.ordinal.to_string()).collect();
            self.notify_queue(id, Some(sent)).await;
        }
        Ok(())
    }

    /// The turn with its run's host keys (ADR-0037 §2, §3), minted for the linked sandbox, and the delivery they belong
    /// to. Keys are a convenience, never a condition: when they can't be minted the turn goes without them.
    async fn with_host_keys(
        &self,
        id: &ConversationId,
        mut frame: ClientFrame,
        senders: &[uuid::Uuid],
    ) -> (ClientFrame, Option<uuid::Uuid>) {
        let ClientFrame::UserMessage { host_keys, .. } = &mut frame else {
            return (frame, None);
        };
        let (Ok(conversation), Some((sandbox, revision))) = (
            crate::conversations::uuid(id),
            self.services.links.sandbox(id),
        ) else {
            return (frame, None);
        };
        let settings = self.services.settings.current();
        if revision < tqa_types::HOST_KEYS_REVISION {
            self.too_old_for_keys(id, senders, &settings).await;
            return (frame, None);
        }
        let delivery = uuid::Uuid::now_v7();
        let minted = self
            .bounded(crate::host_keys::for_turn(
                &self.services.pool,
                (conversation, sandbox.as_str()),
                senders,
                (&settings.egress, self.config.egress.disabled),
                delivery,
            ))
            .await;
        match minted {
            Ok(grants) => *host_keys = grants,
            Err(error) => {
                tracing::warn!(conversation_id = %id, error = %error, "the turn goes without its host keys");
            }
        }
        (frame, Some(delivery))
    }

    /// A runtime older than revision 11 can't take keys (guardrails §8 keeps it linking): when the sender has keys the
    /// task could use, the task's streams say so in plain words, and the run goes ahead without them.
    async fn too_old_for_keys(
        &self,
        id: &ConversationId,
        senders: &[uuid::Uuid],
        settings: &crate::settings::Snapshot,
    ) {
        let keyed = self
            .bounded(crate::host_keys::would_have_keys(
                &self.services.pool,
                senders,
                (&settings.egress, self.config.egress.disabled),
            ))
            .await;
        if !matches!(keyed, Ok(true)) {
            return;
        }
        let notice = tqa_types::ServerFrame::Error {
            seq: None,
            code: tqa_types::ErrorCode::HostKeysUnsupported,
            message: "This environment is too old to use your keys; rebuild it.".into(),
            retryable: false,
            target: None,
        };
        if let Err(error) = self.services.events.control(id, notice) {
            tracing::debug!(
                code = error.code(),
                "the host keys notice could not be published"
            );
        }
    }

    /// Drops the placeholders of a turn the runtime didn't take; a failure leaves them to the next turn's minting.
    async fn drop_host_keys(&self, delivery: Option<uuid::Uuid>) {
        let Some(delivery) = delivery else {
            return;
        };
        let dropped = self
            .bounded(async {
                let mut connection = self.services.pool.acquire().await?;
                crate::host_keys::drop_delivery(&mut connection, delivery).await
            })
            .await;
        if dropped.is_err() {
            tracing::warn!("an undelivered turn's placeholders stay until the next turn");
        }
    }

    /// Claims the waiting messages that fit one turn (locking them against a concurrent cancel) and builds the turn.
    async fn claim_turn(
        &self,
        id: &ConversationId,
    ) -> Result<Option<(Vec<Claimed>, ClientFrame)>, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        let messages = held::route(id);
        self.bounded(async {
            let mut transaction = self.services.pool.begin().await?;
            let rows = sqlx::query!("SELECT key,user_id,ordinal AS \"ordinal!\",held,queued_frame FROM idempotency WHERE conversation_id=$1 AND route=$2 AND execution_status='queued' ORDER BY ordinal FOR UPDATE", conversation, messages)
                .fetch_all(&mut *transaction).await?;
            let parts = rows
                .iter()
                .map(|row| part(row.queued_frame.as_deref()))
                .collect::<Result<Vec<_>, _>>()?;
            if parts.is_empty() {
                return Ok(None);
            }
            let (taken, frame) = held::join(&parts);
            let claimed: Vec<Claimed> = rows
                .into_iter()
                .take(taken)
                .map(|row| Claimed { key: row.key, user: row.user_id, ordinal: row.ordinal, held: row.held })
                .collect();
            let keys: Vec<String> = claimed.iter().map(|row| row.key.clone()).collect();
            let users: Vec<uuid::Uuid> = claimed.iter().map(|row| row.user).collect();
            // Sent as a run, a message to a leading lead is no longer the lead's to read: its team copy is never read (GW-I-307).
            sqlx::query!("UPDATE idempotency SET execution_status='sending',team_copy_channel=NULL,team_copy_seq=NULL,updated_at=CURRENT_TIMESTAMP WHERE (key,user_id) IN (SELECT * FROM unnest($1::text[],$2::uuid[]))", &keys, &users)
                .execute(&mut *transaction).await?;
            transaction.commit().await?;
            Ok(Some((claimed, frame)))
        })
        .await
    }

    /// Messages that wait are held ones, whenever they arrived (a message right after another, before its run is
    /// journaled, waits too): listed, told, and dropped with a run that fails.
    async fn mark_waiting(
        &self,
        id: &ConversationId,
        conversation: uuid::Uuid,
        messages: &str,
    ) -> Result<(), GatewayError> {
        let marked = self.bounded(async {
            Ok(sqlx::query!("UPDATE idempotency SET held=true WHERE conversation_id=$1 AND route=$2 AND execution_status='queued' AND NOT held", conversation, messages)
                .execute(&self.services.pool).await?.rows_affected())
        }).await?;
        if marked > 0 {
            self.notify_queue(id, None).await;
        }
        Ok(())
    }

    /// Whether messages wait out a busy backoff. The turn in flight is forgotten once a later lifecycle event shows the
    /// runtime took it, or past the bound; it's kept only so that a busy refusal finds it. A message right after a turn
    /// isn't held for it: if it comes too early, the runtime refuses it as busy and it is put back.
    fn backing_off(&self, slot: &Slot, latest: Option<&Latest>) -> bool {
        let mut state = slot.turns.lock();
        let now = Instant::now();
        let ended = state.turn.as_ref().map(|turn| {
            let started = latest.is_some_and(|latest| latest.seq > turn.lifecycle);
            let expired = now.saturating_duration_since(turn.at) > self.config.io_timeout() * 3;
            (started, expired)
        });
        if let Some((started, expired)) = ended {
            if started {
                state.backoff = None;
            }
            if started || expired {
                state.turn = None;
            }
        }
        state.not_before.is_some_and(|until| now < until)
    }

    /// A busy refusal puts the turn in flight back, to wait for the run's end and a backoff that doubles up to 4 s.
    async fn put_back(&self, id: &ConversationId, slot: &Slot) -> Result<(), GatewayError> {
        if !slot.busy.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let turn = {
            let mut state = slot.turns.lock();
            let wait = state
                .backoff
                .map_or(FIRST_BACKOFF, |last| (last * 2).min(MAX_BACKOFF));
            state.backoff = Some(wait);
            state.not_before = Some(Instant::now() + wait);
            state.turn.take()
        };
        let Some(turn) = turn else {
            return Ok(());
        };
        self.drop_host_keys(turn.delivery).await;
        let (keys, users): (Vec<String>, Vec<uuid::Uuid>) = turn.rows.into_iter().unzip();
        self.bounded(async {
            sqlx::query!("UPDATE idempotency SET execution_status='queued',held=true,updated_at=CURRENT_TIMESTAMP WHERE (key,user_id) IN (SELECT * FROM unnest($1::text[],$2::uuid[])) AND execution_status IN ('sending','delivered')", &keys, &users)
                .execute(&self.services.pool).await?;
            Ok(())
        }).await?;
        tracing::info!(conversation_id = %id, "the runtime was busy; the messages wait for its run to end");
        self.notify_queue(id, None).await;
        Ok(())
    }

    /// Messages sent but never confirmed by a lifecycle event after them go back to waiting: the gateway restarted
    /// before the runtime started a run on them, or the runtime's busy refusal went with a dead link, so the turn in
    /// flight (kept in memory) is gone. A runtime that did take them starts a run within seconds, and a link that was
    /// down replays that `run.started` from the runtime's log first; so this waits until the link has been ready, and
    /// the messages sent, for three `io_timeout`s, with no run in progress. Looked for at most once per that bound.
    async fn reclaim(&self, id: &ConversationId, slot: &Slot) -> Result<(), GatewayError> {
        let bound = self.config.io_timeout() * 3;
        let now = Instant::now();
        let ready = self.services.links.is_ready(id).await;
        {
            let mut state = slot.turns.lock();
            if !ready {
                state.ready_since = None;
                return Ok(());
            }
            let since = *state.ready_since.get_or_insert(now);
            let recent = state
                .checked
                .is_some_and(|at| now.saturating_duration_since(at) < bound);
            if now.saturating_duration_since(since) < bound || recent {
                return Ok(());
            }
            state.checked = Some(now);
        }
        let conversation = crate::conversations::uuid(id)?;
        let latest = self.latest_lifecycle(conversation).await?;
        if latest
            .as_ref()
            .is_some_and(|latest| matches!(latest.kind.as_str(), "run.started" | "run.resumed"))
        {
            return Ok(());
        }
        let after = latest.map(|latest| latest.at);
        let reclaimed = self.bounded(async {
            Ok(sqlx::query_scalar!("UPDATE idempotency SET execution_status='queued',held=true,updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1 AND route=$2 AND execution_status IN ('sending','delivered') AND team_copy_seq IS NULL AND updated_at > COALESCE($3,'-infinity'::timestamptz) AND updated_at < CURRENT_TIMESTAMP-make_interval(secs=>$4) RETURNING ordinal", conversation, held::route(id), after, bound.as_secs_f64())
                .fetch_all(&self.services.pool).await?)
        }).await?;
        if !reclaimed.is_empty() {
            slot.turns.lock().turn = None;
            tracing::warn!(conversation_id = %id, count = reclaimed.len(), "messages sent but never taken go out again");
            self.notify_queue(id, None).await;
        }
        Ok(())
    }

    /// A run that failed or was stopped drops the messages held behind it (stored before its end); later ones stay.
    async fn drop_after(
        &self,
        id: &ConversationId,
        conversation: uuid::Uuid,
        messages: &str,
        latest: &Latest,
    ) -> Result<(), GatewayError> {
        let reason = match latest.kind.as_str() {
            "run.failed" => "run_failed",
            "run.cancelled" => "run_cancelled",
            _ => return Ok(()),
        };
        let dropped = self.bounded(async {
            Ok(sqlx::query_scalar!("UPDATE idempotency SET execution_status='failed',execution_error=$4,updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1 AND route=$2 AND held AND execution_status='queued' AND created_at<$3 RETURNING ordinal", conversation, messages, latest.at, reason)
                .fetch_all(&self.services.pool).await?)
        }).await?;
        if !dropped.is_empty() {
            tracing::info!(conversation_id = %id, count = dropped.len(), reason, "held messages dropped with their run");
            self.notify_queue(id, None).await;
        }
        Ok(())
    }

    async fn latest_lifecycle(
        &self,
        conversation: uuid::Uuid,
    ) -> Result<Option<Latest>, GatewayError> {
        self.bounded(async {
            Ok(sqlx::query!("SELECT seq,lifecycle AS \"lifecycle!\",created_at FROM events WHERE conversation_id=$1 AND lifecycle IS NOT NULL ORDER BY seq DESC LIMIT 1", conversation)
                .fetch_optional(&self.services.pool).await?
                .map(|row| Latest { seq: row.seq, kind: row.lifecycle, at: row.created_at }))
        })
        .await
    }

    /// Tells the task's open streams; a notice that can't be delivered is logged, never a failure of the work.
    async fn notify_queue(&self, id: &ConversationId, sent: Option<Vec<String>>) {
        let published = self
            .bounded(held::publish(
                &self.services.pool,
                &self.services.events,
                id,
                sent,
            ))
            .await;
        if let Err(error) = published {
            tracing::warn!(conversation_id = %id, code = error.code(), "held messages notice not delivered");
        }
    }
}
