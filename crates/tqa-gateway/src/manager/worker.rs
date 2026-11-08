use super::idle::Lifetime;
use super::{SandboxManager, Slot, failure};
use crate::GatewayError;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tqa_types::ConversationId;

impl SandboxManager {
    pub(crate) fn schedule(self: &Arc<Self>, id: &ConversationId) -> Result<(), GatewayError> {
        let slot = self.slot(id)?;
        slot.notify.notify_one();
        if !slot.running.swap(true, Ordering::AcqRel) {
            let manager = self.clone();
            let id = id.clone();
            self.services.tasks.spawn(manager.supervise(id, slot));
        }
        Ok(())
    }

    /// [`Self::schedule`] for a task whose message is stored: a full slot table leaves its work queued for admission,
    /// never a 429 for work the gateway kept, which a retry would send twice (PERF-020 on c16). `admit` holds a new
    /// sandbox's slot, so only an existing record whose slot is gone gets here.
    ///
    /// # Errors
    /// As [`Self::schedule`], except `429 busy`.
    pub(crate) fn schedule_admitted(
        self: &Arc<Self>,
        id: &ConversationId,
    ) -> Result<(), GatewayError> {
        match self.schedule(id) {
            Err(GatewayError::Busy) => {
                tracing::error!(conversation_id = %id, "an admitted task found the slot table full; its work stays queued");
                self.nudge_admission();
                Ok(())
            }
            result => result,
        }
    }

    async fn supervise(self: Arc<Self>, id: ConversationId, slot: Arc<Slot>) {
        let outcome = self.run(&id, &slot).await;
        if matches!(outcome, Err(GatewayError::Busy)) {
            // Waiting for a slot, or for the retiring owner to leave: the work stays queued.
            self.nudge_admission();
        } else if let Err(error) = outcome
            && !self.services.cancel.is_cancelled()
            && self.failed(&id, &slot, &error).await.is_err()
        {
            tracing::error!(
                conversation_id = %id,
                "sandbox failure cleanup is incomplete; reconciliation is required"
            );
        }
        slot.running.store(false, Ordering::Release);
    }

    async fn run(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
    ) -> Result<(), GatewayError> {
        if slot.unconfirmed.load(Ordering::Acquire) {
            return self.teardown_first(id, slot).await;
        }
        let mut handle = self.start(id, slot).await?;
        let mut idle = super::idle::Idle::new(self.services.idle_clock.clone());
        let mut timer = tokio::time::interval(Duration::from_millis(250));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let pass = self.work_pass(id, slot, &mut handle, &mut idle).await;
            match pass {
                Ok(true) => return Ok(()),
                Ok(false)
                | Err(
                    GatewayError::StoreUnavailable
                    | GatewayError::Timeout
                    | GatewayError::Conflict("sandbox_not_ready"),
                ) => (),
                Err(error) => return Err(error),
            }
            tokio::select! {biased;
                ()=self.services.cancel.cancelled()=>return Err(GatewayError::Cancelled),
                ()=slot.notify.notified()=>(),
                _=timer.tick()=>(),
                ()=self.services.idle_clock.changed()=>(),
            }
        }
    }

    /// The sandbox, started and its runtime registered (or a switch or a refusal to act on at once). A VM host that died
    /// before its runtime registered is started once more within the create budget: a setup step's crash loses no task
    /// state, so a person doesn't see a failed start for it. A second death in a row fails the start, as before
    /// (SBX-F-038).
    async fn start(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
    ) -> Result<tqa_sandbox::SandboxHandle, GatewayError> {
        let began = tokio::time::Instant::now();
        let mut handle = self.ensure(id).await?;
        let mut retried = false;
        loop {
            match self.until_ready(slot, &handle).await {
                Ok(()) => return Ok(handle),
                Err(tqa_sandbox::SandboxError::HostDied)
                    if self.may_start_again(began, retried) =>
                {
                    retried = true;
                    handle = self.start_again(id, slot, &handle).await?;
                }
                Err(error) => return Err(failure("start")(error)),
            }
        }
    }

    /// Waits for the sandbox's runtime to register. A switch or a refused runtime retires a sandbox that is still
    /// starting at once: the first pass acts on it, instead of waiting out the readiness bound (and then failing the
    /// queued work as a start failure).
    async fn until_ready(
        &self,
        slot: &Arc<Slot>,
        handle: &tqa_sandbox::SandboxHandle,
    ) -> Result<(), tqa_sandbox::SandboxError> {
        let ready = self.backend.engine.wait_ready(
            handle,
            self.services.links.as_ref(),
            self.config.sandbox.ready(),
            self.services.cancel.clone(),
        );
        tokio::pin!(ready);
        loop {
            tokio::select! {biased;
                result=&mut ready=>return result,
                ()=slot.notify.notified()=>{
                    if slot.switch.load(Ordering::Acquire) || slot.refused.load(Ordering::Acquire) != 0 {return Ok(());}
                }
            }
        }
    }

    async fn work_pass(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        handle: &mut tqa_sandbox::SandboxHandle,
        idle: &mut super::idle::Idle,
    ) -> Result<bool, GatewayError> {
        if slot.switch.swap(false, Ordering::AcqRel) && !self.gives_way_later(id, slot).await? {
            self.retire_switched(id, slot).await?;
            return Ok(true);
        }
        if let Some(refused) = super::Refused::take(&slot.refused) {
            self.retire_refused(id, slot, refused).await?;
            return Ok(true);
        }
        match self.recover_missing(id, slot, handle, idle).await? {
            super::recovery::Recovery::Present => {}
            super::recovery::Recovery::Replaced(replacement) => {
                *handle = replacement;
                idle.started_now();
            }
            super::recovery::Recovery::Waiting => return Ok(false),
            super::recovery::Recovery::Stopped => return Ok(true),
        }
        // A sandbox past its lifetime takes no new work: queued actions wait for its replacement.
        if !idle.past_lifetime() {
            // Boxed: holding and joining make this future large (clippy::large_futures).
            Box::pin(self.deliver_held(id, slot)).await?;
        }
        let activity = self.observe_activity(id, idle).await?;
        // On Docker a running workspace is measured, and its run stopped past the quota plus a margin (N4).
        if self.live_quota(id, slot, idle).await? {
            return Ok(true);
        }
        match idle.lifetime(activity.timers.max_lifetime) {
            Lifetime::Within => {}
            Lifetime::Grace => return Ok(false),
            // A turn its runtime took and hasn't started a run for keeps the sandbox: retired now, it is stranded.
            Lifetime::Retire if self.turn_in_flight(id).await? => return Ok(false),
            Lifetime::Retire => {
                if self.mark_closing(id, Some(idle)).await? {
                    self.lifetime_reached(id, "idle");
                    self.retire(id, slot, super::Teardown::Lifetime).await?;
                    self.after_retirement(id).await?;
                    return Ok(true);
                }
                return Ok(false);
            }
            Lifetime::RetireBusy => {
                self.lifetime_reached(id, "busy");
                self.mark_closing(id, None).await?;
                let retired = self.retire(id, slot, super::Teardown::LifetimeBusy).await;
                self.state_notice(id, tqa_types::SandboxState::Destroyed)?;
                match retired {
                    // The run is still active: a replacement starts now, and its runtime resumes the run from the journal.
                    Ok(()) => self.schedule(id)?,
                    // A slow engine may still remove it: retried in the background, and the run resumes after that.
                    Err(error) => {
                        tracing::warn!(conversation_id = %id, code = error.code(),
                            "a busy sandbox's teardown failed at its lifetime; retrying it in the background");
                        self.retire_later(id.clone(), slot.clone(), super::teardown::Then::Resume);
                    }
                }
                return Ok(true);
            }
        }
        // A pinned sandbox is kept through idle up to its lifetime (keep-alive pin); the lifetime is checked above.
        let due = !activity.pinned && idle.expired(activity.timers.idle);
        if due && self.mark_closing(id, Some(idle)).await? {
            self.retire(id, slot, super::Teardown::Idle).await?;
            self.after_retirement(id).await?;
            return Ok(true);
        }
        Ok(false)
    }

    /// A sandbox whose destroy was never confirmed is removed before anything else: once it is gone, a new worker
    /// starts the conversation's sandbox; while it is not, the queued work fails visibly and nothing starts beside it.
    async fn teardown_first(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
    ) -> Result<(), GatewayError> {
        match self.retire(id, slot, super::Teardown::Unconfirmed).await {
            Ok(()) => {
                self.state_notice(id, tqa_types::SandboxState::Destroyed)?;
                self.schedule(id)
            }
            Err(error) => {
                tracing::warn!(conversation_id = %id, code = error.code(),
                    "the old sandbox still could not be removed; nothing new starts beside it");
                let routes = self.fail_actions(id, &error).await?;
                self.report_failure(id, &routes, &error)
            }
        }
    }

    /// Whether a hand-over or an eviction asked of this sandbox waits instead: a turn its runtime took and hasn't
    /// started a run for would be stranded with the sandbox, as nothing sends it again (a live check before 17.0,
    /// GW-I-309). The admission asks again once the run has ended; an eviction it was counting on is released so
    /// another can go.
    async fn gives_way_later(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
    ) -> Result<bool, GatewayError> {
        let reason = slot
            .switch_reason
            .lock()
            .map_or(super::Teardown::EnvironmentSwitch, |why| *why);
        if !matches!(
            reason,
            super::Teardown::HandOver | super::Teardown::Eviction
        ) || !self.turn_in_flight(id).await?
        {
            return Ok(false);
        }
        // A holder whose lease ran out is taken as dead (no heartbeats): its hand-over goes (GW-I-031).
        if reason == super::Teardown::HandOver && self.lease_expired(id).await? {
            return Ok(false);
        }
        self.admission.release_eviction(id);
        tracing::info!(conversation_id = %id, reason = reason.as_str(),
            "a retirement waits: the task's turn was taken and its run hasn't started");
        Ok(true)
    }

    /// Whether `id`'s workspace lease, when it holds it, has run out.
    async fn lease_expired(&self, id: &ConversationId) -> Result<bool, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        self.bounded(async {
            Ok(sqlx::query_scalar!(
                "SELECT EXISTS(SELECT 1 FROM workspaces w JOIN conversations c ON c.workspace_id = w.id \
                 WHERE c.id = $1 AND w.lease_conversation_id = c.id \
                 AND COALESCE(w.lease_expires_at <= CURRENT_TIMESTAMP, true)) AS \"expired!\"",
                conversation
            )
            .fetch_one(&self.services.pool)
            .await?)
        })
        .await
    }

    /// Whether `id` has a turn in flight that its sandbox must stay for (`lease::turn_in_flight`).
    async fn turn_in_flight(&self, id: &ConversationId) -> Result<bool, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        self.bounded(async {
            let mut connection = self.services.pool.acquire().await?;
            super::lease::turn_in_flight(
                &mut connection,
                conversation,
                self.config.io_timeout() * 3,
            )
            .await
        })
        .await
    }

    /// One log line and one metric per sandbox retired at its max lifetime (ARCHITECTURE §4.4, rule 1).
    fn lifetime_reached(&self, id: &ConversationId, how: &'static str) {
        tracing::warn!(conversation_id = %id, how, "sandbox reached its max lifetime");
        self.services.telemetry.sandbox_retired("lifetime");
    }

    /// Retires a sandbox whose conversation switched template. An action accepted meanwhile starts a new
    /// sandbox, built from the new template, instead of reaching the old one.
    async fn retire_switched(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
    ) -> Result<(), GatewayError> {
        self.mark_closing(id, None).await?;
        let why = slot
            .switch_reason
            .lock()
            .map_or(super::Teardown::EnvironmentSwitch, |why| *why);
        if let Err(error) = self.retire(id, slot, why).await {
            // An evicted sandbox is idle, so its failed destroy is nobody's failure: retried in the background, and
            // released once found gone, instead of a second destroy on the spot and a slot left for a restart. Its
            // eviction is no longer the one in flight, so admission evicts another meanwhile (GW-I-310, the 16o live
            // checks' W12).
            if why == super::Teardown::Eviction {
                tracing::warn!(conversation_id = %id, code = error.code(),
                    "an evicted sandbox's teardown failed; retrying it in the background");
                self.admission.release_eviction(id);
                self.retire_later(id.clone(), slot.clone(), super::teardown::Then::Idle);
                self.nudge_admission();
                return Ok(());
            }
            return Err(error);
        }
        self.after_retirement(id).await
    }

    /// Retires a sandbox refused at the link, without a replacement: one whose runtime is below the protocol floor
    /// (ADR-0026), which a new sandbox from the same image would repeat until the image is updated, or one whose
    /// decision engine is gone (ADR-0013), until an administrator restores it. The task sees the sandbox as failed.
    async fn retire_refused(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        refused: super::Refused,
    ) -> Result<(), GatewayError> {
        self.mark_closing(id, None).await?;
        self.retire(id, slot, super::Teardown::Refused).await?;
        let (code, message, retryable) = refused.notice();
        // The owner learns why, not only that the sandbox failed (GW-I-087).
        self.services.events.control(
            id,
            tqa_types::ServerFrame::Error {
                seq: None,
                code,
                message: message.into(),
                retryable,
                target: None,
            },
        )?;
        self.state_notice(id, tqa_types::SandboxState::Failed)
    }

    /// Clients show a retired sandbox as sleeping. Work accepted while it retired starts a new sandbox now.
    async fn after_retirement(self: &Arc<Self>, id: &ConversationId) -> Result<(), GatewayError> {
        self.state_notice(id, tqa_types::SandboxState::Destroyed)?;
        if self.has_queued(id).await? {
            self.schedule(id)?;
        }
        Ok(())
    }

    pub(super) async fn has_queued(&self, id: &ConversationId) -> Result<bool, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        let queued = self
            .bounded(async {
                Ok(sqlx::query_scalar!(
                    "SELECT EXISTS(SELECT 1 FROM idempotency WHERE conversation_id=$1 AND execution_status='queued')",
                    conversation
                )
                .fetch_one(&self.services.pool)
                .await?)
            })
            .await?;
        Ok(queued.unwrap_or(false))
    }

    pub(super) async fn observe_activity(
        &self,
        id: &ConversationId,
        idle: &mut super::idle::Idle,
    ) -> Result<Activity, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        let row = self
            .bounded(async {
                sqlx::query!(
                    "SELECT s.last_action_at,c.pinned,s.idle_seconds,s.max_lifetime_seconds FROM sandbox_instances s JOIN conversations c ON c.id=s.conversation_id WHERE s.conversation_id=$1",
                    conversation
                )
                .fetch_one(&self.services.pool)
                .await
                .map_err(GatewayError::from)
            })
            .await?;
        idle.action(row.last_action_at);
        let frames = self
            .services
            .events
            .read(id, idle.sequence, self.config.limits.page_size)
            .await?;
        for frame in frames {
            idle.observe(&frame)?;
        }
        // The timers the sandbox was admitted with; one admitted before they were recorded follows the current settings.
        let current = self.services.settings.current().limits;
        let seconds = |captured: Option<i32>, fallback: u32| {
            std::time::Duration::from_secs(
                captured
                    .and_then(|value| u64::try_from(value).ok())
                    .unwrap_or(u64::from(fallback)),
            )
        };
        Ok(Activity {
            pinned: row.pinned,
            timers: Timers {
                idle: seconds(row.idle_seconds, current.idle_seconds),
                max_lifetime: seconds(row.max_lifetime_seconds, current.max_lifetime_seconds),
            },
        })
    }

    async fn failed(
        &self,
        id: &ConversationId,
        slot: &Arc<Slot>,
        cause: &GatewayError,
    ) -> Result<(), GatewayError> {
        self.mark_closing(id, None).await?;
        let routes = self.fail_actions(id, cause).await?;
        // Report before cleanup, so a failing cleanup can never hide the failure from the user.
        self.report_failure(id, &routes, cause)?;
        self.retire(id, slot, super::Teardown::StartFailed).await
    }

    /// Marks every queued action of the conversation failed with the cause's wire code and returns their routes.
    pub(super) async fn fail_actions(
        &self,
        id: &ConversationId,
        cause: &GatewayError,
    ) -> Result<Vec<String>, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        let code = failure_code(cause).as_str();
        self.bounded(async {
            Ok(sqlx::query_scalar!("UPDATE idempotency SET execution_status='failed',execution_error=$2,updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1 AND execution_status='queued' RETURNING route",conversation,code).fetch_all(&self.services.pool).await?)
        }).await
    }

    /// Logs one WARN per failed action and tells every open stream, with a bounded, secret-free cause.
    pub(super) fn report_failure(
        &self,
        id: &ConversationId,
        routes: &[String],
        cause: &GatewayError,
    ) -> Result<(), GatewayError> {
        if routes.is_empty() {
            tracing::warn!(conversation_id = %id, cause = %cause, "sandbox failed with no queued action");
        }
        for route in routes {
            tracing::warn!(conversation_id = %id, route = %route, cause = %cause, "queued action failed");
        }
        let mut notice = cause.frame();
        if let tqa_types::ServerFrame::Error {
            code, retryable, ..
        } = &mut notice
        {
            // A failed action is always retryable by sending it again as a new action.
            *code = failure_code(cause);
            *retryable = true;
        }
        self.services.events.control(id, notice)?;
        self.state_notice(id, tqa_types::SandboxState::Failed)
    }
}

/// The wire code of a failed action: `runtime_start_failed` for a runtime exit, `decision_engine_unavailable` for a
/// task whose engine is gone, otherwise `sandbox_failed`.
pub(super) fn failure_code(cause: &GatewayError) -> tqa_types::ErrorCode {
    match cause {
        GatewayError::RuntimeExited { .. } => tqa_types::ErrorCode::RuntimeStartFailed,
        GatewayError::Conflict("decision_engine_unavailable") => {
            tqa_types::ErrorCode::DecisionEngineUnavailable
        }
        _ => tqa_types::ErrorCode::SandboxFailed,
    }
}

/// What one pass observed of a sandbox: whether its conversation is pinned, and the timers it runs under.
pub(super) struct Activity {
    pub pinned: bool,
    pub timers: Timers,
}

/// A sandbox's idle and lifetime delays, fixed when it was admitted (ADR-0028 slice 5).
pub(super) struct Timers {
    pub idle: std::time::Duration,
    pub max_lifetime: std::time::Duration,
}
