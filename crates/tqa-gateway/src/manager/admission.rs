//! Slots, eviction and the fair queue (ARCHITECTURE §4.4, ADR-0016, answers-048).
//! A conversation waits when it has accepted work but no sandbox record: derived from the store, never stored.
use super::SandboxManager;
use crate::GatewayError;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tqa_types::{
    ConversationId, QueuePosition, QueueReason, SandboxState, SandboxSummary, ServerFrame,
};
use uuid::Uuid;

/// The admission task also runs on this period, so a missed wakeup only delays admission.
const PERIOD: Duration = Duration::from_secs(1);
/// At most this many waiting conversations are ordered per pass; later ones keep their place for the next pass.
const MAX_WAITING: i64 = 1024;
/// Eviction looks at this many of the least recently active sandboxes for one that is idle.
const EVICTION_CANDIDATES: i64 = 16;
/// The longest an eviction counts as in flight: past it the next pass may choose another victim even if this one's slot
/// is still there (its conversation took a new sandbox under the same slot, or its retire never reported). Above a
/// destroy's bound, so an eviction that is merely slow is never doubled.
const EVICTION_BOUND: Duration = Duration::from_mins(2);

#[derive(Default)]
pub(super) struct Queue {
    wake: Notify,
    /// Held for a whole pass, and by a withdrawal, so a withdrawn task's notice never comes before a stale pass's.
    pass: tokio::sync::Mutex<()>,
    published: Mutex<HashMap<ConversationId, QueuePosition>>,
    /// The sandbox being evicted, and its owner.
    evicting: Mutex<Option<(ConversationId, Uuid, Instant)>>,
}

impl Queue {
    /// Forgets the eviction of `id`, when it is the one in flight: it waits instead (a turn in flight, GW-I-309), so
    /// another eviction may go.
    pub(super) fn release_eviction(&self, id: &ConversationId) {
        if let Ok(mut evicting) = self.evicting.lock()
            && evicting.as_ref().is_some_and(|(victim, _, _)| victim == id)
        {
            *evicting = None;
        }
    }
}

/// What a pass did with one waiting conversation.
enum Grant {
    Admitted,
    Waiting,
    /// Its waiting work was cancelled after the pass read the queue: neither admitted nor listed.
    Withdrawn,
}

struct Waiting {
    conversation: Uuid,
    owner: Uuid,
    live: i64,
    /// Its environment declares `desktop`: it also waits for a free Desktop (`desktop.max_sessions`, ADR-0039).
    desktop: bool,
    /// Another task waits on the same workspace with an older turn, and its owner's cap would admit it: this one goes
    /// after it, as the workspace's hand-over orders turns (GW-I-258).
    behind: bool,
}

impl SandboxManager {
    /// Wakes the admission task: a slot may be free, or new work waits.
    pub(crate) fn nudge_admission(&self) {
        self.admission.wake.notify_one();
    }

    /// Called once a cancel took a task's waiting work: if nothing else waits and no slot was granted, the task left
    /// the slot queue, and open streams stop showing it queued (no sandbox, as before its first message). A slot
    /// already granted keeps starting its sandbox, which idles out. Runs between passes, so a pass that read the queue
    /// before the cancel has published before this. The tasks behind it move up at the next pass.
    ///
    /// # Errors
    /// Fails on a bounded storage error.
    pub(crate) async fn withdrawn(&self, id: &ConversationId) -> Result<(), GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        let _pass = self.admission.pass.lock().await;
        let left = self
            .bounded(async {
                Ok(sqlx::query_scalar!(
                    "SELECT NOT EXISTS(SELECT 1 FROM idempotency WHERE conversation_id=$1 AND execution_status='queued') \
                     AND NOT EXISTS(SELECT 1 FROM sandbox_instances WHERE conversation_id=$1)",
                    conversation
                )
                .fetch_one(&self.services.pool)
                .await?)
            })
            .await?;
        if left.unwrap_or(false) {
            self.forget_position(id)?;
            self.state_notice(id, SandboxState::Destroyed)?;
        }
        self.nudge_admission();
        Ok(())
    }

    /// Starts the owned admission task; it stops with the gateway.
    pub(crate) fn start_admission(self: &Arc<Self>) {
        let manager = self.clone();
        self.services
            .tasks
            .spawn(async move { manager.admission_loop().await });
    }

    async fn admission_loop(self: Arc<Self>) {
        let mut timer = tokio::time::interval(PERIOD);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;
                () = self.services.cancel.cancelled() => return,
                () = self.admission.wake.notified() => {}
                _ = timer.tick() => {}
            }
            if let Err(error) = self.admission_pass().await {
                tracing::debug!(code = error.code(), "admission pass deferred");
            }
        }
    }

    /// Runs one admission pass now. The owned admission task runs the same pass on every nudge and every second;
    /// this entry point exists so a caller can wait for a pass deterministically.
    ///
    /// # Errors
    /// Fails on a bounded storage error.
    pub async fn admission_pass_now(self: &Arc<Self>) -> Result<(), GatewayError> {
        self.admission_pass().await
    }

    /// The host's sandbox records, starting and live: what the slots are counted against.
    async fn sandbox_count(&self) -> Result<i64, GatewayError> {
        self.bounded(async {
            Ok(
                sqlx::query_scalar!("SELECT count(*) FROM sandbox_instances")
                    .fetch_one(&self.services.pool)
                    .await?
                    .unwrap_or(0),
            )
        })
        .await
    }

    /// Admits waiting work while slots are free, publishes positions, and evicts one idle sandbox when none is.
    async fn admission_pass(self: &Arc<Self>) -> Result<(), GatewayError> {
        let _pass = self.admission.pass.lock().await;
        let engines = self.enabled_engines();
        self.fail_missing_engines(&engines).await?;
        let waiting = self.waiting(&engines).await?;
        let live = self.sandbox_count().await?;
        let mut desktops = self.live_desktops().await?;
        // One snapshot for the whole pass: an override applies from the next pass (ADR-0028).
        let admission = self.services.settings.current().limits;
        let mut free = i64::from(admission.slots) - live;
        let mut desktop_waiting: Vec<Uuid> = Vec::new();
        let mut owned: HashMap<Uuid, i64> = HashMap::new();
        let mut queue = Vec::new();
        let mut capped: Vec<(Uuid, Uuid)> = Vec::new();
        let mut leased = Vec::new();
        for entry in waiting {
            // Turns on one workspace go in acceptance order: never admitted, nor handed the workspace, ahead of an older
            // waiter its owner's cap would admit. Admission's fairness between owners otherwise disagreed with the
            // hand-over's order, and two tasks took the workspace from each other every pass (GW-I-258).
            // It waits for its turn on the workspace, not for a slot: said as the workspace's wait (GW-I-303; in
            // real-world use before 17.0, a lead behind its own helper read "Waiting for a free slot · next" with slots
            // free).
            if entry.behind && self.still_behind(entry.conversation, admission).await? {
                leased.push(entry.conversation);
                continue;
            }
            // Waiting for the workspace, not for a slot: hand over from an idle or dead holder (066b).
            if let Some(block) = self.lease_block(entry.conversation).await? {
                self.hand_over(block)?;
                leased.push(entry.conversation);
                continue;
            }
            let count = *owned.entry(entry.owner).or_insert(entry.live);
            if count >= i64::from(admission.per_user) {
                capped.push((entry.conversation, entry.owner));
                continue;
            }
            // Each Desktop holds 4 GiB: past the cap it waits for one to end, or for an idle one to give way below.
            if entry.desktop && desktops >= i64::from(admission.desktop_max_sessions) {
                desktop_waiting.push(entry.conversation);
                continue;
            }
            let grant = if free > 0 {
                self.admit_waiting(entry.conversation).await?
            } else {
                Grant::Waiting
            };
            match grant {
                Grant::Admitted => {
                    free -= 1;
                    owned.insert(entry.owner, count + 1);
                    desktops += i64::from(entry.desktop);
                }
                Grant::Waiting => queue.push((entry.conversation, entry.owner)),
                Grant::Withdrawn => {}
            }
        }
        let starting = self.make_room(free, &queue, &capped).await?;
        // A desktop task waits: an idle Desktop whose browser holds no sign-in gives way (ADR-0039 section 10, amended
        // 17 Oct 2026; in real-world use before 17.0, two desktop tasks waited minutes behind finished tasks).
        if !desktop_waiting.is_empty() {
            self.evict_one(None, true).await?;
        }
        // Owners at their cap are not counted ahead of anyone; everyone eligible is served before them.
        let ahead_of_capped = u32::try_from(queue.len()).unwrap_or(u32::MAX);
        let positions: Vec<(Uuid, QueuePosition)> = queue
            .iter()
            .zip(0_u32..)
            .map(|((id, _), ahead)| (*id, ahead))
            .chain(capped.iter().map(|(id, _)| (*id, ahead_of_capped)))
            .filter_map(|(id, ahead)| {
                let position = if starting == Some(id) {
                    own_position()?
                } else {
                    slot_position(ahead)?
                };
                Some((id, position))
            })
            .chain(
                leased
                    .iter()
                    .filter_map(|id| Some((*id, workspace_position()?))),
            )
            .chain(desktop_positions(&desktop_waiting))
            .collect();
        self.publish_positions(&positions)
    }

    /// The waiting query flags `behind` by the per-user cap alone; whether an older waiter could really start (the
    /// Desktop cap too) is confirmed here, with the hand-over's rule (`lease::admits_waiter`, GW-U-172).
    async fn still_behind(
        &self,
        conversation: Uuid,
        limits: crate::settings::limits::Limits,
    ) -> Result<bool, GatewayError> {
        self.bounded(async {
            let mut connection = self.services.pool.acquire().await?;
            super::lease::behind_older_waiter(
                &mut connection,
                conversation,
                super::lease::Caps::of(&limits, self.config.io_timeout()),
            )
            .await
        })
        .await
    }

    /// Live sandboxes whose environment declares `desktop`.
    async fn live_desktops(&self) -> Result<i64, GatewayError> {
        self.bounded(async {
            Ok(sqlx::query_scalar!(
                "SELECT count(*) FROM sandbox_instances s JOIN conversations c ON c.id=s.conversation_id JOIN templates t ON t.name=c.template WHERE 'desktop'=ANY(t.capabilities)"
            )
            .fetch_one(&self.services.pool)
            .await?
            .unwrap_or(0))
        })
        .await
    }

    /// Waiting conversations in service order: owners with the fewest live sandboxes first, then earliest accepted
    /// action. Derived from the store on every pass, so work accepted while the gateway was down is handled by the
    /// first pass after start-up.
    /// A conversation whose profile names a decision engine not in `engines` doesn't wait: a sandbox would only be
    /// refused at the link (ADR-0013). It waits again once the engine is back.
    async fn waiting(&self, engines: &[String]) -> Result<Vec<Waiting>, GatewayError> {
        self.bounded(async {
            let rows = sqlx::query!("WITH live AS (SELECT c.owner_id, count(*) AS n FROM sandbox_instances s JOIN conversations c ON c.id=s.conversation_id GROUP BY c.owner_id), queued AS (SELECT i.conversation_id, MIN(i.created_at) AS first FROM idempotency i WHERE i.execution_status='queued' GROUP BY i.conversation_id), waiting AS (SELECT w.conversation_id, c.owner_id, c.workspace_id, MIN(w.first) AS first FROM queued w JOIN conversations c ON c.id=w.conversation_id WHERE c.state='active' AND (c.profile->>'decision_engine' IS NULL OR c.profile->>'decision_engine' = ANY($2)) AND NOT EXISTS (SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=w.conversation_id) GROUP BY w.conversation_id, c.owner_id, c.workspace_id) SELECT w.conversation_id AS \"conversation_id!\", w.owner_id AS \"owner_id!\", COALESCE(l.n,0) AS \"live!\", EXISTS(SELECT 1 FROM waiting o LEFT JOIN live ol ON ol.owner_id=o.owner_id WHERE o.workspace_id=w.workspace_id AND o.conversation_id<>w.conversation_id AND o.first<w.first AND COALESCE(ol.n,0)<$3) AS \"behind!\", EXISTS (SELECT 1 FROM conversations d JOIN templates t ON t.name=d.template WHERE d.id=w.conversation_id AND 'desktop'=ANY(t.capabilities)) AS \"desktop!\" FROM waiting w LEFT JOIN live l ON l.owner_id=w.owner_id ORDER BY COALESCE(l.n,0), w.first, w.conversation_id LIMIT $1", MAX_WAITING, engines, i64::from(self.services.settings.current().limits.per_user))
                .fetch_all(&self.services.pool)
                .await?;
            Ok(rows
                .into_iter()
                .map(|row| Waiting { conversation: row.conversation_id, owner: row.owner_id, live: row.live, behind: row.behind, desktop: row.desktop })
                .collect())
        })
        .await
    }

    /// The installation's enabled decision engines, from the live settings.
    fn enabled_engines(&self) -> Vec<String> {
        self.services
            .settings
            .current()
            .decision_engines
            .iter()
            .filter(|engine| engine.enabled)
            .map(|engine| engine.name.clone())
            .collect()
    }

    /// Fails, once, the queued work of conversations without a sandbox whose profile names an engine the installation
    /// no longer has or has disabled, and tells their owners (ADR-0013 point 6): admission leaves them alone, so the
    /// work would otherwise show as queued for ever. A message after the engine is back runs again.
    async fn fail_missing_engines(&self, engines: &[String]) -> Result<(), GatewayError> {
        let stranded = self
            .bounded(async {
                Ok(sqlx::query_scalar!("SELECT DISTINCT i.conversation_id AS \"conversation_id!\" FROM idempotency i JOIN conversations c ON c.id=i.conversation_id WHERE i.execution_status='queued' AND c.state='active' AND c.profile->>'decision_engine' IS NOT NULL AND NOT (c.profile->>'decision_engine' = ANY($1)) AND NOT EXISTS (SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=i.conversation_id) LIMIT $2", engines, MAX_WAITING)
                    .fetch_all(&self.services.pool)
                    .await?)
            })
            .await?;
        let cause = GatewayError::Conflict("decision_engine_unavailable");
        for conversation in stranded {
            let id: ConversationId = conversation
                .to_string()
                .parse()
                .map_err(|_| GatewayError::StoreUnavailable)?;
            self.forget_position(&id)?;
            let routes = self.fail_actions(&id, &cause).await?;
            if !routes.is_empty() {
                self.report_failure(&id, &routes, &cause)?;
            }
        }
        Ok(())
    }

    /// Takes a slot for a waiting conversation and starts its worker, only while it still has waiting work: a cancel
    /// since this pass read the queue withdrew it (GW-I-173).
    async fn admit_waiting(self: &Arc<Self>, conversation: Uuid) -> Result<Grant, GatewayError> {
        let id: ConversationId = conversation
            .to_string()
            .parse()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        // The worker's slot before the record: a full slot table leaves the work waiting, never a record that no worker
        // serves (GW-I-251, GW-U-171). Held until the worker is scheduled, so it can't be evicted in between.
        let _slot = match self.slot(&id) {
            Ok(slot) => slot,
            Err(GatewayError::Busy) => return Ok(Grant::Waiting),
            Err(error) => return Err(error),
        };
        match self.admit_waiting_work(&id).await {
            Ok(true) => {
                self.forget_position(&id)?;
                // An open stream moves from `queued` to `creating` before the worker starts the sandbox.
                self.state_notice(&id, tqa_types::SandboxState::Creating)?;
                self.schedule(&id)?;
                Ok(Grant::Admitted)
            }
            Ok(false) => Ok(Grant::Withdrawn),
            Err(GatewayError::Busy | GatewayError::NotFound | GatewayError::Conflict(_)) => {
                Ok(Grant::Waiting)
            }
            Err(error) => Err(error),
        }
    }

    /// Publishes `queued` with the position, only when it changed (answers-050).
    fn publish_positions(&self, positions: &[(Uuid, QueuePosition)]) -> Result<(), GatewayError> {
        let mut published = self
            .admission
            .published
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let current: HashMap<ConversationId, QueuePosition> = positions
            .iter()
            .filter_map(|(id, ahead)| Some((id.to_string().parse().ok()?, *ahead)))
            .collect();
        for (id, ahead) in &current {
            if published.get(id) != Some(ahead) {
                self.services
                    .events
                    .control(id, queued_notice(id, self.config.backend, *ahead))?;
            }
        }
        *published = current;
        Ok(())
    }

    fn forget_position(&self, id: &ConversationId) -> Result<(), GatewayError> {
        self.admission
            .published
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .remove(id);
        Ok(())
    }

    /// The position of a waiting conversation, for a stream that opens now.
    ///
    /// # Errors
    /// Fails on a bounded storage error.
    pub(crate) async fn queue_position(
        &self,
        id: &ConversationId,
    ) -> Result<Option<QueuePosition>, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        if let Some(position) = self
            .admission
            .published
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .get(id)
        {
            return Ok(Some(*position));
        }
        let waiting = self.waiting(&self.enabled_engines()).await?;
        let Some((ahead, entry)) = waiting
            .iter()
            .zip(0_u32..)
            .find(|(entry, _)| entry.conversation == conversation)
            .map(|(entry, ahead)| (ahead, entry))
        else {
            return Ok(None);
        };
        let limits = self.services.settings.current().limits;
        Ok(
            if (entry.behind && self.still_behind(conversation, limits).await?)
                || self.lease_block(conversation).await?.is_some()
            {
                workspace_position()
            } else {
                slot_position(ahead)
            },
        )
    }

    /// Another task's sandbox holds this conversation's workspace (066b).
    ///
    /// # Errors
    /// A bounded store failure.
    pub(super) async fn lease_block(
        &self,
        conversation: Uuid,
    ) -> Result<Option<super::lease::Block>, GatewayError> {
        self.bounded(async {
            let mut connection = self.services.pool.acquire().await?;
            let caps = super::lease::Caps::of(
                &self.services.settings.current().limits,
                self.config.io_timeout(),
            );
            super::lease::block(&mut connection, conversation, caps).await
        })
        .await
    }

    /// Retires the lease holder for the task that waits for its workspace, when it may hand over (idle or dead).
    fn hand_over(self: &Arc<Self>, block: super::lease::Block) -> Result<(), GatewayError> {
        if !block.handoff {
            return Ok(());
        }
        let id: ConversationId = block
            .holder
            .to_string()
            .parse()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        self.retire_for_switch(&id, super::Teardown::HandOver)
    }

    /// Makes room when work waits: on a full host, the least recently active idle sandbox; for an owner at its cap, one
    /// of its own, since a busy or pinned own sandbox is never evicted and that owner's work stays
    /// queued. Returns the task whose only wait is then its owner's own idle sandbox retiring: from that person's side
    /// it is starting (GW-I-158).
    async fn make_room(
        self: &Arc<Self>,
        free: i64,
        queue: &[(Uuid, Uuid)],
        capped: &[(Uuid, Uuid)],
    ) -> Result<Option<Uuid>, GatewayError> {
        if free <= 0
            && let Some(&(first, owner)) = queue.first()
            && let Some(retiring) = self.evict_one(None, false).await?
        {
            return Ok((retiring == owner).then_some(first));
        }
        let mut owners: Vec<Uuid> = capped.iter().map(|(_, owner)| *owner).collect();
        owners.dedup();
        for owner in owners {
            let Some(retiring) = self.evict_one(Some(owner), false).await? else {
                continue;
            };
            let first = capped
                .iter()
                .find(|(_, of)| *of == owner)
                .map(|(id, _)| *id);
            return Ok(first.filter(|_| retiring == owner));
        }
        Ok(None)
    }

    /// Retires the least recently active idle sandbox: live, not pinned, no queued work and no active run.
    /// Busy and pinned sandboxes are never evicted. One eviction is in flight at a time.
    /// Returns the owner of the sandbox whose eviction is requested or already in flight, if any. `owner` limits new
    /// candidates to that owner; `desktops` to Desktops, for a waiting desktop task (GW-I-305). Whatever waits, a
    /// Desktop goes only when its browser profile is known empty and nobody holds its screen (GW-I-308). A sandbox with
    /// a turn in flight (one its runtime took and hasn't started a run for) is no candidate (`lease::turn_in_flight`).
    async fn evict_one(
        self: &Arc<Self>,
        owner: Option<Uuid>,
        desktops: bool,
    ) -> Result<Option<Uuid>, GatewayError> {
        {
            let mut evicting = self
                .admission
                .evicting
                .lock()
                .map_err(|_| GatewayError::StoreUnavailable)?;
            // One eviction at a time, but only while it is really under way: a victim whose destroy failed (its slot
            // kept, unconfirmed, the retire retried in the background) no longer holds every other eviction back
            // (GW-I-310, a live check before 17.0: a waiter got in only when an unrelated sandbox idled out).
            if let Some((victim, of, asked)) = evicting.as_ref()
                && still_evicting(self.victim_unconfirmed(victim)?, asked.elapsed())
            {
                return Ok(Some(*of));
            }
            *evicting = None;
        }
        let candidates = self
            .bounded(async {
                Ok(sqlx::query!("SELECT s.conversation_id,c.owner_id,EXISTS (SELECT 1 FROM templates t WHERE t.name=c.template AND 'desktop'=ANY(t.capabilities)) AS \"desktop!\" FROM sandbox_instances s JOIN conversations c ON c.id=s.conversation_id WHERE s.phase='live' AND NOT c.pinned AND NOT EXISTS (SELECT 1 FROM idempotency i WHERE i.conversation_id=s.conversation_id AND (i.execution_status IN ('queued','sending') OR (i.execution_status='delivered' AND i.updated_at > CURRENT_TIMESTAMP - make_interval(secs => $4) AND i.updated_at > COALESCE((SELECT MAX(e.created_at) FROM events e WHERE e.conversation_id=s.conversation_id AND e.lifecycle IS NOT NULL), '-infinity'::timestamptz) AND COALESCE((SELECT e.lifecycle FROM events e WHERE e.conversation_id=s.conversation_id AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1), '') NOT IN ('run.started', 'run.resumed')))) AND ($2::uuid IS NULL OR c.owner_id=$2) AND (NOT $3::bool OR EXISTS (SELECT 1 FROM templates t WHERE t.name=c.template AND 'desktop'=ANY(t.capabilities))) ORDER BY s.last_action_at LIMIT $1", EVICTION_CANDIDATES, owner, desktops, (self.config.io_timeout() * 3).as_secs_f64())
                    .fetch_all(&self.services.pool)
                    .await?)
            })
            .await?;
        for candidate in candidates {
            let active = self
                .bounded(crate::events::run_activity(
                    &self.services.pool,
                    candidate.conversation_id,
                ))
                .await?;
            if active == crate::events::RunActivity::Active {
                continue;
            }
            let id: ConversationId = candidate
                .conversation_id
                .to_string()
                .parse()
                .map_err(|_| GatewayError::StoreUnavailable)?;
            // A Desktop whose browser may hold a sign-in, or whose screen a person has, keeps its place (ADR-0039
            // section 10), for a waiting desktop and for a slot alike.
            if candidate.desktop && !crate::screen::may_retire(&id) {
                continue;
            }
            self.retire_for_switch(&id, super::Teardown::Eviction)?;
            *self
                .admission
                .evicting
                .lock()
                .map_err(|_| GatewayError::StoreUnavailable)? =
                Some((id, candidate.owner_id, Instant::now()));
            return Ok(Some(candidate.owner_id));
        }
        Ok(None)
    }

    /// Whether `id`'s slot is there and its sandbox's destroy failed (`Some(true)`) or not (`Some(false)`); `None` without
    /// a slot.
    fn victim_unconfirmed(&self, id: &ConversationId) -> Result<Option<bool>, GatewayError> {
        Ok(self
            .slots
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .get(id)
            .map(|slot| slot.unconfirmed.load(std::sync::atomic::Ordering::Acquire)))
    }
}

/// Whether an eviction asked for `elapsed` ago is still under way: its victim's slot is there (`unconfirmed` is `Some`),
/// its destroy hasn't failed, and it is within [`EVICTION_BOUND`].
fn still_evicting(unconfirmed: Option<bool>, elapsed: Duration) -> bool {
    unconfirmed == Some(false) && elapsed < EVICTION_BOUND
}

/// The unsequenced `session.ready` a waiting conversation receives.
/// A place in the slot queue, clamped to the protocol bound.
fn slot_position(ahead: u32) -> Option<QueuePosition> {
    QueuePosition::new(ahead.min(tqa_types::MAX_QUEUE_AHEAD))
        .ok()
        .map(|position| position.with_reason(QueueReason::Slot))
}

/// Waiting only for the same person's own idle sandbox to retire: starting, from their side (GW-I-158).
fn own_position() -> Option<QueuePosition> {
    QueuePosition::new(0)
        .ok()
        .map(|position| position.with_reason(QueueReason::OwnSandbox))
}

/// Waiting for a free Desktop under `desktop.max_sessions` (ADR-0039), each behind the desktop tasks before it.
fn desktop_positions(waiting: &[Uuid]) -> impl Iterator<Item = (Uuid, QueuePosition)> + '_ {
    waiting.iter().zip(0_u32..).filter_map(|(id, ahead)| {
        let position = QueuePosition::new(ahead.min(tqa_types::MAX_QUEUE_AHEAD)).ok()?;
        Some((*id, position.with_reason(QueueReason::DesktopSessions)))
    })
}

/// Waiting for the workspace: next as soon as its holder is gone.
fn workspace_position() -> Option<QueuePosition> {
    QueuePosition::new(0)
        .ok()
        .map(|position| position.with_reason(QueueReason::Workspace))
}

pub(crate) fn queued_notice(
    id: &ConversationId,
    backend: tqa_types::SandboxBackend,
    position: QueuePosition,
) -> ServerFrame {
    ServerFrame::SessionReady {
        conversation_id: id.clone(),
        resumed: true,
        sandbox: SandboxSummary::new(backend, SandboxState::Queued),
        queue: Some(position),
    }
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
