//! Responses stream events mapped to provider-independent chunks; unknown events are ignored.

use crate::{FinishReason, LlmError, StreamChunk, StreamLimitKind, StreamLimits, Usage, sse::Wire};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Error codes kept from a stream error; anything else is reported as `unknown`.
const MAX_CODE_BYTES: usize = 64;

pub(super) struct ResponsesWire {
    limits: StreamLimits,
    calls: BTreeMap<u64, Call>,
    buffered: usize,
    called: bool,
    done: bool,
}

/// A function call being assembled; it is emitted once, whole, at its `done` event.
struct Call {
    slot: u32,
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    emitted: bool,
}

#[derive(Deserialize)]
struct Delta {
    delta: String,
}
#[derive(Deserialize)]
struct ItemEvent {
    output_index: u64,
    item: Item,
}
#[derive(Deserialize)]
struct Item {
    #[serde(rename = "type")]
    kind: String,
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
}
#[derive(Deserialize)]
struct ArgumentsDelta {
    output_index: u64,
    delta: String,
}
#[derive(Deserialize)]
struct ArgumentsDone {
    output_index: u64,
    arguments: Option<String>,
}
#[derive(Deserialize)]
struct Terminal {
    response: TerminalResponse,
}
#[derive(Deserialize)]
struct TerminalResponse {
    usage: Option<WireUsage>,
    incomplete_details: Option<IncompleteDetails>,
    error: Option<StreamError>,
}
#[derive(Deserialize)]
struct IncompleteDetails {
    reason: Option<String>,
}
#[derive(Deserialize)]
struct StreamError {
    code: Option<Value>,
}
#[derive(Deserialize)]
struct WireUsage {
    input_tokens: u64,
    output_tokens: u64,
    input_tokens_details: Option<InputDetails>,
    output_tokens_details: Option<OutputDetails>,
}
#[derive(Deserialize)]
struct InputDetails {
    cached_tokens: Option<u64>,
}
#[derive(Deserialize)]
struct OutputDetails {
    reasoning_tokens: Option<u64>,
}

fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, LlmError> {
    serde_json::from_value(value).map_err(|error| {
        LlmError::Malformed(format!(
            "invalid Responses event: {}",
            crate::redact(&error.to_string())
        ))
    })
}

impl ResponsesWire {
    pub(super) fn new(limits: StreamLimits) -> Self {
        Self {
            limits,
            calls: BTreeMap::new(),
            buffered: 0,
            called: false,
            done: false,
        }
    }

    fn open(&mut self, index: u64, item: Item) -> Result<(), LlmError> {
        if self.calls.contains_key(&index) {
            return Err(LlmError::Malformed("duplicate call in the response".into()));
        }
        if self.calls.len() >= self.limits.max_tool_calls {
            return Err(LlmError::StreamLimit {
                which: StreamLimitKind::ToolCalls,
                max: self.limits.max_tool_calls,
            });
        }
        let slot = u32::try_from(self.calls.len())
            .map_err(|_| LlmError::Malformed("too many calls".into()))?;
        self.calls.insert(
            index,
            Call {
                slot,
                id: item.call_id,
                name: item.name,
                arguments: String::new(),
                emitted: false,
            },
        );
        self.append(index, &item.arguments.unwrap_or_default())
    }

    fn append(&mut self, index: u64, delta: &str) -> Result<(), LlmError> {
        let call = self
            .calls
            .get_mut(&index)
            .ok_or_else(|| LlmError::Malformed("arguments for an unknown call".into()))?;
        self.buffered = self.buffered.saturating_add(delta.len());
        if self.buffered > self.limits.max_completion_bytes {
            return Err(LlmError::StreamLimit {
                which: StreamLimitKind::Completion,
                max: self.limits.max_completion_bytes,
            });
        }
        call.arguments.push_str(delta);
        Ok(())
    }

    /// Emits a call once at its first `done` event; a later one must agree with it.
    fn finish_call(
        &mut self,
        index: u64,
        arguments: Option<String>,
        item: Option<Item>,
    ) -> Result<Vec<StreamChunk>, LlmError> {
        let call = self
            .calls
            .get_mut(&index)
            .ok_or_else(|| LlmError::Malformed("done event for an unknown call".into()))?;
        if let Some(arguments) = arguments
            && arguments != call.arguments
        {
            return Err(LlmError::Malformed(
                "call arguments do not match their deltas".into(),
            ));
        }
        if let Some(item) = item {
            call.id = call.id.take().or(item.call_id);
            call.name = call.name.take().or(item.name);
        }
        if call.emitted {
            return Ok(Vec::new());
        }
        let (Some(id), Some(name)) = (&call.id, &call.name) else {
            return Err(LlmError::Malformed(
                "tool call is missing its identifier or name".into(),
            ));
        };
        call.emitted = true;
        self.called = true;
        Ok(vec![StreamChunk::ToolCallDelta {
            index: call.slot,
            id: Some(id.clone()),
            name: Some(name.clone()),
            arguments_delta: call.arguments.clone(),
        }])
    }

    fn terminal(&mut self, value: Value, completed: bool) -> Result<Vec<StreamChunk>, LlmError> {
        if self.calls.values().any(|call| !call.emitted) {
            return Err(LlmError::Malformed(
                "a tool call was unfinished when the response ended".into(),
            ));
        }
        let terminal: Terminal = parse(value)?;
        self.done = true;
        let mut chunks: Vec<_> = terminal
            .response
            .usage
            .map(usage)
            .map(StreamChunk::Usage)
            .into_iter()
            .collect();
        let reason = if completed {
            if self.called {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        } else {
            match terminal
                .response
                .incomplete_details
                .and_then(|details| details.reason)
                .as_deref()
            {
                Some("max_output_tokens") => FinishReason::Length,
                Some("content_filter") => FinishReason::ContentFilter,
                Some(other) => FinishReason::Other(other.into()),
                None => FinishReason::Other("incomplete".into()),
            }
        };
        chunks.push(StreamChunk::Finish(reason));
        Ok(chunks)
    }
}

impl Wire for ResponsesWire {
    fn data(&mut self, data: &str) -> Result<Vec<StreamChunk>, LlmError> {
        let value: Value = serde_json::from_str(data).map_err(|error| {
            LlmError::Malformed(format!(
                "invalid Responses event: {}",
                crate::redact(&error.to_string())
            ))
        })?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| LlmError::Malformed("invalid Responses event: no type".into()))?
            .to_owned();
        match kind.as_str() {
            "response.output_text.delta" | "response.refusal.delta" => {
                Ok(vec![StreamChunk::TextDelta(parse::<Delta>(value)?.delta)])
            }
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                Ok(vec![StreamChunk::ReasoningDelta(
                    parse::<Delta>(value)?.delta,
                )])
            }
            "response.output_item.added" => {
                let event: ItemEvent = parse(value)?;
                if event.item.kind == "function_call" {
                    self.open(event.output_index, event.item)?;
                }
                Ok(Vec::new())
            }
            "response.function_call_arguments.delta" => {
                let event: ArgumentsDelta = parse(value)?;
                self.append(event.output_index, &event.delta)?;
                Ok(Vec::new())
            }
            "response.function_call_arguments.done" => {
                let event: ArgumentsDone = parse(value)?;
                self.finish_call(event.output_index, event.arguments, None)
            }
            "response.output_item.done" => {
                let item = value.get("item").cloned();
                let event: ItemEvent = parse(value)?;
                match event.item.kind.as_str() {
                    "function_call" => {
                        let arguments = event.item.arguments.clone();
                        self.finish_call(event.output_index, arguments, Some(event.item))
                    }
                    "reasoning" => Ok(reasoning_state(item).into_iter().collect()),
                    _ => Ok(Vec::new()),
                }
            }
            "response.completed" => self.terminal(value, true),
            "response.incomplete" => self.terminal(value, false),
            "response.failed" => {
                let terminal: Terminal = parse(value)?;
                Err(stream_error(
                    terminal
                        .response
                        .error
                        .as_ref()
                        .and_then(|error| error.code.as_ref()),
                ))
            }
            "error" => Err(stream_error(value.get("code"))),
            _ => Ok(Vec::new()),
        }
    }

    fn done(&self) -> bool {
        self.done
    }
}

/// A completed reasoning item that carries `encrypted_content`, as it will be replayed: the item
/// as received, without its `status`. Items without encrypted content cannot be replayed statelessly.
fn reasoning_state(item: Option<Value>) -> Option<StreamChunk> {
    let Some(Value::Object(mut item)) = item else {
        return None;
    };
    item.get("encrypted_content")
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty())?;
    item.remove("status");
    Some(StreamChunk::ReasoningState(Value::Object(item).to_string()))
}

fn usage(usage: WireUsage) -> Usage {
    Usage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        cached_tokens: usage
            .input_tokens_details
            .and_then(|details| details.cached_tokens),
        reasoning_tokens: usage
            .output_tokens_details
            .and_then(|details| details.reasoning_tokens),
    }
}

/// Maps a stream error by its code alone; the message is never read, since it can echo prompt
/// content. Account and billing codes are permanent, so they never trigger a route switch.
fn stream_error(code: Option<&Value>) -> LlmError {
    let code = code
        .and_then(Value::as_str)
        .filter(|code| {
            !code.is_empty()
                && code.len() <= MAX_CODE_BYTES
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        })
        .unwrap_or("unknown");
    match code {
        "rate_limit_exceeded" => LlmError::RateLimited { retry_after: None },
        "server_error" | "internal_error" | "overloaded" | "server_is_overloaded" => {
            LlmError::Unavailable { retry_after: None }
        }
        _ => LlmError::Protocol(format!("model error: {code}")),
    }
}

/// Usage from a terminal event, for the relay's usage tap; other events return `None`.
pub(crate) fn terminal_usage(data: &str) -> Option<Usage> {
    let value: Value = serde_json::from_str(data).ok()?;
    match value.get("type")?.as_str()? {
        "response.completed" | "response.incomplete" | "response.failed" => {
            let terminal: Terminal = serde_json::from_value(value).ok()?;
            terminal.response.usage.map(usage)
        }
        _ => None,
    }
}
