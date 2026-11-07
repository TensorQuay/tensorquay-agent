//! Per-invocation ownership and the only event append path.
use super::{Runner, journal, watch::Watch};
use crate::{CoreError, EventContext, RunOutcome, SinkError, limits::LOCAL_PROJECTION_TIMESTAMP};
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tqa_context::{CappedResult, ContextError};
use tqa_tools::{ToolError, ToolOutput};
use tqa_types::{BudgetKind, ErrorCode, Event, RunId, ServerFrame, ToolStatus};

pub(super) enum Stop {
    Cancel,
    Shutdown,
    Failed(ErrorCode, String),
    Error(CoreError),
}
impl Stop {
    pub fn wall_clock() -> Self {
        Self::Failed(ErrorCode::Budget, "budget.wall_clock".into())
    }
}
impl From<CoreError> for Stop {
    fn from(error: CoreError) -> Self {
        Self::Error(error)
    }
}
impl From<tqa_context::ProjectionError> for Stop {
    fn from(error: tqa_context::ProjectionError) -> Self {
        CoreError::History(error).into()
    }
}

pub(super) struct Execution<'a> {
    pub runner: &'a Runner,
    pub frames: Vec<ServerFrame>,
    pub context: EventContext,
    pub cancel: CancellationToken,
    pub shutdown: CancellationToken,
    pub children: CancellationToken,
    /// Each started tool call's worker: the call, its result, and whether the screen before it was kept.
    pub jobs: JoinSet<(journal::Call, Result<ToolOutput, ToolError>, bool)>,
    pub spills: JoinSet<Result<CappedResult, ContextError>>,
    pub step_started: Option<Instant>,
    /// 0 for the conversation's own route, otherwise the 1-based fallback index.
    pub active_route: usize,
    /// Whether the active route was restored from this run's journal.
    pub route_synced: bool,
    /// The provider's own words, sanitised, for the `run.failed` of a provider failure (CORE-U-052).
    pub provider_message: Option<String>,
    /// The step whose link waits `link_waited` totals: the budget is per step (CORE-U-062).
    pub link_step: Option<u32>,
    pub link_waited: std::time::Duration,
    /// The one retry with the tools declared natively after a swallowed text-protocol reply (CORE-U-065).
    pub native_retry: super::model::NativeRetry,
}
impl<'a> Execution<'a> {
    pub fn new(
        runner: &'a Runner,
        history: &[ServerFrame],
        cancel: CancellationToken,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            runner,
            frames: history.to_vec(),
            context: EventContext::default(),
            cancel,
            shutdown,
            children: CancellationToken::new(),
            jobs: JoinSet::new(),
            spills: JoinSet::new(),
            step_started: None,
            active_route: 0,
            route_synced: false,
            provider_message: None,
            native_retry: super::model::NativeRetry::default(),
            link_step: None,
            link_waited: std::time::Duration::ZERO,
        }
    }
    pub fn run_id(&self) -> Result<RunId, Stop> {
        self.context
            .run_id
            .clone()
            .ok_or_else(|| CoreError::Internal("missing active run").into())
    }
    pub fn watch(&self) -> Watch {
        Watch {
            cancel: self.cancel.clone(),
            shutdown: self.shutdown.clone(),
            sleeper: self.runner.sleeper.clone(),
            remaining: self.wall_limit().saturating_sub(self.elapsed()),
        }
    }
    pub fn guard(&self) -> Result<(), Stop> {
        if self.shutdown.is_cancelled() {
            return Err(Stop::Shutdown);
        }
        if self.cancel.is_cancelled() {
            return Err(Stop::Cancel);
        }
        if self.context.run_id.is_some() && self.elapsed() >= self.wall_limit() {
            return Err(Stop::wall_clock());
        }
        Ok(())
    }
    pub fn elapsed(&self) -> Duration {
        let completed = self
            .frames
            .iter()
            .filter_map(|frame| match frame {
                ServerFrame::Event {
                    run_id,
                    event: Event::StepCompleted { duration_ms, .. },
                    ..
                } if *run_id == self.context.run_id => Some(Duration::from_millis(*duration_ms)),
                _ => None,
            })
            .fold(Duration::ZERO, Duration::saturating_add);
        completed.saturating_add(self.active_step_time())
    }
    pub fn active_step_time(&self) -> Duration {
        self.step_started.map_or(Duration::ZERO, |start| {
            self.runner.clock.now().saturating_duration_since(start)
        })
    }
    fn wall_limit(&self) -> Duration {
        Duration::from_secs(u64::from(self.runner.limits.wall_clock_secs))
    }
    pub async fn emit(&mut self, event: Event) -> Result<tqa_types::Seq, Stop> {
        self.guard()?;
        self.append(event).await
    }
    pub async fn append(&mut self, event: Event) -> Result<tqa_types::Seq, Stop> {
        if self.shutdown.is_cancelled() {
            return Err(Stop::Shutdown);
        }
        let seq = self
            .runner
            .sink
            .emit(event.clone(), self.context.clone())
            .await
            .map_err(CoreError::Sink)?;
        self.record(
            ServerFrame::Event {
                seq,
                run_id: self.context.run_id.clone(),
                step: self.context.step,
                ts: LOCAL_PROJECTION_TIMESTAMP.into(),
                event,
            },
            seq,
        )?;
        Ok(seq)
    }
    pub fn record(&mut self, frame: ServerFrame, seq: tqa_types::Seq) -> Result<(), Stop> {
        let previous = self.frames.iter().rev().find_map(journal::sequence);
        if seq == tqa_types::Seq::ZERO || previous.is_some_and(|value| value.next() != Some(seq)) {
            return Err(CoreError::Sink(SinkError {
                code: ErrorCode::SeqGap,
            })
            .into());
        }
        self.frames.push(frame);
        Ok(())
    }
    pub async fn resolve(
        &mut self,
        result: Result<RunOutcome, Stop>,
    ) -> Result<RunOutcome, CoreError> {
        let stop = match result {
            Ok(outcome) => return Ok(outcome),
            Err(stop) => stop,
        };
        self.children.cancel();
        let result = match stop {
            Stop::Shutdown => return Ok(RunOutcome::Interrupted),
            Stop::Error(error) => return Err(error),
            Stop::Cancel => self.close_run(None).await,
            Stop::Failed(code, reason) => self.close_run(Some((code, reason))).await,
        };
        match result {
            Ok(outcome) => Ok(outcome),
            Err(Stop::Shutdown) => Ok(RunOutcome::Interrupted),
            Err(Stop::Error(error)) => Err(error),
            Err(_) => Err(CoreError::Internal(
                "terminal append interrupted unexpectedly",
            )),
        }
    }
    async fn close_run(
        &mut self,
        failure: Option<(ErrorCode, String)>,
    ) -> Result<RunOutcome, Stop> {
        let run_id = self.run_id()?;
        let content = match &failure {
            Some((ErrorCode::Budget, reason)) => format!("stopped: {reason}"),
            Some(_) => "stopped: the run failed".into(),
            None => "stopped: the run was cancelled".into(),
        };
        let calls = journal::attempts(&self.frames)
            .into_iter()
            .filter(|attempt| attempt.context.run_id.as_ref() == Some(&run_id))
            .flat_map(|attempt| attempt.calls)
            .filter(|call| call.result.is_none())
            .collect::<Vec<_>>();
        for call in calls {
            self.close_call(&call, ToolStatus::Cancelled, &content)
                .await?;
        }
        self.context.run_id = Some(run_id.clone());
        let provider_message = self.provider_message.take();
        if let Some((code, reason)) = failure {
            self.append(Event::RunFailed {
                run_id,
                code,
                reason: reason.clone(),
                provider_message: provider_message.filter(|_| code == ErrorCode::ProviderError),
            })
            .await?;
            Ok(RunOutcome::Failed { code, reason })
        } else {
            self.append(Event::RunCancelled { run_id }).await?;
            Ok(RunOutcome::Cancelled)
        }
    }
    pub async fn close_call(
        &mut self,
        call: &journal::Call,
        status: ToolStatus,
        content: &str,
    ) -> Result<(), Stop> {
        self.context = call.context.clone();
        if call.started && !call.ended {
            self.append(Event::ToolEnd {
                call_id: call.id.clone(),
                status,
                exit_code: None,
                failure: None,
            })
            .await?;
        }
        self.append(Event::ToolResult {
            call_id: call.id.clone(),
            status,
            content: content.into(),
            target: None,
            evidence: None,
        })
        .await?;
        Ok(())
    }
    pub async fn warnings(&mut self) -> Result<(), Stop> {
        let completed = self.frames.iter().filter(|frame| matches!(frame,
            ServerFrame::Event { run_id, event: Event::StepCompleted { .. }, .. } if *run_id == self.context.run_id)).count();
        let elapsed = self.elapsed();
        let measures = [
            (
                BudgetKind::Steps,
                u64::try_from(completed).unwrap_or(u64::MAX),
                u64::from(self.runner.limits.max_steps),
            ),
            (
                BudgetKind::WallClock,
                millis(elapsed),
                u64::from(self.runner.limits.wall_clock_secs) * 1000,
            ),
        ];
        for (budget, used, limit) in measures {
            let warned = self.frames.iter().any(|frame| matches!(frame, ServerFrame::Event { run_id, event: Event::BudgetWarning { budget: previous, .. }, .. }
                if *run_id == self.context.run_id && *previous == budget));
            if warned || used.saturating_mul(5) < limit.saturating_mul(4) {
                continue;
            }
            let divisor = if budget == BudgetKind::WallClock {
                1000
            } else {
                1
            };
            let (used, limit) = (used / divisor, limit / divisor);
            self.emit(Event::BudgetWarning {
                budget,
                used,
                limit,
            })
            .await?;
        }
        Ok(())
    }
    pub async fn join_workers(&mut self) {
        self.children.cancel();
        while self.jobs.join_next().await.is_some() {}
        while self.spills.join_next().await.is_some() {}
    }
}
pub(super) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
