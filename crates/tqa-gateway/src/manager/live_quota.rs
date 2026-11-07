//! The live workspace quota on Docker (N4): while a run is in progress its workspace and its
//! conversation's runtime state directory are measured with a bounded walk on the blocking pool, scheduled by
//! `live_usage`. Past the quota the owner is told once and the run continues; past the quota plus its margin (or the
//! state directory past `sandbox.state_bytes`) the run is cancelled with a typed notice, and a runtime that does not
//! stop is retired without a replacement. The workspace is never deleted or truncated. Firecracker's block image is its
//! own hard bound, so it is not measured here.
use super::{
    SandboxManager, Slot,
    idle::Idle,
    live_usage::{Sample, hard_limit, next_due},
};
use crate::GatewayError;
use std::{
    path::PathBuf,
    sync::{Arc, PoisonError},
    time::{Duration, Instant},
};
use tqa_types::{ClientFrame, ConversationId, ErrorCode, RunId, ServerFrame};

/// Live measurements running at once, gateway-wide.
pub(super) const CONCURRENT_MEASUREMENTS: usize = 2;

/// What one measurement found.
#[derive(Clone, Copy, Debug)]
enum Found {
    /// Both within their hard limits; the workspace's charge.
    Within(u64),
    /// The workspace past the quota plus its margin.
    WorkspaceOver,
    /// The state directory past `sandbox.state_bytes`.
    StateOver,
    /// The walk failed; measured again at the interval.
    Failed,
}

/// The live quota's state for one sandbox.
#[derive(Default)]
pub(super) struct LiveState {
    /// The workspace's id and both directories, once looked up.
    paths: Option<(uuid::Uuid, PathBuf, PathBuf)>,
    /// The workspace's own quota, looked up with its paths (the environment sizes design).
    quota: Option<u64>,
    due: Option<Instant>,
    measuring: bool,
    last: Option<Sample>,
    previous: Option<Sample>,
    cost: Duration,
    /// A finished measurement the next pass acts on.
    found: Option<(Found, Instant)>,
    /// The run the over-quota notice was sent for (once per run).
    noticed: Option<RunId>,
    /// The run that was cancelled, and when.
    stopped: Option<(RunId, Instant)>,
    /// A run has been active since the last measurement after one ended: the next pass without a run measures once
    /// more (GW-I-200).
    ran: bool,
}

impl LiveState {
    fn lock(slot: &Slot) -> std::sync::MutexGuard<'_, Self> {
        slot.usage.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A finished measurement, once: it becomes the latest sample (when it has a size) and sets when the next is due.
    fn take_found(&mut self, interval: Duration, hard: u64) -> Option<Found> {
        let (found, at) = self.found.take()?;
        if let Found::Within(bytes) = found {
            self.previous = self.last;
            self.last = Some(Sample { at, bytes });
        }
        let wait = self.last.map_or(interval, |last| {
            next_due(interval, hard, last, self.previous, self.cost)
        });
        self.due = Some(at + wait);
        Some(found)
    }

    /// Whether `run` gets the over-quota notice now: the first time for each run.
    fn first_notice(&mut self, run: &RunId) -> bool {
        let first = self.noticed.as_ref() != Some(run);
        self.noticed = Some(run.clone());
        first
    }

    /// Whether `run` is stopped now: not when it already was.
    fn first_stop(&mut self, run: &RunId) -> bool {
        if self
            .stopped
            .as_ref()
            .is_some_and(|(stopped, _)| stopped == run)
        {
            return false;
        }
        self.stopped = Some((run.clone(), Instant::now()));
        true
    }

    fn workspace(&self) -> Option<uuid::Uuid> {
        self.paths.as_ref().map(|(workspace, ..)| *workspace)
    }
}

/// Measures the workspace within `hard`, then the state directory within `state_bytes` (blocking).
fn measure(
    workspace: &std::path::Path,
    state: &std::path::Path,
    hard: u64,
    state_bytes: u64,
) -> Found {
    let bytes = match crate::workspaces::charged_within(workspace, hard) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Found::WorkspaceOver,
        Err(_) => return Found::Failed,
    };
    match crate::workspaces::charged_within(state, state_bytes) {
        Ok(Some(_)) => Found::Within(bytes),
        Ok(None) => Found::StateOver,
        Err(_) => Found::Failed,
    }
}

impl SandboxManager {
    /// The workspace's own quota once known; the installation's default until the first lookup.
    fn quota(&self, slot: &Slot) -> u64 {
        LiveState::lock(slot)
            .quota
            .unwrap_or(self.config.workspaces.quota_bytes)
    }

    fn hard_limit(&self, slot: &Slot) -> u64 {
        hard_limit(self.quota(slot), self.config.workspaces.margin_percent)
    }

    /// One pass of the live quota for a sandbox. `Ok(true)` when it retired the sandbox (the worker ends).
    ///
    /// # Errors
    /// Store failures; a failed walk is only measured again.
    pub(super) async fn live_quota(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        idle: &Idle,
    ) -> Result<bool, GatewayError> {
        let Some(interval) = self.config.workspaces.live_check else {
            return Ok(false);
        };
        if self.config.uses_block_images() {
            return Ok(false);
        }
        let Some(run) = idle.active().cloned() else {
            self.after_run(id, slot, interval).await?;
            return Ok(false);
        };
        LiveState::lock(slot).ran = true;
        let now = Instant::now();
        // Read before the lock below: `hard_limit` takes the same lock.
        let hard = self.hard_limit(slot);
        let (found, stopped) = {
            let mut state = LiveState::lock(slot);
            (state.take_found(interval, hard), state.stopped.clone())
        };
        if let Some(found) = found {
            self.act(id, slot, &run, found).await?;
        }
        // The backstop: a runtime that has not ended the cancelled run within two I/O timeouts loses its sandbox.
        if let Some((stopped, at)) = stopped
            && stopped == run
            && now.saturating_duration_since(at) > self.config.io_timeout().saturating_mul(2)
        {
            self.retire_over_quota(id, slot).await?;
            return Ok(true);
        }
        self.measure_if_due(id, slot, now).await?;
        Ok(false)
    }

    /// With no run active: a measurement that finished is recorded (no quota action without a run), and once after a
    /// run has ended the workspace is measured again, so a kept, idle environment's workspace shows what the run left,
    /// not what the run's first measurement found (GW-I-200).
    async fn after_run(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        interval: Duration,
    ) -> Result<(), GatewayError> {
        // Read before the lock below: `hard_limit` takes the same lock.
        let hard = self.hard_limit(slot);
        let (found, workspace, measure) = {
            let mut state = LiveState::lock(slot);
            state.stopped = None;
            let found = state.take_found(interval, hard);
            // One in flight may have started before the run's last writes: measure after it, not instead of it.
            let measure = state.ran && !state.measuring;
            if measure {
                state.ran = false;
                state.due = None;
            }
            (found, state.workspace(), measure)
        };
        if let (Some(Found::Within(bytes)), Some(workspace)) = (found, workspace) {
            self.record_size(workspace, bytes).await?;
        }
        if measure {
            self.measure_if_due(id, slot, Instant::now()).await?;
        }
        Ok(())
    }

    async fn act(
        &self,
        id: &ConversationId,
        slot: &Arc<Slot>,
        run: &RunId,
        found: Found,
    ) -> Result<(), GatewayError> {
        let quota = self.quota(slot);
        let workspace = LiveState::lock(slot).workspace();
        match found {
            Found::Failed => Ok(()),
            Found::Within(bytes) => {
                if let Some(workspace) = workspace {
                    self.record_size(workspace, bytes).await?;
                }
                if bytes > quota && LiveState::lock(slot).first_notice(run) {
                    self.over_quota_notice(id, (bytes, quota))?;
                }
                Ok(())
            }
            Found::WorkspaceOver | Found::StateOver => {
                if !LiveState::lock(slot).first_stop(run) {
                    return Ok(());
                }
                let workspace_over = matches!(found, Found::WorkspaceOver);
                // New runs are refused from now on (the admission check reads the recorded size).
                if workspace_over && let Some(workspace) = workspace {
                    self.record_size(workspace, self.hard_limit(slot).saturating_add(1))
                        .await?;
                }
                self.stop_run(id, run, workspace_over).await
            }
        }
    }

    /// Tells the owner, once per run, that the workspace is over its quota while the run continues.
    fn over_quota_notice(
        &self,
        id: &ConversationId,
        (bytes, quota): (u64, u64),
    ) -> Result<(), GatewayError> {
        tracing::warn!(conversation_id = %id, bytes, quota, "a running workspace is over its quota");
        self.services.events.control(
            id,
            ServerFrame::Error {
                seq: None,
                code: ErrorCode::WorkspaceQuota,
                message: "this workspace is over its quota; the run continues for now, but it is stopped if the \
                          workspace grows much further, and new runs start only once files are removed"
                    .into(),
                retryable: true,
                target: None,
            },
        )
    }

    /// Cancels `run` and tells the owner why.
    async fn stop_run(
        &self,
        id: &ConversationId,
        run: &RunId,
        workspace_over: bool,
    ) -> Result<(), GatewayError> {
        let (code, message) = if workspace_over {
            (
                ErrorCode::WorkspaceQuota,
                "the run was stopped: its workspace grew past its quota and margin; remove files to run again",
            )
        } else {
            (
                ErrorCode::StateQuota,
                "the run was stopped: its conversation's runtime state grew past its bound; an administrator must \
                 intervene",
            )
        };
        tracing::warn!(conversation_id = %id, run_id = %run, code = code.as_str(), "stopping a run over its quota");
        let cancel = ClientFrame::RunCancel {
            run_id: run.clone(),
        };
        if let Err(error) = self.bounded(self.services.links.deliver(id, cancel)).await {
            // No link now: the backstop retires the sandbox if the run goes on.
            tracing::debug!(conversation_id = %id, error_code = error.code(), "the run's cancel was not delivered");
        }
        self.services.events.control(
            id,
            ServerFrame::Error {
                seq: None,
                code,
                message: message.into(),
                retryable: false,
                target: None,
            },
        )
    }

    /// Retires a sandbox whose runtime did not stop its over-quota run, without a replacement.
    async fn retire_over_quota(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
    ) -> Result<(), GatewayError> {
        tracing::warn!(conversation_id = %id, "retiring a sandbox whose run did not stop over its quota");
        self.mark_closing(id, None).await?;
        self.retire(id, slot, super::Teardown::Quota).await?;
        self.state_notice(id, tqa_types::SandboxState::Failed)
    }

    async fn record_size(&self, workspace: uuid::Uuid, bytes: u64) -> Result<(), GatewayError> {
        let size = i64::try_from(bytes).unwrap_or(i64::MAX);
        self.bounded(async {
            sqlx::query!(
                "UPDATE workspaces SET size_bytes=$2, size_measured_at=CURRENT_TIMESTAMP WHERE id=$1 AND state='ready'",
                workspace,
                size
            )
            .execute(&self.services.pool)
            .await?;
            Ok(())
        })
        .await
    }

    /// The conversation's workspace and state directories, looked up once per sandbox.
    async fn live_paths(
        &self,
        id: &ConversationId,
        slot: &Slot,
    ) -> Result<(PathBuf, PathBuf), GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        let row = self
            .bounded(async {
                Ok(sqlx::query!(
                    "SELECT c.workspace_id, w.quota_bytes FROM conversations c JOIN workspaces w ON w.id=c.workspace_id WHERE c.id=$1",
                    conversation
                )
                .fetch_optional(&self.services.pool)
                .await?)
            })
            .await?
            .ok_or(GatewayError::NotFound)?;
        let workspace = row.workspace_id;
        LiveState::lock(slot).quota = Some(crate::workspaces::quota(&self.config, row.quota_bytes));
        let (workspace_path, state_path) = (
            self.services.storage.workspace_path(workspace),
            self.services.storage.runtime_state_path(conversation),
        );
        LiveState::lock(slot).paths = Some((workspace, workspace_path.clone(), state_path.clone()));
        Ok((workspace_path, state_path))
    }

    /// The screenshot bytes the task keeps in the gateway's store.
    async fn screenshots_held(&self, id: &ConversationId) -> Result<u64, GatewayError> {
        let conversation = crate::conversations::uuid(id)?;
        let held = self
            .bounded(async {
                Ok(sqlx::query_scalar!(
                    "SELECT COALESCE(SUM(bytes), 0)::bigint AS \"held!\" FROM screenshots WHERE conversation_id = $1 AND purged_at IS NULL",
                    conversation
                )
                .fetch_one(&self.services.pool)
                .await?)
            })
            .await?;
        Ok(u64::try_from(held).unwrap_or(0))
    }

    /// Starts a measurement on the blocking pool when one is due and none is running; its result comes back through
    /// the slot and wakes the worker.
    async fn measure_if_due(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        now: Instant,
    ) -> Result<(), GatewayError> {
        let (due, known) = {
            let state = LiveState::lock(slot);
            (
                !state.measuring && state.due.is_none_or(|due| now >= due),
                state.paths.clone(),
            )
        };
        if !due {
            return Ok(());
        }
        let (workspace_path, state_path) = match known {
            Some((_, workspace, state)) => (workspace, state),
            None => self.live_paths(id, slot).await?,
        };
        // One quota for the run's state and the task's kept screenshots (ADR-0039 section 9): the directory gets what
        // the screenshots leave.
        let screenshots = self.screenshots_held(id).await?;
        LiveState::lock(slot).measuring = true;
        let hard = self.hard_limit(slot);
        let state_bytes = self.config.sandbox.state_bytes.saturating_sub(screenshots);
        let permits = Arc::clone(&self.measurements);
        let slot = Arc::clone(slot);
        self.services.tasks.spawn(async move {
            let started = Instant::now();
            let found = match permits.acquire_owned().await {
                Ok(_permit) => tokio::task::spawn_blocking(move || {
                    measure(&workspace_path, &state_path, hard, state_bytes)
                })
                .await
                .unwrap_or(Found::Failed),
                Err(_) => Found::Failed,
            };
            let finished = Instant::now();
            {
                let mut state = LiveState::lock(&slot);
                state.measuring = false;
                state.cost = finished.saturating_duration_since(started);
                state.found = Some((found, finished));
            }
            slot.notify.notify_one();
        });
        Ok(())
    }
}
