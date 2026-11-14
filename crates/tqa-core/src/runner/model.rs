//! One bounded stream per journalled attempt; retries retain their original attempt numbers.
mod decisions;
mod fallback;
mod images;
mod pictures;
pub(super) mod request;
mod screenshots;
mod states;
mod swallowed;
use super::{
    journal,
    output::{OutputMeter, output_exhausted},
    routes,
    session::{Execution, Stop, millis},
};
use crate::{
    CoreError, LinkSettle, RunState,
    limits::{MODEL_ATTEMPTS, RETRY_BASE, RETRY_CAP, THROTTLE_REPORT_EVERY},
};
use decisions::StepPlan;
use futures::StreamExt;
use std::{
    collections::BTreeSet,
    sync::PoisonError,
    time::{Duration, Instant},
};
pub(in crate::runner) use swallowed::NativeRetry;
use tqa_llm::{
    Completion, FinishReason, LlmError, RetryPolicy, StreamAssembler, StreamChunk, StreamLimits,
    ThrottleScope,
};
use tqa_types::{ErrorCode, Event, ToolCallId};

/// The retry reason of an attempt given back because the runtime's link to the gateway was down (CORE-U-062).
const GATEWAY_LINK: &str = "gateway_link";

enum AttemptError {
    Stop(Stop),
    Provider(LlmError),
    /// A text-protocol reply that could not be read as tool calls, and what exactly was wrong when that is known.
    Fallback(
        tqa_prompt::FallbackFailure,
        Option<tqa_types::FallbackProblem>,
    ),
    /// The step asks again at this attempt: after an unreadable text-protocol reply, or a swallowed one (CORE-U-065).
    Again(u32),
}
impl From<Stop> for AttemptError {
    fn from(error: Stop) -> Self {
        Self::Stop(error)
    }
}
impl From<LlmError> for AttemptError {
    fn from(error: LlmError) -> Self {
        Self::Provider(error)
    }
}
impl Execution<'_> {
    pub(super) async fn model_phase(
        &mut self,
        step: u32,
        first_attempt: u32,
    ) -> Result<BTreeSet<String>, Stop> {
        self.sync_route().await?;
        // The engine is asked once per step; the step's retries and a failover reuse its decision (ADR-0013 point 1).
        self.context.step = Some(step);
        let state = self.run_state(first_attempt)?;
        let plan = match self.tool_subset(&state).await? {
            // The engine acted: the step's one call is journaled, and there is no model call (ADR-0013 point 2).
            StepPlan::Acted(call_id) => return Ok(BTreeSet::from([call_id])),
            StepPlan::Model(subset, fallback) => (subset, fallback),
        };
        let mut attempt = first_attempt;
        // Each pass that returns `None` has moved to the next fallback route, so this is bounded.
        loop {
            if let Some(actions) = self
                .route_attempts(step, &mut attempt, &state, &plan)
                .await?
            {
                return Ok(actions);
            }
        }
    }
    /// Model attempts on the active route; `None` after switching to the next route.
    async fn route_attempts(
        &mut self,
        step: u32,
        attempt: &mut u32,
        state: &RunState,
        (subset, fallback): &(BTreeSet<String>, bool),
    ) -> Result<Option<BTreeSet<String>>, Stop> {
        let mut empties = 0;
        // An answer that couldn't be read gets one more try, as a cut stream does; a second fails the run (CORE-E-040).
        let mut malformed = 0;
        let mut throttles = Throttles::default();
        let mut local_attempt = 0;
        while local_attempt < MODEL_ATTEMPTS {
            local_attempt += 1;
            self.remaining_output()?;
            if local_attempt == 1 {
                self.compact_if_needed(*attempt).await?;
            }
            self.context.step = Some(step);
            self.emit(Event::StepStarted {
                step,
                attempt: *attempt,
            })
            .await?;
            let result = self.response_attempt(subset, *attempt).await;
            let (error, empty) = match result {
                Ok(Some(response)) => {
                    return self
                        .select_actions(state, &response, *fallback)
                        .await
                        .map(Some);
                }
                Ok(None) => (LlmError::Protocol("empty_response".into()), true),
                Err(AttemptError::Provider(error)) => (error, false),
                Err(AttemptError::Stop(interruption)) => return Err(interruption),
                Err(AttemptError::Fallback(failure, problem)) => {
                    self.fallback_retry(step, attempt, failure, problem).await?;
                    continue;
                }
                Err(AttemptError::Again(next)) => {
                    *attempt = next;
                    continue;
                }
            };
            // The relay's own limit spends no attempt: only cancellation and the wall clock bound the wait.
            let switched = if let LlmError::Throttled { retry_after, scope } = error {
                local_attempt -= 1;
                let throttle = (retry_after, scope, provider_reason(&error));
                Some(self.throttled(throttle, &mut throttles, attempt).await?)
            } else {
                throttles.in_a_row = 0;
                None
            };
            match switched {
                Some(true) => return Ok(None),
                Some(false) => continue,
                None => {}
            }
            if self
                .link_back(step, empty, (attempt, &mut local_attempt), &error)
                .await?
            {
                continue;
            }
            empties += u32::from(empty);
            if empty && empties >= self.empty_tries() {
                return Err(Stop::Failed(ErrorCode::ProviderError, self.empty_reason()));
            }
            self.malformed_again(&error, &mut malformed)?;
            let reason = if empty {
                "empty_response".into()
            } else {
                provider_reason(&error)
            };
            let delay = self.retry_delay(local_attempt, &error, empty)?;
            let switch = !empty && routes::should_switch(&error, delay.is_none());
            if switch && self.switch_route(reason.clone()).await? {
                *attempt = next_attempt(*attempt)?;
                return Ok(None);
            }
            let Some(delay) = delay else {
                return Err(self.provider_failure(&error, reason));
            };
            // The same reason the switch and the failure use: an empty answer is `empty_response`, not `protocol`.
            self.retry_after(delay, attempt, reason, &error).await?;
        }
        Err(CoreError::Internal("retry budget escaped its bound").into())
    }
    async fn response_attempt(
        &mut self,
        subset: &BTreeSet<String>,
        attempt: u32,
    ) -> Result<Option<Completion>, AttemptError> {
        let mut meter = OutputMeter::default();
        let response = match self.stream_attempt(subset, attempt, &mut meter).await {
            Ok(response) => response,
            // A server that refuses the tools the one retry declared: the route stays text-only (CORE-U-065).
            Err(AttemptError::Provider(error))
                if let Some(next) = self.native_refused(attempt, &error).await? =>
            {
                self.record_usage(&meter).await?;
                return Err(AttemptError::Again(next));
            }
            Err(error) => {
                if matches!(
                    &error,
                    AttemptError::Provider(_)
                        | AttemptError::Fallback(..)
                        | AttemptError::Stop(Stop::Cancel | Stop::Failed(..))
                ) {
                    self.record_usage(&meter).await?;
                }
                return Err(error);
            }
        };
        let step = self.context.step.unwrap_or_default();
        if let Some(withheld) = self.swallowed(&response, step) {
            self.record_usage(&meter).await?;
            return Err(AttemptError::Again(
                self.native_retry(step, attempt, withheld).await?,
            ));
        }
        if response.text.trim().is_empty() && response.tool_calls.is_empty() {
            self.record_usage(&meter).await?;
            self.check_output(&response, &meter)?;
            return Ok(None);
        }
        self.complete_model(&response, attempt, &meter).await?;
        Ok(Some(response))
    }
    /// Sends the one retry prompt for an unreadable text-protocol reply, or fails the run on the
    /// second one. Moves `attempt` on to the next attempt number.
    async fn fallback_retry(
        &mut self,
        step: u32,
        attempt: &mut u32,
        failure: tqa_prompt::FallbackFailure,
        problem: Option<tqa_types::FallbackProblem>,
    ) -> Result<(), Stop> {
        if self.fallback_retried(step) {
            return Err(Stop::Failed(
                ErrorCode::FallbackParse,
                fallback::retry_reason(failure),
            ));
        }
        self.emit(Event::ProviderRetry {
            attempt: *attempt,
            delay_ms: 0,
            reason: fallback::retry_reason(failure),
            provider_message: None,
            fallback_problem: problem,
        })
        .await?;
        *attempt = attempt
            .checked_add(1)
            .ok_or(CoreError::Internal("model attempt exhausted"))?;
        Ok(())
    }
    /// Reports a counted failed attempt, waits its delay and moves the attempt number on.
    async fn retry_after(
        &mut self,
        delay: Duration,
        attempt: &mut u32,
        reason: String,
        error: &LlmError,
    ) -> Result<(), Stop> {
        self.emit(Event::ProviderRetry {
            attempt: *attempt,
            delay_ms: millis(delay),
            reason,
            provider_message: error.provider_message(),
            fallback_problem: None,
        })
        .await?;
        self.watch().wait(self.runner.sleeper.sleep(delay)).await?;
        *attempt = next_attempt(*attempt)?;
        Ok(())
    }
    /// Whether the failed attempt is given back because the runtime's own link to the gateway was down and came back:
    /// then the record is emitted (after the wait, since nothing the runtime sends while its link is down reaches
    /// anyone before it is back) and the attempt number moves on, as the throttled path does. `route_attempts` asks it
    /// before the empty-reply accounting, the retry delay and the route switch, so a link outage never switches route
    /// or spends a fallback (CORE-U-062).
    async fn link_back(
        &mut self,
        step: u32,
        empty: bool,
        (attempt, local_attempt): (&mut u32, &mut u32),
        error: &LlmError,
    ) -> Result<bool, Stop> {
        if empty {
            return Ok(false);
        }
        let Some(waited) = self.link_settled(step, error).await? else {
            return Ok(false);
        };
        self.emit(Event::ProviderRetry {
            attempt: *attempt,
            delay_ms: millis(waited),
            reason: GATEWAY_LINK.into(),
            provider_message: None,
            fallback_problem: None,
        })
        .await?;
        *attempt = next_attempt(*attempt)?;
        *local_attempt -= 1;
        Ok(true)
    }
    /// Settles the runtime's link after a transport failure: `Some(waited)` when it was down and came back within the
    /// step's remaining link budget. Healthy, still-down or unconsulted (another failure class, no link, a spent
    /// budget) are `None`, and the failure counts as before. Cancellation, shutdown and the wall clock bound the wait.
    async fn link_settled(
        &mut self,
        step: u32,
        error: &LlmError,
    ) -> Result<Option<Duration>, Stop> {
        let Some(link) = self.runner.gateway_link.clone() else {
            return Ok(None);
        };
        if !matches!(
            error,
            LlmError::Connection(_) | LlmError::Io(_) | LlmError::StreamCut { .. }
        ) {
            return Ok(None);
        }
        if self.link_step != Some(step) {
            self.link_step = Some(step);
            self.link_waited = Duration::ZERO;
        }
        let budget = link.budget().saturating_sub(self.link_waited);
        if budget.is_zero() {
            return Ok(None);
        }
        match self.watch().wait(link.settle(budget)).await? {
            LinkSettle::Reachable => Ok(None),
            LinkSettle::Restored(waited) => {
                self.link_waited = self.link_waited.saturating_add(waited.min(budget));
                Ok(Some(waited))
            }
            LinkSettle::Unreachable(waited) => {
                self.link_waited = self.link_waited.saturating_add(waited.min(budget));
                Ok(None)
            }
        }
    }
    /// Waits out one relay throttle, or moves the run to the next route (ADR-0025); `true` once it has switched.
    async fn throttled(
        &mut self,
        (retry_after, scope, reason): (Duration, ThrottleScope, String),
        throttles: &mut Throttles,
        attempt: &mut u32,
    ) -> Result<bool, Stop> {
        throttles.in_a_row += 1;
        if routes::should_switch_throttled(scope, retry_after, throttles.in_a_row)
            && self.switch_route(reason.clone()).await?
        {
            *attempt = next_attempt(*attempt)?;
            return Ok(true);
        }
        let delay = retry_after.min(RETRY_CAP);
        if throttles.report(delay, self.runner.clock.now()) {
            self.emit(Event::ProviderRetry {
                attempt: *attempt,
                delay_ms: millis(delay),
                reason,
                provider_message: None,
                fallback_problem: None,
            })
            .await?;
        }
        self.watch().wait(self.runner.sleeper.sleep(delay)).await?;
        *attempt = next_attempt(*attempt)?;
        Ok(false)
    }
    /// Why a second empty answer ends the run. On a text-protocol route it says what was seen: with thinking on, a
    /// server can drop the whole answer that follows the reasoning and return nothing (CORE-U-055, ADR-0007).
    /// How many empty replies a step takes before it fails. A text-protocol route gets the step's whole attempt budget:
    /// a server there drops a reply's text now and then, and on 28 Sep a retry got 13 of 21 back. A native-tool route
    /// keeps a single retry.
    fn empty_tries(&self) -> u32 {
        if self.is_fallback() {
            MODEL_ATTEMPTS
        } else {
            2
        }
    }
    fn empty_reason(&self) -> String {
        if self.is_fallback() {
            EMPTY_TEXT_PROTOCOL.into()
        } else {
            "empty_response".into()
        }
    }
    /// The provider error that ends the run, keeping the provider's own words for its `run.failed`.
    /// Counts an answer that couldn't be read; the second in a step fails the run (CORE-E-040).
    fn malformed_again(&mut self, error: &LlmError, malformed: &mut u32) -> Result<(), Stop> {
        if matches!(error, LlmError::Malformed(_)) {
            *malformed += 1;
            if *malformed > 1 {
                return Err(self.provider_failure(error, provider_reason(error)));
            }
        }
        Ok(())
    }
    fn provider_failure(&mut self, error: &LlmError, reason: String) -> Stop {
        self.provider_message = error.provider_message();
        Stop::Failed(failure_code(error), reason)
    }
    fn retry_delay(
        &self,
        attempt: u32,
        error: &LlmError,
        empty: bool,
    ) -> Result<Option<Duration>, Stop> {
        let policy = RetryPolicy::new(MODEL_ATTEMPTS, RETRY_BASE, RETRY_CAP, true)
            .map_err(|_| CoreError::Internal("invalid retry constants"))?;
        let mut jitter = self
            .runner
            .jitter
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // An empty completed response, and an answer that couldn't be read, are retried like an incomplete one (see
        // `empty_tries`, CORE-E-040), with the same backoff.
        let incomplete = LlmError::StreamCut {
            partial: Box::default(),
        };
        let like_a_cut = empty || matches!(error, LlmError::Malformed(_));
        policy
            .next_delay(
                attempt,
                if like_a_cut { &incomplete } else { error },
                jitter.as_mut(),
            )
            .map_err(|error| Stop::Failed(failure_code(&error), provider_reason(&error)))
    }
    async fn stream_attempt(
        &mut self,
        subset: &BTreeSet<String>,
        attempt: u32,
        meter: &mut OutputMeter,
    ) -> Result<Completion, AttemptError> {
        // Text that could begin a call is held until the reply is whole, so a call's markup is never shown.
        let mut text = if self.is_fallback() {
            fallback::TextStream::default()
        } else {
            fallback::TextStream::native()
        };
        let remaining = self.remaining_output()?;
        let declare = self.declare_now(attempt);
        let density = request::count(self, &self.frames).await?.density();
        let built = request::build(self, subset, declare, density)?;
        meter.requested = built.request.max_output;
        // The request's own estimate goes into its usage, so the route's density is read back exactly (CORE-U-084).
        meter.estimated_input = Some(built.needed);
        meter.estimated_route = Some(self.route().name.clone());
        // Its image tokens, attachments and screenshots alike, so the density takes both out of the prompt
        // (CORE-U-084; decided on 7 Oct 2026: estimated_images counts CU's screenshots).
        let (mut request, images) = self.with_images(built).await?;
        meter.estimated_images = Some(images);
        request.run_id.clone_from(&self.context.run_id);
        let mut stream = self
            .watch()
            .wait(
                self.provider()
                    .complete(request, self.children.child_token()),
            )
            .await??;
        let mut assembler = StreamAssembler::new(StreamLimits::RECOMMENDED);
        while let Some(chunk) = self.watch().wait(stream.next()).await? {
            let chunk = chunk?;
            assembler.push(&chunk)?;
            meter.observe(&chunk);
            match chunk {
                StreamChunk::TextDelta(delta) => self.show_delta(&mut text, delta).await?,
                StreamChunk::ReasoningDelta(text) => {
                    self.emit(Event::ReasoningDelta { text }).await?;
                }
                _ => {}
            }
            if meter.count() > u64::from(remaining) {
                return Err(output_exhausted().into());
            }
        }
        let mut response = assembler.finish()?;
        let step = self
            .context
            .step
            .ok_or(Stop::from(CoreError::Internal("missing model step")))?;
        let tail = if self.is_fallback() {
            self.confirm_native(attempt, !response.tool_calls.is_empty());
            let (read, tail) = self
                .read_fallback(response, &text, step, attempt)
                .map_err(|(failure, problem)| AttemptError::Fallback(failure, problem))?;
            response = read;
            tail
        } else if response.tool_calls.is_empty()
            && !matches!(response.finish_reason, Some(FinishReason::Length))
        {
            // A native route whose reply wrote its call as text: run it, or ask again; never a silent answer.
            let (read, tail) = self
                .read_text_calls(response, &text, step, attempt)
                .map_err(|failure| AttemptError::Fallback(failure, None))?;
            response = read;
            tail
        } else {
            // Native calls, or a reply cut at the limit: not read for text calls, so what was held shows as it came, but
            // for a correction copied back ahead of native calls (CORE-U-087).
            text.rest_of(&mut response)
        };
        if let Some(text) = tail {
            self.emit(Event::TextDelta { text }).await?;
        }
        // The journal, the decision engine and dispatch all use the catalogue's own names (ADR-0011 decision 4).
        for call in &mut response.tool_calls {
            call.name = self.runner.names.to_internal(&call.name).to_owned();
        }
        let mut ids = BTreeSet::new();
        for call in &response.tool_calls {
            if call.id.parse::<ToolCallId>().is_err() || !ids.insert(&call.id) {
                return Err(LlmError::Malformed(
                    "invalid or duplicate tool call identifier".into(),
                )
                .into());
            }
        }
        Ok(response)
    }
    async fn complete_model(
        &mut self,
        response: &Completion,
        attempt: u32,
        meter: &OutputMeter,
    ) -> Result<(), Stop> {
        self.record_states(&response.reasoning_state).await?;
        // A reply the model ended at its output limit always stops the run (its budget, or reply_cut), so it stops here,
        // before its tool calls are journaled: a cut call's arguments are incomplete and it never runs, so the page must
        // not show it. The usage already counts the calls' bytes. Every other reply keeps the journal's usual order.
        if matches!(response.finish_reason, Some(FinishReason::Length)) {
            self.record_usage(meter).await?;
            self.check_output(response, meter)?;
        }
        for call in &response.tool_calls {
            let arguments = match serde_json::from_str::<serde_json::Value>(&call.arguments) {
                Ok(mut value @ serde_json::Value::Object(_)) => {
                    // A null for an optional argument means absent: record the tool's real arguments, so the
                    // journal and dispatch both see the same ones (TOOL-U-034, CORE-U-056).
                    self.runner
                        .tools
                        .normalize_arguments(&call.name, &mut value);
                    value
                }
                _ => serde_json::Value::String(call.arguments.clone()),
            };
            self.emit(Event::ToolCall {
                call_id: call
                    .id
                    .parse()
                    .map_err(|_| CoreError::Internal("validated call identifier changed"))?,
                name: call.name.clone(),
                arguments,
            })
            .await?;
        }
        self.record_usage(meter).await?;
        self.check_output(response, meter)?;
        let finish_reason = match response.finish_reason {
            Some(FinishReason::Stop) => tqa_types::FinishReason::Stop,
            Some(FinishReason::ToolCalls) => tqa_types::FinishReason::ToolCalls,
            Some(FinishReason::Length) => tqa_types::FinishReason::Length,
            Some(FinishReason::ContentFilter) => tqa_types::FinishReason::ContentFilter,
            _ => tqa_types::FinishReason::Other,
        };
        self.emit(Event::ModelCompleted {
            step: self
                .context
                .step
                .ok_or(CoreError::Internal("missing model step"))?,
            attempt,
            finish_reason,
        })
        .await?;
        Ok(())
    }
    pub(super) fn current_attempt(&self) -> Result<journal::Attempt, Stop> {
        journal::attempts(&self.frames)
            .into_iter()
            .find(|attempt| attempt.context == self.context)
            .ok_or_else(|| CoreError::Internal("missing current attempt").into())
    }
}
/// One step's run of relay throttles: how many in a row, and the last wait reported.
#[derive(Default)]
pub(super) struct Throttles {
    in_a_row: u32,
    reported: Option<(Duration, Instant)>,
}

impl Throttles {
    /// Whether this wait is reported: a changed delay, or [`THROTTLE_REPORT_EVERY`] since the last report.
    pub(super) fn report(&mut self, delay: Duration, now: Instant) -> bool {
        let due = self.reported.is_none_or(|(last, at)| {
            last != delay || now.saturating_duration_since(at) >= THROTTLE_REPORT_EVERY
        });
        if due {
            self.reported = Some((delay, now));
        }
        due
    }
}

/// The reason for a second empty answer on a text-protocol route.
const EMPTY_TEXT_PROTOCOL: &str = "empty_response: the server returned no text on a text-protocol route; with thinking \
    on, a server can drop the answer that follows after its reasoning";

/// A bounded reason for a provider failure: its code, with the status for an HTTP error; never the body. The
/// gateway's own refusal is `gateway.<code> <status>`, so it is never credited to the provider, and its codes can't
/// be confused with the runtime's own (`rate_limited`, `timeout`).
pub(super) fn provider_reason(error: &LlmError) -> String {
    match error {
        LlmError::Http { status, .. } => format!("{} {status}", error.code()),
        LlmError::GatewayRefused { code, status, .. } => format!("gateway.{code} {status}"),
        _ => error.code().into(),
    }
}

/// A model failure's run code: an agent team's spent budget (the gateway's `team_budget`, ADR-0034 point 8) ends the run
/// as `budget`, as a run past its own limits does; every other model failure is the provider's.
fn failure_code(error: &LlmError) -> ErrorCode {
    match error {
        // A helper stopped at its team's helpers' share is a budget stop too (GW-I-304).
        LlmError::GatewayRefused { code, .. }
            if code == "team_budget" || code == "team_budget_helpers" =>
        {
            ErrorCode::Budget
        }
        _ => ErrorCode::ProviderError,
    }
}

fn next_attempt(attempt: u32) -> Result<u32, Stop> {
    attempt
        .checked_add(1)
        .ok_or_else(|| CoreError::Internal("model attempt exhausted").into())
}
