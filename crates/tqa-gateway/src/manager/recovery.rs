//! Missing instances are replaced without terminating the run's authoritative history.
use super::losses::{Next, Retry};
use super::{SandboxManager, Slot, failure};
use crate::GatewayError;
use std::sync::Arc;
use tqa_sandbox::{SandboxError, SandboxHandle};
use tqa_types::{ConversationId, SandboxRecovery, SandboxState, SandboxSummary, ServerFrame};

/// What became of a sandbox the worker checked.
pub(super) enum Recovery {
    /// It is there.
    Present,
    /// It was lost and has been replaced by this one.
    Replaced(SandboxHandle),
    /// It was lost; its replacement waits for its delay.
    Waiting,
    /// It was lost too often and retired as failed, or its removal is retried in the background.
    Stopped,
}

struct Record {
    conversation_id: uuid::Uuid,
    sandbox_id: Option<String>,
    phase: String,
}

impl SandboxManager {
    /// Replaces a sandbox that is no longer there, after the delay [`super::losses`] sets, or retires it as failed
    /// past its limit. The run's history is untouched either way: the replacement's runtime resumes from its journal.
    pub(super) async fn recover_missing(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        handle: &SandboxHandle,
        idle: &mut super::idle::Idle,
    ) -> Result<Recovery, GatewayError> {
        match self
            .bounded(async { Ok(self.backend.engine.address(handle).await) })
            .await?
        {
            Ok(_) => return Ok(Recovery::Present),
            Err(SandboxError::NotFound) => (),
            // A daemon outage is not evidence that the container has died.
            Err(_) => return Err(GatewayError::StoreUnavailable),
        }
        // A deleted task's sandbox went with it (its runtime's token went at the delete): it is retired and its slot
        // freed, never replaced or retried (candidate 14a: deleted tasks held every slot while their replacements were
        // retried).
        if self.task_gone(id).await? {
            self.retire(id, slot, super::Teardown::TaskDeleted).await?;
            return Ok(Recovery::Stopped);
        }
        let now = idle.now();
        // A replacement whose create failed (a full disk) is retried on its own schedule, never counted as a new loss.
        if let Some(retry) = idle.losses.create_retry(now) {
            return match retry {
                Retry::In(_) => Ok(Recovery::Waiting),
                Retry::Now => self.try_replacing(id, slot, idle).await,
                Retry::Exhausted => self.stop_after_failed_creates(id, slot, idle).await,
            };
        }
        let lost_now = !idle.losses.pending();
        let next = idle.losses.next(now);
        if lost_now && let Err(error) = self.clear_lost(slot, handle).await {
            // A failed cleanup is never the task's failure: the removal is retried in the background while the
            // task shows it is starting, and nothing starts beside the old instance until it is gone (GW-I-160).
            tracing::warn!(conversation_id = %id, code = error.code(),
                "a lost sandbox's removal failed; retrying it in the background");
            slot.unconfirmed
                .store(true, std::sync::atomic::Ordering::Release);
            self.state_notice(id, SandboxState::Creating)?;
            self.retire_later(id.clone(), slot.clone(), super::teardown::Then::Resume);
            return Ok(Recovery::Stopped);
        }
        match next {
            Next::Wait(recovery) => {
                if lost_now {
                    self.recovery_notice(id, SandboxState::Destroyed, recovery)?;
                }
                Ok(Recovery::Waiting)
            }
            Next::Stop(recovery) => {
                self.stop_replacing(id, slot, recovery).await?;
                Ok(Recovery::Stopped)
            }
            Next::Replace(recovery) => {
                if lost_now {
                    self.recovery_notice(id, SandboxState::Destroyed, recovery)?;
                }
                idle.losses.replaced(now);
                self.try_replacing(id, slot, idle).await
            }
        }
    }

    /// Starts the replacement. One that can't start (a full disk fails its create) leaves a retry and a notice with
    /// its countdown, never neither: the run would hang (the full-disk P0, 3 Oct 2026).
    async fn try_replacing(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        idle: &mut super::idle::Idle,
    ) -> Result<Recovery, GatewayError> {
        let recovery = SandboxRecovery {
            losses: idle.losses.count(),
            retry_in_secs: Some(0),
        };
        match self.replace(id, recovery).await {
            Ok(replacement) => {
                idle.losses.created();
                Ok(Recovery::Replaced(replacement))
            }
            Err(GatewayError::Cancelled) => Err(GatewayError::Cancelled),
            // Only a create that failed (a full disk) is retried; a runtime that dies at start, a deleted task or a
            // store failure ends the run as it did before (candidate 14a).
            Err(error) if !create_failed(&error) => Err(error),
            Err(error) => {
                let retry = idle.losses.create_failed(idle.now());
                tracing::warn!(conversation_id = %id, code = error.code(), retry = ?retry,
                    "a replacement sandbox couldn't be created; trying again");
                match retry {
                    Retry::In(seconds) => {
                        self.recovery_notice(
                            id,
                            SandboxState::Destroyed,
                            SandboxRecovery {
                                losses: recovery.losses,
                                retry_in_secs: Some(seconds),
                            },
                        )?;
                        Ok(Recovery::Waiting)
                    }
                    // Overran: the next pass tries again at once.
                    Retry::Now => Ok(Recovery::Waiting),
                    Retry::Exhausted => self.stop_after_failed_creates(id, slot, idle).await,
                }
            }
        }
    }

    /// Whether the task is deleted, or purged entirely.
    async fn task_gone(&self, id: &ConversationId) -> Result<bool, GatewayError> {
        let id = crate::conversations::uuid(id)?;
        self.bounded(async {
            Ok(sqlx::query_scalar!(
                "SELECT state <> 'active' AS \"gone!\" FROM conversations WHERE id = $1",
                id
            )
            .fetch_optional(&self.services.pool)
            .await?
            .unwrap_or(true))
        })
        .await
    }

    /// Every retry of the replacement failed: the sandbox is retired as failed and the owner told, with a retryable
    /// code, that their next message resumes the run.
    async fn stop_after_failed_creates(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        idle: &mut super::idle::Idle,
    ) -> Result<Recovery, GatewayError> {
        idle.losses.created();
        let recovery = SandboxRecovery {
            losses: idle.losses.count(),
            retry_in_secs: None,
        };
        self.stop_replacing(id, slot, recovery).await?;
        Ok(Recovery::Stopped)
    }

    /// Forgets a lost instance: its token, whatever the engine still holds of it, and the slot's handle.
    async fn clear_lost(
        &self,
        slot: &Arc<Slot>,
        handle: &SandboxHandle,
    ) -> Result<(), GatewayError> {
        let _permit = self
            .bounded(async {
                slot.lifecycle
                    .acquire()
                    .await
                    .map_err(|_| GatewayError::Cancelled)
            })
            .await?;
        super::teardown_begun(&handle.conversation_id, super::Teardown::Lost);
        self.services.tokens.revoke(&handle.id.parse()?).await?;
        self.backend
            .engine
            .destroy(handle)
            .await
            .map_err(failure("destroy"))?;
        *slot
            .handle
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)? = None;
        Ok(())
    }

    async fn replace(
        self: &Arc<Self>,
        id: &ConversationId,
        recovery: SandboxRecovery,
    ) -> Result<SandboxHandle, GatewayError> {
        self.reset_record(crate::conversations::uuid(id)?).await?;
        self.recovery_notice(
            id,
            SandboxState::Creating,
            SandboxRecovery {
                retry_in_secs: None,
                ..recovery
            },
        )?;
        let began = tokio::time::Instant::now();
        let mut replacement = self.ensure_for(id, super::Create::Replacement).await?;
        let mut retried = false;
        loop {
            let ready = self
                .backend
                .engine
                .wait_ready(
                    &replacement,
                    self.services.links.as_ref(),
                    self.config.sandbox.ready(),
                    self.services.cancel.clone(),
                )
                .await;
            match ready {
                Ok(()) => return Ok(replacement),
                // Mid-task too, nothing is lost before the runtime registers: the replacement is started once more.
                Err(SandboxError::HostDied) if self.may_start_again(began, retried) => {
                    retried = true;
                    let slot = self.slot(id)?;
                    replacement = self.start_again(id, &slot, &replacement).await?;
                }
                Err(error) => return Err(failure("start")(error)),
            }
        }
    }

    /// Whether a start whose VM host died before its runtime registered may be made once more: the first death, within
    /// `create_budget_seconds` of the start (SBX-F-038, GW-I-227).
    pub(super) fn may_start_again(&self, began: tokio::time::Instant, retried: bool) -> bool {
        !retried && began.elapsed() < self.config.sandbox.create_budget()
    }

    /// Clears a start whose VM host died before its runtime registered and starts the sandbox again.
    pub(super) async fn start_again(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        dead: &SandboxHandle,
    ) -> Result<SandboxHandle, GatewayError> {
        tracing::warn!(conversation_id = %id, "the VM host died before its runtime registered; starting it once more");
        self.clear_lost(slot, dead).await?;
        self.reset_record(crate::conversations::uuid(id)?).await?;
        self.ensure_for(id, super::Create::StartAgain).await
    }

    /// Past the limit: the sandbox is retired as failed and the owner told, with a retryable code, that their next
    /// message resumes the run.
    async fn stop_replacing(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        recovery: SandboxRecovery,
    ) -> Result<(), GatewayError> {
        tracing::warn!(conversation_id = %id, losses = recovery.losses,
            "sandbox stopped unexpectedly too often; not replacing it until the owner resumes");
        self.services.telemetry.sandbox_retired("repeated_loss");
        self.mark_closing(id, None).await?;
        self.retire(id, slot, super::Teardown::RepeatedLoss).await?;
        self.services.events.control(
            id,
            ServerFrame::Error {
                seq: None,
                code: tqa_types::ErrorCode::SandboxFailed,
                message: format!(
                    "this task's sandbox stopped {} times in a few minutes, so it was not restarted; send a message \
                     to resume (if it keeps happening, the host's storage may be full)",
                    recovery.losses
                ),
                retryable: true,
                target: None,
            },
        )?;
        // A team's helper: its lead is told at once, since its unfinished run still reads as at work (GW-I-254).
        let conversation = crate::conversations::uuid(id)?;
        let pool = self.services.pool.clone();
        if let Err(error) = self
            .bounded(crate::teams::member_state::sandbox_failed(
                &pool,
                conversation,
                recovery.losses,
            ))
            .await
        {
            tracing::warn!(conversation_id = %id, code = error.code(), "the lead wasn't told its helper's sandbox failed");
        }
        self.recovery_notice(id, SandboxState::Failed, recovery)
    }

    fn recovery_notice(
        &self,
        id: &ConversationId,
        state: SandboxState,
        recovery: SandboxRecovery,
    ) -> Result<(), GatewayError> {
        self.notice(
            id,
            SandboxSummary {
                recovery: Some(recovery),
                ..SandboxSummary::new(self.config.backend, state)
            },
        )
    }

    pub(super) fn state_notice(
        &self,
        id: &ConversationId,
        state: SandboxState,
    ) -> Result<(), GatewayError> {
        self.notice(id, SandboxSummary::new(self.config.backend, state))
    }

    fn notice(&self, id: &ConversationId, sandbox: SandboxSummary) -> Result<(), GatewayError> {
        self.services.events.control(
            id,
            ServerFrame::SessionReady {
                conversation_id: id.clone(),
                resumed: true,
                sandbox,
                queue: None,
            },
        )
    }
    /// Reconciles labelled backend inventory with durable records, then resumes accepted work.
    /// Call during startup, before accepting HTTP traffic.
    ///
    /// # Errors
    /// Fails closed if inventory, cleanup or revocation cannot be confirmed.
    pub async fn reconcile(self: &Arc<Self>) -> Result<(), GatewayError> {
        // No create runs yet: a token reservation left now is an interrupted create's, and would fail its
        // conversation's next start until it expired (GW-I-252).
        let cleared = self.services.tokens.clear_reservations().await?;
        if cleared > 0 {
            tracing::info!(
                count = cleared,
                "runtime token reservations of interrupted creates cleared"
            );
        }
        // Networks first: a sandbox retired here is then found missing below, reset and resumed.
        Box::pin(self.reconcile_networks()).await?;
        let records=self.bounded(async {
            sqlx::query_as!(Record,"SELECT conversation_id,sandbox_id,phase FROM sandbox_instances ORDER BY conversation_id LIMIT 4097").fetch_all(&self.services.pool).await.map_err(GatewayError::from)
        }).await?;
        if records.len() > 4096 {
            return Err(GatewayError::Busy);
        }
        let live = self.live_inventory(&records).await?;
        for record in records {
            let id: ConversationId = record
                .conversation_id
                .to_string()
                .parse()
                .map_err(|_| GatewayError::StoreUnavailable)?;
            if record.phase == "destroying" {
                self.finish_retirement(&id, record.sandbox_id.as_deref())
                    .await?;
                continue;
            }
            let slot = self.slot(&id)?;
            let handle = live
                .iter()
                .find(|handle| {
                    record.sandbox_id.as_deref() == Some(&handle.id) && handle.conversation_id == id
                })
                .cloned();
            if handle.is_none() {
                self.revoke_record(record.sandbox_id.as_deref()).await?;
                self.reset_record(record.conversation_id).await?;
                self.state_notice(&id, tqa_types::SandboxState::Creating)?;
            }
            *slot
                .handle
                .lock()
                .map_err(|_| GatewayError::StoreUnavailable)? = handle;
            self.schedule(&id)?;
        }
        // Answers-024 Q2: an interrupted run is derived from the store, never a stored status.
        for id in self.services.events.interrupted_without_sandbox().await? {
            self.state_notice(&id, SandboxState::Creating)?;
            self.schedule(&id)?;
        }
        Ok(())
    }

    /// Completes a retirement a restart interrupted; its queued actions fail visibly, never silently. Unless it was a
    /// hand-over's: another task on the workspace has a turn queued, and this task's turns were waiting their turn
    /// behind it, so they stay queued for the admission pass, which serves the workspace's turns in order (GW-I-263).
    async fn finish_retirement(
        &self,
        id: &ConversationId,
        sandbox: Option<&str>,
    ) -> Result<(), GatewayError> {
        self.revoke_record(sandbox).await?;
        let conversation = crate::conversations::uuid(id)?;
        let handed_over = self
            .bounded(async {
                let mut connection = self.services.pool.acquire().await?;
                super::lease::others_queued(&mut connection, conversation).await
            })
            .await?;
        if handed_over {
            // Not this task's worker: its start would skip the turn order, which only the admission pass keeps
            // (GW-I-258).
            self.forget(id).await?;
            self.nudge_admission();
            return Ok(());
        }
        let cause = GatewayError::Sandbox {
            stage: "retirement",
            cause: "interrupted by a gateway restart",
        };
        let routes = self.fail_actions(id, &cause).await?;
        if !routes.is_empty() {
            self.report_failure(id, &routes, &cause).await?;
        }
        self.forget(id).await
    }

    async fn revoke_record(&self, sandbox: Option<&str>) -> Result<(), GatewayError> {
        if let Some(sandbox) = sandbox {
            self.services.tokens.revoke(&sandbox.parse()?).await?;
        }
        Ok(())
    }
    async fn live_inventory(&self, records: &[Record]) -> Result<Vec<SandboxHandle>, GatewayError> {
        let inventory = self
            .backend
            .engine
            .list()
            .await
            .map_err(failure("inventory"))?;
        let mut live = Vec::with_capacity(inventory.len());
        for handle in inventory {
            let owned = records.iter().any(|record| {
                record.conversation_id.to_string() == handle.conversation_id.as_str()
                    && record.sandbox_id.as_deref() == Some(&handle.id)
                    && record.phase == "live"
            });
            let running = if owned {
                match self.backend.engine.address(&handle).await {
                    Ok(_) => true,
                    Err(tqa_sandbox::SandboxError::NotFound) => false,
                    Err(error) => return Err(failure("inventory")(error)),
                }
            } else {
                false
            };
            if running {
                live.push(handle);
                continue;
            }
            super::teardown_begun(&handle.conversation_id, super::Teardown::Reconcile);
            self.services.tokens.revoke(&handle.id.parse()?).await?;
            self.backend
                .engine
                .destroy(&handle)
                .await
                .map_err(failure("destroy"))?;
            if owned {
                self.state_notice(&handle.conversation_id, SandboxState::Destroyed)?;
            }
        }
        Ok(live)
    }
}

/// Whether a replacement failed at its create, the one failure that is retried (GW-I-208): the backend couldn't make
/// the sandbox (a full disk, an engine past its create bound). Anything else is not a create's to retry.
pub(super) fn create_failed(error: &GatewayError) -> bool {
    matches!(
        error,
        GatewayError::Sandbox {
            stage: "create",
            ..
        }
    )
}
