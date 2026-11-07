//! Journal-directed startup and step transitions; acknowledged work is never inferred from caches.
use super::{
    UserTurn, journal,
    session::{Execution, Stop, millis},
};
use crate::{CoreError, EventContext, RunOutcome};
use std::collections::BTreeSet;
use tqa_context::ResumePoint;
use tqa_types::{ErrorCode, Event, ToolStatus};

impl Execution<'_> {
    pub(super) async fn execute(
        &mut self,
        point: ResumePoint,
        message: Option<UserTurn>,
    ) -> Result<RunOutcome, Stop> {
        if self.shutdown.is_cancelled() {
            return Err(Stop::Shutdown);
        }
        // A run parked for an approval before ADR-0035 has nothing to wait for; its waiting call never runs.
        if let ResumePoint::NextStep { run_id, .. }
        | ResumePoint::ToolPhase { run_id, .. }
        | ResumePoint::RetryModel { run_id, .. } = &point
            && super::journal::ends_parked(&self.frames)
        {
            self.context.run_id = Some(run_id.clone());
            return Err(Stop::Failed(
                tqa_types::ErrorCode::ApprovalsRemoved,
                "approvals were removed; send a message to continue".into(),
            ));
        }
        let (step, attempt, tools) = match point {
            ResumePoint::Idle { .. } => {
                self.close_dangling().await?;
                let Some(message) = message else {
                    return Ok(RunOutcome::Idle);
                };
                self.context = EventContext::default();
                let seq = self.append(super::brief::message_event(message)?).await?;
                let run_id = super::run_id_for_message(seq)?;
                self.context.run_id = Some(run_id);
                self.append(Event::RunStarted {
                    run_id: self.run_id()?,
                })
                .await?;
                self.context_loaded().await?;
                (1, 1, false)
            }
            ResumePoint::StartRun { run_id } => {
                self.context.run_id = Some(run_id.clone());
                self.append(Event::RunStarted { run_id }).await?;
                self.context_loaded().await?;
                (1, 1, false)
            }
            ResumePoint::NextStep { run_id, step } => {
                self.context.run_id = Some(run_id);
                self.resumed(step).await?;
                if self.previous_was_final() {
                    return self.complete_run().await;
                }
                self.guard()?;
                self.warnings().await?;
                self.step_budget()?;
                self.check_progress().await?;
                (step, 1, false)
            }
            ResumePoint::RetryModel {
                run_id,
                step,
                attempt,
            } => {
                self.context = EventContext {
                    run_id: Some(run_id),
                    step: Some(step),
                };
                self.resumed(step).await?;
                (step, attempt, false)
            }
            ResumePoint::ToolPhase { run_id, step, .. } => {
                self.context = EventContext {
                    run_id: Some(run_id),
                    step: Some(step),
                };
                self.resumed(step).await?;
                self.recover_tools().await?;
                (step, self.current_attempt()?.number, true)
            }
        };
        self.advance_steps(step, attempt, tools).await
    }
    async fn advance_steps(
        &mut self,
        mut step: u32,
        mut attempt: u32,
        mut tool_phase: bool,
    ) -> Result<RunOutcome, Stop> {
        loop {
            self.guard()?;
            self.context.step = Some(step);
            self.step_started = Some(self.runner.clock.now());
            self.repeated_failure()?;
            // A resumed tool phase keeps the engine's journaled selection (ADR-0013 point 4).
            let selected = if tool_phase {
                match self.journaled_selection() {
                    Some(selected) => selected,
                    None => self
                        .current_attempt()?
                        .calls
                        .iter()
                        .map(|call| call.id.to_string())
                        .collect::<BTreeSet<_>>(),
                }
            } else {
                self.model_phase(step, attempt).await?
            };
            let has_calls = !self.current_attempt()?.calls.is_empty();
            self.tool_phase(&selected).await?;
            self.finish_step(step).await?;
            if !has_calls {
                return self.complete_run().await;
            }
            self.check_progress().await?;
            self.warnings().await?;
            self.step_budget()?;
            self.guard()?;
            step = step
                .checked_add(1)
                .ok_or(CoreError::Internal("step counter exhausted"))?;
            attempt = 1;
            tool_phase = false;
        }
    }
    async fn resumed(&mut self, from_step: u32) -> Result<(), Stop> {
        self.append(Event::RunResumed {
            run_id: self.run_id()?,
            from_step,
        })
        .await?;
        self.context_loaded().await
    }
    /// Reports what this process loaded into the instruction slot, once per start or resume of a run.
    async fn context_loaded(&mut self) -> Result<(), Stop> {
        if let Some(loaded) = &self.runner.prompt.loaded {
            self.append(loaded.clone().into_event()).await?;
        }
        Ok(())
    }
    async fn finish_step(&mut self, step: u32) -> Result<(), Stop> {
        self.emit(Event::StepCompleted {
            step,
            duration_ms: millis(self.active_step_time()),
        })
        .await?;
        self.step_started = None;
        Ok(())
    }
    async fn complete_run(&mut self) -> Result<RunOutcome, Stop> {
        if self.shutdown.is_cancelled() {
            return Err(Stop::Shutdown);
        }
        if self.cancel.is_cancelled() {
            return Err(Stop::Cancel);
        }
        self.append(Event::RunCompleted {
            run_id: self.run_id()?,
        })
        .await?;
        Ok(RunOutcome::Completed)
    }
    fn step_budget(&self) -> Result<(), Stop> {
        let completed = self
            .frames
            .iter()
            .filter(|frame| {
                matches!(frame, tqa_types::ServerFrame::Event {
            run_id, event: Event::StepCompleted { .. }, .. } if *run_id == self.context.run_id)
            })
            .count();
        if completed >= self.runner.limits.max_steps as usize {
            return Err(Stop::Failed(
                ErrorCode::Budget,
                "the run's step budget was reached".into(),
            ));
        }
        Ok(())
    }
    fn previous_was_final(&self) -> bool {
        journal::attempts(&self.frames)
            .iter()
            .rev()
            .find(|attempt| attempt.context.run_id == self.context.run_id)
            .is_some_and(|attempt| attempt.complete && attempt.calls.is_empty())
    }
    async fn close_dangling(&mut self) -> Result<(), Stop> {
        let calls = journal::attempts(&self.frames)
            .into_iter()
            .filter(|attempt| attempt.complete)
            .flat_map(|attempt| attempt.calls)
            .filter(|call| call.result.is_none())
            .collect::<Vec<_>>();
        for call in calls {
            self.close_call(&call, ToolStatus::Interrupted, &interrupted(&call))
                .await?;
        }
        Ok(())
    }
    async fn recover_tools(&mut self) -> Result<(), Stop> {
        for call in self.current_attempt()?.calls {
            if call.started && call.result.is_none() {
                self.guard()?;
                self.close_call(&call, ToolStatus::Interrupted, &interrupted(&call))
                    .await?;
            }
        }
        Ok(())
    }
}
fn interrupted(call: &journal::Call) -> String {
    if call.started && call.name == "git_push" {
        tqa_tools::ToolError::from(tqa_types::GitPushFailure::OutcomeUnknown).model_message()
    } else {
        "the sandbox stopped while this tool ran; it may be retried".into()
    }
}
