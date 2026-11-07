//! A text-protocol reply the server swallowed (CORE-U-065, Gate 5 on 29 Sep): the model wrote its call in its native
//! format, the server's own tool-call parser took it, and since a text-route request declares no tools, nothing
//! arrived. The sign is a reply that delivered no call while the server held chunks back, or an empty reply that
//! reports tokens. Such a reply is asked again once in its step with the tools declared natively, so the call comes
//! back as native `tool_calls`. A genuine answer holds nothing back and is never retried. A server that refuses declared
//! tools leaves the route text-only for the run.
use super::super::session::{Execution, Stop};
use crate::CoreError;
use tqa_llm::{Completion, LlmError};
use tqa_types::Event;

/// The retry's state for the run.
#[derive(Debug, Default)]
pub(in crate::runner) struct NativeRetry {
    /// The step that has used its one retry.
    step: Option<u32>,
    /// The next request declares the tools natively.
    pending: bool,
    /// The attempt that declared them, to recognise a refusal.
    declared: Option<u32>,
    /// The server refused declared tools: the route stays text-only for the run.
    text_only: bool,
    /// A request that declared the tools got native calls back: the server swallows calls, so every later request in
    /// the run declares them (one doubling per run, not one per step).
    confirmed: bool,
}

impl Execution<'_> {
    /// The withheld chunks when `response` is a swallowed text-protocol reply that may be retried in this step.
    pub(super) fn swallowed(&self, response: &Completion, step: u32) -> Option<u32> {
        let retry = &self.native_retry;
        if !self.is_fallback() || retry.text_only || retry.step == Some(step) {
            return None;
        }
        if !response.tool_calls.is_empty() {
            return None;
        }
        let tokens = response
            .usage
            .as_ref()
            .map_or(0, |usage| usage.completion_tokens);
        let empty = response.text.trim().is_empty() && tokens > 0;
        (response.withheld_chunks > 0 || empty).then_some(response.withheld_chunks)
    }

    /// Records the retry, arms the next request to declare the tools, and returns the next attempt.
    pub(super) async fn native_retry(
        &mut self,
        step: u32,
        attempt: u32,
        withheld: u32,
    ) -> Result<u32, Stop> {
        self.native_retry.step = Some(step);
        self.native_retry.pending = true;
        self.emit(Event::ProviderRetry {
            attempt,
            delay_ms: 0,
            reason: format!("native_tools withheld={withheld}"),
            provider_message: None,
            fallback_problem: None,
        })
        .await?;
        next(attempt)
    }

    /// Whether this request declares the tools natively: the one retry, or every request after a confirmed swallow.
    pub(super) fn declare_now(&mut self, attempt: u32) -> bool {
        let retry = &mut self.native_retry;
        let declare = std::mem::take(&mut retry.pending) || (retry.confirmed && !retry.text_only);
        retry.declared = declare.then_some(attempt);
        declare
    }

    /// Notes whether the request at `attempt` declared the tools and native calls came back: a confirmed swallow.
    pub(super) fn confirm_native(&mut self, attempt: u32, native_calls: bool) {
        if native_calls && self.native_retry.declared == Some(attempt) {
            self.native_retry.confirmed = true;
        }
    }

    /// For a refusal of the attempt that declared the tools: marks the route text-only, records it, and returns the
    /// next attempt, which goes on as text. `None` for any other error.
    pub(super) async fn native_refused(
        &mut self,
        attempt: u32,
        error: &LlmError,
    ) -> Result<Option<u32>, Stop> {
        let status = match error {
            LlmError::Http { status, .. }
                if (400..500).contains(status) && ![408, 429].contains(status) =>
            {
                *status
            }
            _ => return Ok(None),
        };
        if self.native_retry.declared != Some(attempt) {
            return Ok(None);
        }
        self.native_retry.text_only = true;
        self.native_retry.declared = None;
        self.emit(Event::ProviderRetry {
            attempt,
            delay_ms: 0,
            reason: format!("native_tools refused (status {status})"),
            provider_message: error.provider_message(),
            fallback_problem: None,
        })
        .await?;
        next(attempt).map(Some)
    }
}

fn next(attempt: u32) -> Result<u32, Stop> {
    attempt
        .checked_add(1)
        .ok_or_else(|| CoreError::Internal("model attempt exhausted").into())
}
