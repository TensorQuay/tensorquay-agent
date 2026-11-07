//! The decision engine's two consultation points in a step (ADR-0013): the run state it sees, filtered by its history
//! exposure; its decision checked, journaled and applied; its fallbacks and circuit breaker; and the replay of a
//! journaled decision on resume.
use super::super::session::{Execution, Stop, millis};
use crate::{
    CoreError, Decision, ModelResponse, RunState, check_act, check_call_model, check_selection,
    valid_stop_reason,
};
use std::collections::BTreeSet;
use tqa_types::{
    DecisionChoice, DecisionExposure, DecisionFallbackReason, DecisionPoint, ErrorCode, Event,
    ToolCallId,
};

/// The most transcript an engine with exposure `full` is shown, in bytes of text.
const TRANSCRIPT_BYTES: usize = 64 * 1024;

/// A response as an engine with `exposure` may see it (ADR-0013 point 5): with `none`, calls by id and name only; with
/// `calls`, their arguments too; with `full`, everything.
fn exposed(response: &ModelResponse, exposure: DecisionExposure) -> ModelResponse {
    if exposure >= DecisionExposure::Full {
        return response.clone();
    }
    let mut shown = response.clone();
    shown.text.clear();
    shown.reasoning.clear();
    shown.reasoning_state.clear();
    if exposure < DecisionExposure::Calls {
        for call in &mut shown.tool_calls {
            call.arguments = "{}".into();
        }
    }
    shown
}

/// What a step journaled at `before_step` before a resume.
enum Journaled {
    /// Nothing: the engine is asked.
    None,
    /// A `call_model`: its tools, on its route.
    Applied(BTreeSet<String>),
    /// A fallback: the model's own decisions.
    FellBack,
    /// A `compact` and no answer after it: asked again, and another `compact` is refused.
    Compacted,
    /// An `act` whose call isn't journaled yet: the call is issued, without asking again.
    Acted {
        /// The tool.
        tool: String,
        /// Its arguments.
        arguments: serde_json::Value,
    },
}

/// How a step goes on after `before_step`.
pub(super) enum StepPlan {
    /// A model call with these tools, and whether the step fell back to the model's own decisions.
    Model(BTreeSet<String>, bool),
    /// The engine acted: the step's one call, already journaled.
    Acted(String),
}

/// `act` steps in a row after which another `act` is refused (ADR-0013 point 2).
const ACT_LOOP_AFTER: u32 = 3;

/// Fallbacks in a row after which the engine isn't asked again in the run (ADR-0013 point 5).
const CIRCUIT_AFTER: u32 = 3;

/// The fallback code of an engine failure.
fn failure_reason(failure: crate::EngineFailure) -> DecisionFallbackReason {
    match failure {
        crate::EngineFailure::Unreachable => DecisionFallbackReason::Unreachable,
        crate::EngineFailure::Timeout => DecisionFallbackReason::Timeout,
        crate::EngineFailure::Invalid => DecisionFallbackReason::Invalid,
        crate::EngineFailure::BudgetSpent => DecisionFallbackReason::BudgetSpent,
    }
}

impl Execution<'_> {
    pub(super) fn run_state(&self, attempt: u32) -> Result<RunState, Stop> {
        Ok(RunState {
            run_id: self.run_id()?,
            step: self
                .context
                .step
                .ok_or(CoreError::Internal("missing step"))?,
            attempt,
            route: self.route().name.clone(),
            routes: std::iter::once(self.runner.route.name.clone())
                .chain(
                    self.runner
                        .fallbacks
                        .iter()
                        .map(|fallback| fallback.route.name.clone()),
                )
                .collect(),
            tools: self
                .runner
                .tools
                .offered_specs()
                .map(|spec| spec.name.as_str().into())
                .collect(),
            budget: crate::BudgetUse {
                steps_used: self.steps_completed(),
                max_steps: self.runner.limits.max_steps,
                output_tokens_used: self.output_used(),
                max_output_tokens: self.runner.limits.max_output_tokens,
            },
            previous_calls: self.previous_calls(),
            transcript: self.transcript()?,
        })
    }
    /// The transcript for an engine with exposure `full`: the projected messages, at most the last 64 KiB of text.
    fn transcript(&self) -> Result<Option<Vec<crate::TranscriptLine>>, Stop> {
        if self.runner.decision.exposure() < DecisionExposure::Full {
            return Ok(None);
        }
        let mut lines = Vec::new();
        let mut bytes = 0;
        for message in tqa_context::project(&self.frames)?.into_iter().rev() {
            bytes += message.text.len();
            if bytes > TRANSCRIPT_BYTES {
                break;
            }
            lines.push(crate::TranscriptLine {
                role: format!("{:?}", message.role).to_lowercase(),
                text: message.text,
            });
        }
        lines.reverse();
        Ok(Some(lines))
    }
    /// Steps this run has completed, from its journal.
    fn steps_completed(&self) -> u32 {
        let completed = self
            .frames
            .iter()
            .filter(|frame| {
                matches!(frame, tqa_types::ServerFrame::Event {
                    run_id,
                    event: Event::StepCompleted { .. },
                    ..
                } if *run_id == self.context.run_id)
            })
            .count();
        u32::try_from(completed).unwrap_or(u32::MAX)
    }
    /// The previous step's calls in journal order, by name with their terminal status, from the journal.
    fn previous_calls(&self) -> Vec<crate::CallSummary> {
        let Some(previous) = self.context.step.and_then(|step| step.checked_sub(1)) else {
            return Vec::new();
        };
        let events = self.frames.iter().filter_map(|frame| match frame {
            tqa_types::ServerFrame::Event {
                run_id,
                step,
                event,
                ..
            } if *run_id == self.context.run_id && *step == Some(previous) => Some(event),
            _ => None,
        });
        let (mut calls, mut results) = (Vec::new(), Vec::new());
        for event in events {
            match event {
                Event::ToolCall {
                    call_id,
                    name,
                    arguments,
                } => calls.push((call_id, name, arguments)),
                Event::ToolResult {
                    call_id, status, ..
                } => results.push((call_id, *status)),
                _ => {}
            }
        }
        let with_arguments = self.runner.decision.exposure() >= DecisionExposure::Calls;
        calls
            .into_iter()
            .map(|(id, name, arguments)| crate::CallSummary {
                name: name.clone(),
                status: results
                    .iter()
                    .find(|(result, _)| *result == id)
                    .map(|(_, status)| *status),
                arguments: with_arguments.then(|| arguments.clone()),
            })
            .collect()
    }
    /// The step's tool subset from the engine's `before_step`, and whether the step fell back to the model's own
    /// decisions. A named engine's decision is checked against the step's limits and journaled before it takes
    /// effect; a `call_model` on another allowed route moves the step there (ADR-0013 point 2).
    pub(super) async fn tool_subset(&mut self, state: &RunState) -> Result<StepPlan, Stop> {
        let every = || StepPlan::Model(state.tools.iter().cloned().collect(), true);
        let Some(engine) = self.runner.decision.name().map(str::to_owned) else {
            let decision = self
                .watch()
                .wait(self.runner.decision.before_step(state))
                .await?;
            if let Ok(Ok((names, _))) = decision.map(|decision| check_call_model(state, decision)) {
                return Ok(StepPlan::Model(names, false));
            }
            tracing::warn!("invalid before-step decision; using model decisions");
            return Ok(every());
        };
        // A resumed step takes the decision it journaled and never asks again (ADR-0013 point 4).
        let mut compacted = false;
        match self.journaled_before_step(state)? {
            Journaled::None => {}
            Journaled::Applied(names) => return Ok(StepPlan::Model(names, false)),
            Journaled::FellBack => return Ok(every()),
            Journaled::Compacted => compacted = true,
            Journaled::Acted { tool, arguments } => {
                return self.issue_act(state, &tool, arguments).await;
            }
        }
        // An engine that doesn't answer before a step leaves it to the model, without a request or an event; its
        // `after_response` is still asked. Checked after the journal, so a decision already journaled is replayed.
        if !self.runner.decision.answers(DecisionPoint::BeforeStep) {
            return Ok(StepPlan::Model(
                state.tools.iter().cloned().collect(),
                false,
            ));
        }
        let (in_a_row, open) = self.circuit();
        if open {
            return Ok(every());
        }
        if in_a_row >= CIRCUIT_AFTER {
            self.fall_back(
                DecisionPoint::BeforeStep,
                &engine,
                DecisionFallbackReason::CircuitOpen,
            )
            .await?;
            return Ok(every());
        }
        // A `compact` compacts now and the engine is asked again, once per step (ADR-0013 point 2).
        loop {
            let (answer, latency_ms) = self.ask_before_step(state).await?;
            if !matches!(answer, Ok(Decision::Compact)) {
                return self
                    .answer_before_step(&engine, state, answer, latency_ms)
                    .await;
            }
            let ran = self
                .apply_compact(&engine, state.attempt, compacted, latency_ms)
                .await?;
            if !ran {
                return Ok(every());
            }
            compacted = true;
        }
    }
    /// An answer other than `compact`: a stop ends the run, anything else is applied as a `call_model` or falls back.
    async fn answer_before_step(
        &mut self,
        engine: &str,
        state: &RunState,
        answer: Result<Decision, crate::EngineFailure>,
        latency_ms: u64,
    ) -> Result<StepPlan, Stop> {
        match answer {
            Ok(Decision::Stop(reason)) => {
                self.apply_stop(engine, &reason, latency_ms).await?;
                Ok(StepPlan::Model(state.tools.iter().cloned().collect(), true))
            }
            Ok(Decision::Act(call)) => self.apply_act(engine, state, &call, latency_ms).await,
            other => {
                let (subset, fallback) = self
                    .apply_call_model(engine, state, other, latency_ms)
                    .await?;
                Ok(StepPlan::Model(subset, fallback))
            }
        }
    }
    /// An `act`: checked against the step's limits and the loop cap, journaled, then issued as the step's one call.
    async fn apply_act(
        &mut self,
        engine: &str,
        state: &RunState,
        call: &tqa_llm::ToolCall,
        latency_ms: u64,
    ) -> Result<StepPlan, Stop> {
        let checked = if self.acts_in_a_row() >= ACT_LOOP_AFTER {
            Err(DecisionFallbackReason::ActLoop)
        } else {
            check_act(state, call, |name, arguments| {
                self.runner.tools.prepare(name, arguments).is_ok()
            })
        };
        let arguments = match checked {
            Ok(arguments) => arguments,
            Err(reason) => {
                self.fall_back(DecisionPoint::BeforeStep, engine, reason)
                    .await?;
                return Ok(StepPlan::Model(state.tools.iter().cloned().collect(), true));
            }
        };
        self.emit(Event::Decision {
            point: DecisionPoint::BeforeStep,
            engine: engine.to_owned(),
            choice: DecisionChoice::Act {
                tool: call.name.clone(),
                arguments: arguments.clone(),
            },
            latency_ms,
        })
        .await?;
        self.issue_act(state, &call.name, arguments).await
    }
    /// Starts the step and journals the engine's call, named `de-<step>-<attempt>`, for the tool phase to run like any
    /// call.
    async fn issue_act(
        &mut self,
        state: &RunState,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<StepPlan, Stop> {
        let call_id = format!("de-{}-{}", state.step, state.attempt);
        let id: ToolCallId = call_id
            .parse()
            .map_err(|_| CoreError::Internal("invalid engine call id"))?;
        self.emit(Event::StepStarted {
            step: state.step,
            attempt: state.attempt,
        })
        .await?;
        self.emit(Event::ToolCall {
            call_id: id,
            name: tool.to_owned(),
            arguments,
        })
        .await?;
        Ok(StepPlan::Acted(call_id))
    }
    /// The run's steps in a row, just before this one, where the engine acted, from the journal.
    fn acts_in_a_row(&self) -> u32 {
        let Some(step) = self.context.step else {
            return 0;
        };
        let acted = |number: u32| {
            self.frames.iter().any(|frame| {
                matches!(frame, tqa_types::ServerFrame::Event {
                    run_id,
                    step: Some(frame_step),
                    event: Event::Decision {
                        point: DecisionPoint::BeforeStep,
                        choice: DecisionChoice::Act { .. },
                        ..
                    },
                    ..
                } if *run_id == self.context.run_id && *frame_step == number)
            })
        };
        let mut count = 0;
        let mut number = step;
        while number > 1 && acted(number - 1) {
            count += 1;
            number -= 1;
        }
        count
    }
    /// What this step journaled at `before_step`, for a resumed step: the last decision or fallback decides. A stop
    /// ends the run again; a `compact` with no answer after it means the engine is asked once more.
    fn journaled_before_step(&mut self, state: &RunState) -> Result<Journaled, Stop> {
        let last = self
            .step_events()
            .filter_map(|event| match event {
                Event::Decision {
                    point: DecisionPoint::BeforeStep,
                    choice,
                    ..
                } => Some(Some(choice.clone())),
                Event::DecisionFallback {
                    point: DecisionPoint::BeforeStep,
                    ..
                } => Some(None),
                _ => None,
            })
            .last();
        Ok(match last {
            None => Journaled::None,
            Some(Some(DecisionChoice::Compact)) => Journaled::Compacted,
            Some(Some(DecisionChoice::Act { tool, arguments })) => {
                Journaled::Acted { tool, arguments }
            }
            Some(Some(DecisionChoice::Stop { reason })) => {
                return Err(Stop::Failed(ErrorCode::DecisionStop, reason));
            }
            Some(Some(DecisionChoice::CallModel { route, tools })) => {
                self.active_route = self
                    .runner
                    .fallbacks
                    .iter()
                    .position(|fallback| fallback.route.name == route)
                    .map_or(0, |index| index + 1);
                Journaled::Applied(
                    tools
                        .into_iter()
                        .filter(|name| state.tools.contains(name))
                        .collect(),
                )
            }
            // A fallback, or a choice this point never records.
            Some(_) => Journaled::FellBack,
        })
    }
    /// The journaled selection of this step's response, for a resumed tool phase; `None` when there is none.
    pub(in crate::runner) fn journaled_selection(&self) -> Option<BTreeSet<String>> {
        self.step_events()
            .filter_map(|event| match event {
                Event::Decision {
                    point: DecisionPoint::AfterResponse,
                    choice: DecisionChoice::Select { call_ids },
                    ..
                } => Some(call_ids.iter().map(ToString::to_string).collect()),
                _ => None,
            })
            .last()
    }
    /// This run's events in the current step, in journal order.
    fn step_events(&self) -> impl Iterator<Item = &Event> {
        let (run, step) = (self.context.run_id.clone(), self.context.step);
        self.frames.iter().filter_map(move |frame| match frame {
            tqa_types::ServerFrame::Event {
                run_id,
                step: frame_step,
                event,
                ..
            } if *run_id == run && *frame_step == step => Some(event),
            _ => None,
        })
    }
    /// Asks the engine's `before_step`, with how long it took.
    async fn ask_before_step(
        &mut self,
        state: &RunState,
    ) -> Result<(Result<Decision, crate::EngineFailure>, u64), Stop> {
        let started = self.runner.clock.now();
        let answer = self
            .watch()
            .wait(self.runner.decision.before_step(state))
            .await?;
        Ok((
            answer,
            millis(self.runner.clock.now().saturating_duration_since(started)),
        ))
    }
    /// A `call_model` checked, journaled and applied, or the fallback with its code.
    async fn apply_call_model(
        &mut self,
        engine: &str,
        state: &RunState,
        answer: Result<Decision, crate::EngineFailure>,
        latency_ms: u64,
    ) -> Result<(BTreeSet<String>, bool), Stop> {
        let checked = answer
            .map_err(failure_reason)
            .and_then(|decision| check_call_model(state, decision));
        let (names, route) = match checked {
            Ok(applied) => applied,
            Err(reason) => {
                self.fall_back(DecisionPoint::BeforeStep, engine, reason)
                    .await?;
                return Ok((state.tools.iter().cloned().collect(), true));
            }
        };
        let tools = state
            .tools
            .iter()
            .filter(|name| names.contains(*name))
            .cloned()
            .collect();
        self.emit(Event::Decision {
            point: DecisionPoint::BeforeStep,
            engine: engine.to_owned(),
            choice: DecisionChoice::CallModel {
                route: route.clone(),
                tools,
            },
            latency_ms,
        })
        .await?;
        self.active_route = self
            .runner
            .fallbacks
            .iter()
            .position(|fallback| fallback.route.name == route)
            .map_or(0, |index| index + 1);
        Ok((names, false))
    }
    /// A `compact`: journaled, then run now. False after a fallback: a second `compact` in the step, or nothing old
    /// enough to fold.
    async fn apply_compact(
        &mut self,
        engine: &str,
        attempt: u32,
        again: bool,
        latency_ms: u64,
    ) -> Result<bool, Stop> {
        let plan = if again { None } else { self.compaction_plan()? };
        let Some(plan) = plan else {
            let reason = if again {
                DecisionFallbackReason::RepeatedCompact
            } else {
                DecisionFallbackReason::CompactNothing
            };
            self.fall_back(DecisionPoint::BeforeStep, engine, reason)
                .await?;
            return Ok(false);
        };
        self.emit(Event::Decision {
            point: DecisionPoint::BeforeStep,
            engine: engine.to_owned(),
            choice: DecisionChoice::Compact,
            latency_ms,
        })
        .await?;
        self.compact_now(&plan, attempt).await?;
        Ok(true)
    }
    /// A stop is journaled, then ends the run as failed; an engine can't declare a run completed (ADR-0013 point 2).
    /// A reason the journal can't carry is an `invalid` fallback, and the step goes on with the model's decisions.
    async fn apply_stop(
        &mut self,
        engine: &str,
        reason: &str,
        latency_ms: u64,
    ) -> Result<(), Stop> {
        if !valid_stop_reason(reason) {
            return self
                .fall_back(
                    DecisionPoint::BeforeStep,
                    engine,
                    DecisionFallbackReason::Invalid,
                )
                .await;
        }
        self.emit(Event::Decision {
            point: DecisionPoint::BeforeStep,
            engine: engine.to_owned(),
            choice: DecisionChoice::Stop {
                reason: reason.to_owned(),
            },
            latency_ms,
        })
        .await?;
        Err(Stop::Failed(ErrorCode::DecisionStop, reason.to_owned()))
    }
    /// Journals a fallback: the step uses the model's own decisions.
    async fn fall_back(
        &mut self,
        point: DecisionPoint,
        engine: &str,
        reason: DecisionFallbackReason,
    ) -> Result<(), Stop> {
        self.emit(Event::DecisionFallback {
            point,
            engine: engine.to_owned(),
            reason,
        })
        .await
        .map(|_| ())
    }
    /// This run's fallbacks since its last applied decision, and whether the engine is no longer asked (the circuit
    /// opened, or its budget was spent), from the journal, so a resumed run keeps the breaker's state (ADR-0013 point 4).
    fn circuit(&self) -> (u32, bool) {
        let mut in_a_row = 0;
        let mut open = false;
        for frame in &self.frames {
            let tqa_types::ServerFrame::Event { run_id, event, .. } = frame else {
                continue;
            };
            if *run_id != self.context.run_id {
                continue;
            }
            match event {
                Event::Decision { .. } => in_a_row = 0,
                // An open circuit, or a spent budget (asked again only by a new run), ends asking for this run.
                Event::DecisionFallback {
                    reason:
                        DecisionFallbackReason::CircuitOpen | DecisionFallbackReason::BudgetSpent,
                    ..
                } => open = true,
                Event::DecisionFallback { .. } => in_a_row += 1,
                _ => {}
            }
        }
        (in_a_row, open)
    }
    pub(super) async fn select_actions(
        &mut self,
        state: &RunState,
        response: &ModelResponse,
        fallback: bool,
    ) -> Result<BTreeSet<String>, Stop> {
        let original: BTreeSet<_> = response
            .tool_calls
            .iter()
            .map(|call| call.id.clone())
            .collect();
        // A step that fell back, a response without calls, or an engine that doesn't answer here leaves nothing for
        // the engine to select.
        if fallback
            || original.is_empty()
            || !self.runner.decision.answers(DecisionPoint::AfterResponse)
            || self.circuit().1
        {
            return Ok(original);
        }
        let shown = exposed(response, self.runner.decision.exposure());
        let planned = self
            .watch()
            .wait(self.runner.decision.after_response(state, &shown))
            .await?;
        let engine = self.runner.decision.name().map(str::to_owned);
        let checked = planned
            .map_err(failure_reason)
            .and_then(|planned| check_selection(&original, &planned));
        let Some(engine) = engine else {
            return Ok(checked.unwrap_or_else(|_| {
                tracing::warn!("invalid response plan; using model decisions");
                original
            }));
        };
        let selected = match checked {
            Ok(selected) => selected,
            Err(reason) => {
                self.fall_back(DecisionPoint::AfterResponse, &engine, reason)
                    .await?;
                return Ok(original);
            }
        };
        // The selection in the response's order, as the ids the journal uses.
        let call_ids: Option<Vec<ToolCallId>> = response
            .tool_calls
            .iter()
            .filter(|call| selected.contains(&call.id))
            .map(|call| call.id.parse().ok())
            .collect();
        let Some(call_ids) = call_ids else {
            self.fall_back(
                DecisionPoint::AfterResponse,
                &engine,
                DecisionFallbackReason::Invalid,
            )
            .await?;
            return Ok(original);
        };
        self.emit(Event::Decision {
            point: DecisionPoint::AfterResponse,
            engine,
            choice: DecisionChoice::Select { call_ids },
            latency_ms: 0,
        })
        .await?;
        Ok(selected)
    }
}
