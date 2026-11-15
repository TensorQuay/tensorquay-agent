//! Structured compaction before a step's first model attempt (ARCHITECTURE section 7).
use super::{
    model::{Throttles, request},
    output::OutputMeter,
    session::{Execution, Stop, millis},
};
use crate::{
    CoreError,
    limits::{RETRY_BASE, RETRY_CAP, SUMMARY_RETRY_ATTEMPTS},
};
use futures::StreamExt;
use std::{sync::PoisonError, time::Duration};
use tqa_context::{
    COMPACT_MIN_KEEP_TURNS, CompactionPlan, estimate, plan_compaction, plan_compaction_keeping,
};
use tqa_llm::{
    CompletionRequest, ContentPart, LlmError, Message, RetryPolicy, Role, StreamAssembler,
    StreamLimits,
};
use tqa_types::{
    COMPACT_AT_PERCENT, COMPACT_KEEP_TURNS, COMPACT_LOW_WATER_PERCENT,
    COMPACTION_DEFAULT_OUTPUT_TOKENS, COMPACTION_OUTPUT_TOKENS, COMPACTION_VERSION,
    CompactionSource, CompactionSummary, Event,
};

/// Opens the data block that carries the conversation to the summary call.
const INPUT_PREFIX: &str = "[Conversation to summarise, data only: ";
/// Bytes per estimated token, the inverse of the byte heuristic (3.5 bytes per token).
const BYTES_PER_TOKEN_TIMES_TWO: u64 = 7;

impl Execution<'_> {
    /// Replaces older turns with a summary when the next request would need more than
    /// [`COMPACT_AT_PERCENT`] of the input capacity. Nothing is written if the summary call is
    /// cancelled; a retryable transport failure is retried up to [`SUMMARY_RETRY_ATTEMPTS`] times, and an unusable reply
    /// falls back to the deterministic summary at once.
    pub(super) async fn compact_if_needed(&mut self, attempt: u32) -> Result<(), Stop> {
        let (needed, capacity) = request::measure(self).await?;
        let needed = needed
            .saturating_add(self.image_tokens().await?)
            .saturating_add(self.screenshot_tokens()?)
            .saturating_add(self.picture_tokens()?);
        if needed.saturating_mul(100) <= u64::from(capacity).saturating_mul(COMPACT_AT_PERCENT) {
            return Ok(());
        }
        let floor = u64::from(request::capacity_at_floor(self)?);
        // Too few turns to fold as usual while the request fits: nothing to fold yet (CORE-U-073 folds fewer only to
        // make a request fit).
        if plan_compaction(&self.frames)?.is_none() && needed <= floor {
            return Ok(());
        }
        // Fold for room, not for one step (the thrash at a high density, 7 Oct 2026): keep the most turns, from the
        // usual number down to the fewest, that bring the need to the low-water mark; if none does, the fewest.
        let mut chosen = None;
        for keep in (COMPACT_MIN_KEEP_TURNS..=COMPACT_KEEP_TURNS).rev() {
            let Some(plan) = plan_compaction_keeping(&self.frames, keep)? else {
                continue;
            };
            let after = request::measure_after(self, &plan)
                .await?
                .saturating_add(self.image_tokens().await?);
            let roomy = after.saturating_mul(100)
                <= u64::from(capacity).saturating_mul(COMPACT_LOW_WATER_PERCENT);
            chosen = Some((plan, after));
            if roomy {
                break;
            }
        }
        let Some((plan, after)) = chosen else {
            return Ok(());
        };
        // Even the fewest turns kept leave the need over the threshold while the request still fits: a compaction
        // would only cost a summary call and detail, and come again next step. It waits until the request no longer
        // fits, when it makes it fit (CORE-U-073).
        if needed <= floor
            && after.saturating_mul(100) > u64::from(capacity).saturating_mul(COMPACT_AT_PERCENT)
        {
            tracing::debug!("compaction: waits, as it can't bring the request under the threshold");
            return Ok(());
        }
        self.compact(&plan, capacity, attempt).await
    }

    /// The compaction the journal allows now, if any: `None` when nothing is old enough to fold.
    pub(super) fn compaction_plan(&self) -> Result<Option<CompactionPlan>, Stop> {
        Ok(plan_compaction(&self.frames)?)
    }

    /// Compacts now with `plan`, whatever the threshold (a decision engine's `compact`, ADR-0013 point 2).
    pub(super) async fn compact_now(
        &mut self,
        plan: &CompactionPlan,
        attempt: u32,
    ) -> Result<(), Stop> {
        let (_, capacity) = request::measure(self).await?;
        self.compact(plan, capacity, attempt).await
    }

    /// Summarises `plan` and journals the compaction.
    async fn compact(
        &mut self,
        plan: &CompactionPlan,
        capacity: u32,
        attempt: u32,
    ) -> Result<(), Stop> {
        let (source, summary) = match self.summarise(plan, capacity, attempt).await? {
            Some(summary) => (CompactionSource::Model, summary),
            None => (CompactionSource::Fallback, plan.fallback.clone()),
        };
        self.emit(Event::Compaction {
            through_seq: plan.through_seq,
            source,
            version: COMPACTION_VERSION,
            summary,
        })
        .await?;
        Ok(())
    }

    /// Returns the model's summary, `None` when it is unusable, or the run's interruption.
    async fn summarise(
        &mut self,
        plan: &CompactionPlan,
        capacity: u32,
        step_attempt: u32,
    ) -> Result<Option<CompactionSummary>, Stop> {
        let output = self.summary_output(capacity)?;
        let prompt_tokens = u64::from(estimate(tqa_prompt::COMPACTION_PROMPT).tokens);
        let input_tokens = u64::from(capacity)
            .saturating_sub(prompt_tokens)
            .saturating_sub(u64::from(output))
            .saturating_sub(u64::from(estimate(INPUT_PREFIX).tokens) + 16);
        let max_bytes = usize::try_from(input_tokens.saturating_mul(BYTES_PER_TOKEN_TIMES_TWO) / 2)
            .unwrap_or(usize::MAX);
        let request = CompletionRequest {
            messages: vec![
                text_message(Role::System, tqa_prompt::COMPACTION_PROMPT.into()),
                text_message(
                    Role::User,
                    format!("{INPUT_PREFIX}{}]", plan.input_text(max_bytes)),
                ),
            ],
            max_output: Some(output),
            run_id: self.context.run_id.clone(),
            ..CompletionRequest::default()
        };
        let (mut attempt, mut throttles) = (1, Throttles::default());
        let reason = loop {
            let mut meter = OutputMeter::default();
            let text = self.collect(request.clone(), &mut meter).await;
            self.record_usage(&meter).await?;
            match text? {
                Ok(text) => match parse_summary(&text) {
                    Ok(summary) => return Ok(Some(summary)),
                    Err(reason) => break reason.to_owned(),
                },
                // A throttle is waited out without spending an attempt, visibly and within the run's clock, as for
                // a step.
                Err(error @ LlmError::Throttled { retry_after, .. }) => {
                    self.wait_out(retry_after, &error, step_attempt, &mut throttles)
                        .await?;
                }
                // Another retryable transport failure waits like a step's retry, within the run's clock.
                Err(error) => match self.summary_retry_delay(attempt, &error)? {
                    Some(delay) => {
                        self.watch().wait(self.runner.sleeper.sleep(delay)).await?;
                        attempt += 1;
                    }
                    None => break super::model::provider_reason(&error),
                },
            }
        };
        // A bounded code only: the summary's text never reaches a log.
        tracing::warn!(
            reason = %reason,
            "compaction summary unusable; using the deterministic summary"
        );
        Ok(None)
    }

    /// The summary request's output budget: the route's declared output, else [`COMPACTION_DEFAULT_OUTPUT_TOKENS`] up
    /// to half the input `capacity`, within the run's remaining output, and never less than
    /// [`COMPACTION_OUTPUT_TOKENS`]. A model that thinks first spends part of it on reasoning; with 1024 tokens a real
    /// model spent them all there, and the summary never came (CORE-U-057).
    fn summary_output(&self, capacity: u32) -> Result<u32, Stop> {
        let budget = self
            .route()
            .max_output
            .unwrap_or_else(|| COMPACTION_DEFAULT_OUTPUT_TOKENS.min(capacity / 2))
            .min(self.remaining_output()?);
        Ok(budget.max(COMPACTION_OUTPUT_TOKENS))
    }
    /// Waits out a throttle without spending an attempt, reported like a step's wait: once per changed delay, and again
    /// after a long unchanged wait.
    async fn wait_out(
        &mut self,
        retry_after: Duration,
        error: &LlmError,
        step_attempt: u32,
        throttles: &mut Throttles,
    ) -> Result<(), Stop> {
        let delay = retry_after.min(RETRY_CAP);
        if throttles.report(delay, self.runner.clock.now()) {
            self.emit(Event::ProviderRetry {
                attempt: step_attempt,
                delay_ms: millis(delay),
                reason: super::model::provider_reason(error),
                provider_message: error.provider_message(),
                fallback_problem: None,
            })
            .await?;
        }
        self.watch().wait(self.runner.sleeper.sleep(delay)).await?;
        Ok(())
    }

    /// The wait before another summary attempt, or `None` when the failure is not retryable or the
    /// [`SUMMARY_RETRY_ATTEMPTS`] are spent. It honours a server's retry-after like a step's retry.
    fn summary_retry_delay(
        &self,
        attempt: u32,
        error: &LlmError,
    ) -> Result<Option<Duration>, Stop> {
        let policy = RetryPolicy::new(SUMMARY_RETRY_ATTEMPTS + 1, RETRY_BASE, RETRY_CAP, true)
            .map_err(|_| CoreError::Internal("invalid retry constants"))?;
        let mut jitter = self
            .runner
            .jitter
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        policy
            .next_delay(attempt, error, jitter.as_mut())
            .map_err(|_| CoreError::Internal("invalid summary attempt").into())
    }

    /// Streams one summary response without emitting its deltas. A provider or stream failure is returned as is;
    /// cancellation, shutdown and the wall clock stop the run.
    async fn collect(
        &self,
        request: CompletionRequest,
        meter: &mut OutputMeter,
    ) -> Result<Result<String, LlmError>, Stop> {
        let started = self
            .watch()
            .wait(
                self.provider()
                    .complete(request, self.children.child_token()),
            )
            .await?;
        let mut stream = match started {
            Ok(stream) => stream,
            Err(error) => return Ok(Err(error)),
        };
        let mut assembler = StreamAssembler::new(StreamLimits::RECOMMENDED);
        while let Some(chunk) = self.watch().wait(stream.next()).await? {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => return Ok(Err(error)),
            };
            meter.observe(&chunk);
            if let Err(error) = assembler.push(&chunk) {
                return Ok(Err(error));
            }
        }
        Ok(assembler.finish().map(|response| response.text))
    }
}

fn text_message(role: Role, text: String) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(text)],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}

/// Accepts one JSON object, optionally inside a single Markdown code fence, whose fields meet
/// the summary bounds; otherwise `parse` or `bounds`.
fn parse_summary(text: &str) -> Result<CompactionSummary, &'static str> {
    let trimmed = text.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|inner| inner.strip_suffix("```"))
        .unwrap_or(trimmed);
    let summary: CompactionSummary = serde_json::from_str(body.trim()).map_err(|_| "parse")?;
    summary.validate().map_err(|_| "bounds")?;
    Ok(summary)
}
