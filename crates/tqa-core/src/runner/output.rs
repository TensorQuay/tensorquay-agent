//! Run-wide output accounting includes superseded attempts and estimates incomplete streams.
use super::session::{Execution, Stop};
use tqa_context::estimate_bytes;
use tqa_llm::{Completion, FinishReason, StreamChunk, Usage};
use tqa_types::{BudgetKind, ErrorCode, Event, ServerFrame};

#[derive(Default)]
pub(super) struct OutputMeter {
    bytes: usize,
    reported_bytes: usize,
    usage: Option<Usage>,
    /// The `max_output` this attempt's request asked for, for a cut reply's reason.
    pub requested: Option<u32>,
    /// The runner's estimate of the step request this attempt sent and its image tokens, recorded in its usage
    /// (CORE-U-084); `None` for a compaction's summary call.
    pub estimated_input: Option<u64>,
    /// See `estimated_input`.
    pub estimated_images: Option<u64>,
    /// The route the request went to, by its immutable name; see `estimated_input`.
    pub estimated_route: Option<String>,
}
impl OutputMeter {
    pub fn observe(&mut self, chunk: &StreamChunk) {
        let bytes = match chunk {
            StreamChunk::TextDelta(text) | StreamChunk::ReasoningDelta(text) => text.len(),
            StreamChunk::ToolCallDelta {
                id,
                name,
                arguments_delta,
                ..
            } => arguments_delta
                .len()
                .saturating_add(id.as_ref().map_or(0, String::len))
                .saturating_add(name.as_ref().map_or(0, String::len)),
            StreamChunk::Usage(usage) => {
                self.usage = Some(usage.clone());
                self.reported_bytes = self.bytes;
                0
            }
            _ => 0,
        };
        self.bytes = self.bytes.saturating_add(bytes);
    }
    pub fn count(&self) -> u64 {
        self.usage.as_ref().map_or_else(
            || u64::from(estimate_bytes(self.bytes).tokens),
            |usage| {
                usage.completion_tokens.saturating_add(u64::from(
                    estimate_bytes(self.bytes.saturating_sub(self.reported_bytes)).tokens,
                ))
            },
        )
    }
}

impl Execution<'_> {
    pub(super) fn output_used(&self) -> u64 {
        let (mut total, mut bytes, mut reported) = (0_u64, 0_usize, None);
        for frame in &self.frames {
            let ServerFrame::Event { run_id, event, .. } = frame else {
                continue;
            };
            if *run_id != self.context.run_id {
                continue;
            }
            match event {
                Event::StepStarted { .. } => {
                    total = total.saturating_add(
                        reported.unwrap_or_else(|| u64::from(estimate_bytes(bytes).tokens)),
                    );
                    bytes = 0;
                    reported = None;
                }
                Event::TextDelta { text } | Event::ReasoningDelta { text } => {
                    bytes = bytes.saturating_add(text.len());
                }
                Event::ToolCall {
                    call_id,
                    name,
                    arguments,
                } => {
                    bytes = bytes
                        .saturating_add(call_id.as_str().len())
                        .saturating_add(name.len())
                        .saturating_add(arguments.to_string().len());
                }
                // A summary call's usage can share a step window with the step before it.
                Event::Usage {
                    completion_tokens, ..
                } => {
                    reported = Some(reported.unwrap_or(0).saturating_add(*completion_tokens));
                }
                _ => {}
            }
        }
        total.saturating_add(reported.unwrap_or_else(|| u64::from(estimate_bytes(bytes).tokens)))
    }
    pub(super) fn remaining_output(&self) -> Result<u32, Stop> {
        let remaining = self
            .runner
            .limits
            .max_output_tokens
            .saturating_sub(self.output_used());
        if remaining == 0 {
            return Err(output_exhausted());
        }
        Ok(u32::try_from(remaining).unwrap_or(u32::MAX))
    }
    pub(super) async fn record_usage(&mut self, meter: &OutputMeter) -> Result<(), Stop> {
        if meter.bytes == 0 && meter.usage.is_none() {
            return Ok(());
        }
        self.append(Event::Usage {
            prompt_tokens: meter.usage.as_ref().map_or(0, |usage| usage.prompt_tokens),
            completion_tokens: meter.count(),
            cached_tokens: meter.usage.as_ref().and_then(|usage| usage.cached_tokens),
            reasoning_tokens: meter
                .usage
                .as_ref()
                .and_then(|usage| usage.reasoning_tokens),
            estimated: meter.usage.is_none() || meter.bytes > meter.reported_bytes,
            estimated_input: meter.estimated_input,
            estimated_images: meter.estimated_images,
            estimated_route: meter.estimated_route.clone(),
        })
        .await?;
        let used = self.output_used();
        let limit = self.runner.limits.max_output_tokens;
        let warned = self.frames.iter().any(|frame| {
            matches!(frame, ServerFrame::Event { run_id,
            event: Event::BudgetWarning { budget: BudgetKind::OutputTokens, .. }, .. }
            if *run_id == self.context.run_id)
        });
        if !warned && used.saturating_mul(5) >= limit.saturating_mul(4) {
            self.append(Event::BudgetWarning {
                budget: BudgetKind::OutputTokens,
                used,
                limit,
            })
            .await?;
        }
        Ok(())
    }
    /// Stops the run when its own output budget is spent, or when the model ended the reply at its output limit. A
    /// request asks for at most the run's remaining budget, so a length finish is the budget only when the budget is
    /// spent after the reply; otherwise the model server or the route's max output cut it (`reply_cut`), and a cut tool
    /// call never runs.
    pub(super) fn check_output(
        &self,
        response: &Completion,
        meter: &OutputMeter,
    ) -> Result<(), Stop> {
        let used = self.output_used();
        let limit = self.runner.limits.max_output_tokens;
        if used > limit || (used == limit && !response.tool_calls.is_empty()) {
            return Err(output_exhausted());
        }
        if matches!(response.finish_reason, Some(FinishReason::Length)) {
            if used >= limit {
                return Err(output_exhausted());
            }
            let requested = meter.requested.map_or_else(
                || "an unknown number of tokens".to_owned(),
                |tokens| tokens.to_string(),
            );
            // Only thinking, no answer and no call: say so, since a larger limit or less thinking is the way on
            // (CORE-U-072).
            if response.text.trim().is_empty()
                && response.tool_calls.is_empty()
                && !response.reasoning.trim().is_empty()
            {
                let requested = meter.requested.map_or_else(
                    || "no limit was sent, so the model server's own applied".to_owned(),
                    |tokens| format!("{tokens} requested"),
                );
                return Err(Stop::Failed(
                    ErrorCode::ReplyCut,
                    format!(
                        "the model spent its whole output limit thinking ({} tokens) and wrote no answer; {requested}",
                        meter.count()
                    ),
                ));
            }
            return Err(Stop::Failed(
                ErrorCode::ReplyCut,
                format!(
                    "the model stopped at its output limit (finish: length) after {} tokens; {requested} requested",
                    meter.count()
                ),
            ));
        }
        Ok(())
    }
}

pub(super) fn output_exhausted() -> Stop {
    Stop::Failed(ErrorCode::Budget, "budget.output_tokens".into())
}
