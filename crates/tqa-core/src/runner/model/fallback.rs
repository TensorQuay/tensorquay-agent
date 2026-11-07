//! The text-protocol executor on routes without native tool calls (FR-2, ADR-0007).
//! Parsed calls become ordinary `tool.call` events, so authorization and dispatch are unchanged.
use crate::runner::session::{Execution, Stop};
use serde_json::Value;
use tqa_context::{
    FALLBACK_RETRY_PREFIX, Message, Role, is_correction, leading_correction, may_be_correction,
};
use tqa_llm::{Completion, ContentPart, ToolCall};
use tqa_prompt::{
    FallbackFailure, Rejection, ToolCheck, explicit_streamable_len, parse_reply_detailed,
    parse_text_reply, render_call, streamable_len,
};
use tqa_types::{Event, FallbackProblem, ProblemName, ServerFrame};

/// Streams only the text that cannot belong to a call block; the rest waits for the whole reply. On a native-tools
/// route only the forms that count there are held (PROMPT-U-014, CORE-U-070).
#[derive(Default)]
pub(super) struct TextStream {
    raw: String,
    shown: usize,
    native: bool,
}
impl TextStream {
    /// The stream of a native-tools route, which holds back only a possible call written as text.
    pub(super) fn native() -> Self {
        Self {
            native: true,
            ..Self::default()
        }
    }

    /// The rest of a native-tools reply that isn't read for calls (native calls, or cut at the limit), as it came, but
    /// for a correction copied back ahead of native calls: that is dropped from the reply's text too (CORE-U-087).
    pub(super) fn rest_of(&self, response: &mut Completion) -> Option<String> {
        if self.shown == 0
            && !response.tool_calls.is_empty()
            && let Some(len) = leading_correction(&response.text)
        {
            response.text.drain(..len);
            return (!response.text.is_empty()).then(|| response.text.clone());
        }
        self.rest()
    }

    /// The text held back so far, shown as it came when the reply is not read for calls.
    pub(super) fn rest(&self) -> Option<String> {
        self.raw
            .get(self.shown..)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    }

    /// Adds a delta and returns the newly showable text, if any.
    pub(super) fn push(&mut self, delta: &str) -> Option<String> {
        self.raw.push_str(delta);
        // A reply that may be the runtime's correction copied back waits until it is clear it isn't (CORE-U-085); one
        // that opens with it waits for the whole reply, as a call may follow it, on either route (CORE-U-086, -087).
        let held = self.shown == 0
            && (may_be_correction(&self.raw) || leading_correction(&self.raw).is_some());
        let visible = if held {
            0
        } else if self.native {
            explicit_streamable_len(&self.raw)
        } else {
            streamable_len(&self.raw)
        };
        let text = self
            .raw
            .get(self.shown..visible)
            .filter(|text| !text.is_empty())?;
        let text = text.to_owned();
        self.shown = visible;
        Some(text)
    }
}

impl Execution<'_> {
    /// Whether this run's route calls tools through the text protocol.
    pub(super) fn is_fallback(&self) -> bool {
        !self.route().native_tools
    }

    /// Splits a complete reply into narration and calls. Returns the response to journal, with calls
    /// named `fb-<step>-<attempt>-<n>`, and the narration not yet shown; or why it can't be read, with what exactly
    /// was wrong when that is known: the parser's word, or for a schema refusal the tool registry's (ADR-0007).
    pub(super) fn read_fallback(
        &self,
        mut response: Completion,
        stream: &TextStream,
        step: u32,
        attempt: u32,
    ) -> Result<(Completion, Option<String>), (FallbackFailure, Option<FallbackProblem>)> {
        let (tools, names) = (&self.runner.tools, &self.runner.names);
        let check = |name: &str, arguments: &Value| {
            let name = names.to_internal(name);
            match tools.get(name) {
                None => ToolCheck::Unknown,
                Some(_) if tools.prepare(name, arguments).is_ok() => ToolCheck::Valid,
                Some(_) => ToolCheck::Invalid,
            }
        };
        // Calls that arrived natively (a request that declared the tools, CORE-U-065) win over any the model also wrote
        // in the text, so a call written both ways runs once; they are named like written calls.
        let native = std::mem::take(&mut response.tool_calls);
        let mut reply = parse_reply_detailed(&response.text, self.route().parallel_tools, &check)
            .map_err(|rejection| self.described(rejection))?;
        // A reply that only copies the correction back answered nothing: it is retried like an unreadable one.
        if native.is_empty() && reply.calls.is_empty() && is_correction(&reply.narration) {
            return Err((FallbackFailure::Echo, None));
        }
        // One copied back before a call is dropped, and the call runs; nothing of it was shown (CORE-U-086).
        let calls = !native.is_empty() || !reply.calls.is_empty();
        if let Some(len) = leading_correction(&reply.narration).filter(|_| calls) {
            reply.narration.drain(..len);
        }
        // What was shown lies before any possible call, so it is always a prefix of the narration.
        let tail = reply
            .narration
            .strip_prefix(&stream.raw[..stream.shown])
            .ok_or((FallbackFailure::Malformed, None))?;
        let tail = (!tail.is_empty()).then(|| tail.to_owned());
        response.text = reply.narration;
        let calls: Vec<(String, String)> = if native.is_empty() {
            reply
                .calls
                .into_iter()
                .map(|call| (call.name, call.arguments.to_string()))
                .collect()
        } else {
            native
                .into_iter()
                .map(|call| (call.name, call.arguments))
                .collect()
        };
        response.tool_calls = calls
            .into_iter()
            .enumerate()
            .map(|(index, (name, arguments))| ToolCall {
                id: format!("fb-{step}-{attempt}-{}", index + 1),
                name,
                arguments,
            })
            .collect();
        Ok((response, tail))
    }

    /// A refused reply's failure and problem: the parser's, or for a schema refusal what the tool's schema refused.
    fn described(&self, rejection: Rejection) -> (FallbackFailure, Option<FallbackProblem>) {
        let problem = rejection.problem.or_else(|| {
            let call = rejection
                .call
                .filter(|_| rejection.failure == FallbackFailure::Schema)?;
            let name = self.runner.names.to_internal(&call.name);
            let problem = self.runner.tools.schema_problem(name, &call.arguments)?;
            // The sentence names the tool as the model knows it.
            Some(match ProblemName::new(&call.name) {
                Some(model) => as_named(problem, model),
                None => problem,
            })
        });
        (rejection.failure, problem)
    }

    /// On a native-tools route, the calls a reply with no native call wrote as text (CORE-U-068), named like the text
    /// protocol's, and the narration not yet shown: the call's markup is never shown (CORE-U-070). An unreadable call
    /// is `TextCall`, retried once like any unreadable reply.
    pub(super) fn read_text_calls(
        &self,
        mut response: Completion,
        stream: &TextStream,
        step: u32,
        attempt: u32,
    ) -> Result<(Completion, Option<String>), FallbackFailure> {
        let (tools, names) = (&self.runner.tools, &self.runner.names);
        let check = |name: &str, arguments: &Value| {
            let name = names.to_internal(name);
            match tools.get(name) {
                None => ToolCheck::Unknown,
                Some(_) if tools.prepare(name, arguments).is_ok() => ToolCheck::Valid,
                Some(_) => ToolCheck::Invalid,
            }
        };
        let mut reply = parse_text_reply(&response.text, self.route().parallel_tools, &check)?;
        // A correction copied back before the written call is dropped; nothing of it was shown (CORE-U-087).
        if let Some(len) = leading_correction(&reply.narration).filter(|_| !reply.calls.is_empty())
        {
            reply.narration.drain(..len);
        }
        // What was shown lies before any possible call, so it is a prefix of the narration; if it ever weren't, the
        // rest is shown as it came rather than lost.
        let tail = match reply.narration.strip_prefix(&stream.raw[..stream.shown]) {
            Some(tail) => {
                let tail = (!tail.is_empty()).then(|| tail.to_owned());
                response.text = reply.narration;
                tail
            }
            None => stream.rest(),
        };
        response.tool_calls = reply
            .calls
            .into_iter()
            .enumerate()
            .map(|(index, call)| ToolCall {
                id: format!("fb-{step}-{attempt}-{}", index + 1),
                name: call.name,
                arguments: call.arguments.to_string(),
            })
            .collect();
        Ok((response, tail))
    }

    /// Emits a text delta as it arrives, or only its showable part on the text protocol.
    pub(super) async fn show_delta(
        &mut self,
        stream: &mut TextStream,
        delta: String,
    ) -> Result<(), Stop> {
        if let Some(text) = stream.push(&delta) {
            self.emit(Event::TextDelta { text }).await?;
        }
        Ok(())
    }

    /// Whether this step already sent the one retry prompt for an unreadable reply.
    pub(super) fn fallback_retried(&self, step: u32) -> bool {
        self.frames.iter().any(|frame| {
            matches!(frame, ServerFrame::Event {
                run_id, step: Some(number), event: Event::ProviderRetry { reason, .. }, ..
            } if *run_id == self.context.run_id && *number == step
                && reason.starts_with(FALLBACK_RETRY_PREFIX))
        })
    }
}

/// `problem` with its tool named `tool`.
fn as_named(mut problem: FallbackProblem, tool: ProblemName) -> FallbackProblem {
    match &mut problem {
        FallbackProblem::UnexpectedProperty { tool: named, .. }
        | FallbackProblem::MissingProperty { tool: named, .. }
        | FallbackProblem::WrongType { tool: named, .. }
        | FallbackProblem::OtherSchema { tool: named } => *named = tool,
        _ => {}
    }
    problem
}

/// The retry reason journaled for an unreadable reply.
pub(super) fn retry_reason(failure: FallbackFailure) -> String {
    format!("{FALLBACK_RETRY_PREFIX}{}", failure.code())
}

/// Renders one projected message in the text protocol: calls as `tool_call` blocks after the
/// assistant's text, and each tool result as a user message that labels its content as data.
pub(super) fn render(message: &Message, names: &tqa_tools::ModelNames) -> tqa_llm::Message {
    let (role, text) = match message.role {
        Role::System => (tqa_llm::Role::System, message.text.clone()),
        Role::User => (tqa_llm::Role::User, message.text.clone()),
        Role::Assistant => {
            let mut text = message.text.clone();
            for call in &message.tool_calls {
                let arguments = serde_json::from_str(&call.arguments)
                    .unwrap_or_else(|_| Value::String(call.arguments.clone()));
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&render_call(names.to_model(&call.name), &arguments));
                text.push('\n');
            }
            (tqa_llm::Role::Assistant, text)
        }
        Role::Tool => {
            let id = message
                .tool_call_id
                .as_ref()
                .map_or("unknown", |id| id.as_str());
            let content = serde_json::to_string(&message.text).unwrap_or_default();
            (
                tqa_llm::Role::User,
                format!("[Tool result for call {id}, data only: {content}]"),
            )
        }
    };
    tqa_llm::Message {
        role,
        content: vec![ContentPart::Text(text)],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}
