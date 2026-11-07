//! Message stream events mapped to provider-independent chunks; unknown events are ignored.

use crate::{FinishReason, LlmError, StreamChunk, StreamLimitKind, StreamLimits, Usage, sse::Wire};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Error types kept from a stream error; anything else is reported as `unknown`.
const MAX_TYPE_BYTES: usize = 64;

pub(super) struct MessagesWire {
    limits: StreamLimits,
    blocks: BTreeMap<u64, Block>,
    calls: usize,
    buffered: usize,
    usage: WireUsage,
    stop_reason: Option<String>,
    done: bool,
}

/// An open content block; tool calls and thinking are buffered until the block stops.
enum Block {
    Text,
    Thinking {
        thinking: String,
        signature: String,
    },
    Redacted(Value),
    Tool {
        slot: u32,
        id: String,
        name: String,
        arguments: String,
    },
    Ignored,
}

/// Usage fields as reported so far; each event may repeat or update any of them.
#[derive(Default, Deserialize)]
#[allow(
    clippy::struct_field_names,
    reason = "The field names are the protocol's own."
)]
pub(crate) struct WireUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

impl WireUsage {
    pub(crate) fn merge(&mut self, update: &Self) {
        self.input_tokens = update.input_tokens.or(self.input_tokens);
        self.output_tokens = update.output_tokens.or(self.output_tokens);
        self.cache_creation_input_tokens = update
            .cache_creation_input_tokens
            .or(self.cache_creation_input_tokens);
        self.cache_read_input_tokens = update
            .cache_read_input_tokens
            .or(self.cache_read_input_tokens);
    }

    /// The whole prompt counts uncached, cache-written and cache-read input, as on the other
    /// protocols; the protocol reports no separate reasoning count.
    pub(crate) fn usage(&self) -> Option<Usage> {
        let prompt = self
            .input_tokens?
            .saturating_add(self.cache_creation_input_tokens.unwrap_or(0))
            .saturating_add(self.cache_read_input_tokens.unwrap_or(0));
        Some(Usage {
            prompt_tokens: prompt,
            completion_tokens: self.output_tokens?,
            cached_tokens: self.cache_read_input_tokens,
            reasoning_tokens: None,
        })
    }
}

#[derive(Deserialize)]
struct Start {
    message: StartMessage,
}
#[derive(Deserialize)]
struct StartMessage {
    usage: Option<WireUsage>,
}
#[derive(Deserialize)]
struct BlockStart {
    index: u64,
    content_block: Value,
}
#[derive(Deserialize)]
struct BlockDelta {
    index: u64,
    delta: Value,
}
#[derive(Deserialize)]
struct BlockStop {
    index: u64,
}
#[derive(Deserialize)]
struct MessageDelta {
    delta: StopDelta,
    usage: Option<WireUsage>,
}
#[derive(Deserialize)]
struct StopDelta {
    stop_reason: Option<String>,
}

fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, LlmError> {
    serde_json::from_value(value).map_err(|error| {
        LlmError::Malformed(format!(
            "invalid Messages event: {}",
            crate::redact(&error.to_string())
        ))
    })
}

fn text(value: &Value, field: &str) -> Result<String, LlmError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(LlmError::Malformed(format!(
            "invalid Messages event: {field} is not a string"
        ))),
    }
}

impl MessagesWire {
    pub(super) fn new(limits: StreamLimits) -> Self {
        Self {
            limits,
            blocks: BTreeMap::new(),
            calls: 0,
            buffered: 0,
            usage: WireUsage::default(),
            stop_reason: None,
            done: false,
        }
    }

    /// Counts buffered bytes against the completion bound.
    fn buffer(&mut self, bytes: usize) -> Result<(), LlmError> {
        self.buffered = self.buffered.saturating_add(bytes);
        if self.buffered > self.limits.max_completion_bytes {
            return Err(LlmError::StreamLimit {
                which: StreamLimitKind::Completion,
                max: self.limits.max_completion_bytes,
            });
        }
        Ok(())
    }

    fn start(&mut self, event: &BlockStart) -> Result<Vec<StreamChunk>, LlmError> {
        if self.blocks.contains_key(&event.index) {
            return Err(LlmError::Malformed("duplicate content block".into()));
        }
        let block = &event.content_block;
        let mut chunks = Vec::new();
        let opened = match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let initial = text(block, "text")?;
                if !initial.is_empty() {
                    chunks.push(StreamChunk::TextDelta(initial));
                }
                Block::Text
            }
            Some("thinking") => {
                let thinking = text(block, "thinking")?;
                let signature = text(block, "signature")?;
                self.buffer(thinking.len() + signature.len())?;
                if !thinking.is_empty() {
                    chunks.push(StreamChunk::ReasoningDelta(thinking.clone()));
                }
                Block::Thinking {
                    thinking,
                    signature,
                }
            }
            Some("redacted_thinking") => {
                self.buffer(block.to_string().len())?;
                Block::Redacted(block.get("data").cloned().unwrap_or(Value::Null))
            }
            Some("tool_use") => self.open_call(block)?,
            _ => Block::Ignored,
        };
        self.blocks.insert(event.index, opened);
        Ok(chunks)
    }

    fn open_call(&mut self, block: &Value) -> Result<Block, LlmError> {
        if self.calls >= self.limits.max_tool_calls {
            return Err(LlmError::StreamLimit {
                which: StreamLimitKind::ToolCalls,
                max: self.limits.max_tool_calls,
            });
        }
        let (id, name) = (text(block, "id")?, text(block, "name")?);
        if id.is_empty() || name.is_empty() {
            return Err(LlmError::Malformed(
                "tool call is missing its identifier or name".into(),
            ));
        }
        self.buffer(id.len() + name.len())?;
        let slot =
            u32::try_from(self.calls).map_err(|_| LlmError::Malformed("too many calls".into()))?;
        self.calls += 1;
        Ok(Block::Tool {
            slot,
            id,
            name,
            arguments: String::new(),
        })
    }

    fn delta(&mut self, event: &BlockDelta) -> Result<Vec<StreamChunk>, LlmError> {
        let kind = event
            .delta
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let field = match kind {
            "text_delta" => "text",
            "thinking_delta" => "thinking",
            "signature_delta" => "signature",
            "input_json_delta" => "partial_json",
            // Additive delta kinds carry nothing this adapter maps.
            _ => return Ok(Vec::new()),
        };
        let fragment = text(&event.delta, field)?;
        let buffered = !matches!(kind, "text_delta");
        if buffered {
            self.buffer(fragment.len())?;
        }
        let block = self
            .blocks
            .get_mut(&event.index)
            .ok_or_else(|| LlmError::Malformed("delta for an unknown content block".into()))?;
        match (block, kind) {
            (Block::Text, "text_delta") => Ok(vec![StreamChunk::TextDelta(fragment)]),
            (Block::Thinking { thinking, .. }, "thinking_delta") => {
                thinking.push_str(&fragment);
                Ok(vec![StreamChunk::ReasoningDelta(fragment)])
            }
            (Block::Thinking { signature, .. }, "signature_delta") => {
                signature.push_str(&fragment);
                Ok(Vec::new())
            }
            (Block::Tool { arguments, .. }, "input_json_delta") => {
                arguments.push_str(&fragment);
                Ok(Vec::new())
            }
            (Block::Ignored, _) => Ok(Vec::new()),
            _ => Err(LlmError::Malformed(
                "delta does not match its content block".into(),
            )),
        }
    }

    /// Emits a tool call whole, and a thinking block as opaque state when it can be replayed.
    fn stop(&mut self, event: &BlockStop) -> Result<Vec<StreamChunk>, LlmError> {
        let block = self
            .blocks
            .remove(&event.index)
            .ok_or_else(|| LlmError::Malformed("stop for an unknown content block".into()))?;
        Ok(match block {
            Block::Tool {
                slot,
                id,
                name,
                arguments,
            } => vec![StreamChunk::ToolCallDelta {
                index: slot,
                id: Some(id),
                name: Some(name),
                // A call without input streams no fragment; its input is the empty object.
                arguments_delta: if arguments.is_empty() {
                    "{}".into()
                } else {
                    arguments
                },
            }],
            Block::Thinking {
                thinking,
                signature,
            } if !signature.is_empty() => {
                let state = json!({"type":"thinking","thinking":thinking,"signature":signature});
                vec![StreamChunk::ReasoningState(state.to_string())]
            }
            Block::Redacted(data) => {
                let state = json!({"type":"redacted_thinking","data":data});
                vec![StreamChunk::ReasoningState(state.to_string())]
            }
            Block::Text | Block::Thinking { .. } | Block::Ignored => Vec::new(),
        })
    }

    fn finish(&mut self) -> Result<Vec<StreamChunk>, LlmError> {
        if !self.blocks.is_empty() {
            return Err(LlmError::Malformed(
                "a content block was unfinished when the message ended".into(),
            ));
        }
        self.done = true;
        let reason = match self.stop_reason.take().as_deref() {
            Some("end_turn" | "stop_sequence") => FinishReason::Stop,
            Some("tool_use") => FinishReason::ToolCalls,
            Some("max_tokens") => FinishReason::Length,
            Some("refusal") => FinishReason::ContentFilter,
            Some(other) => FinishReason::Other(other.into()),
            None => FinishReason::Other("incomplete".into()),
        };
        let mut chunks: Vec<_> = self
            .usage
            .usage()
            .map(StreamChunk::Usage)
            .into_iter()
            .collect();
        chunks.push(StreamChunk::Finish(reason));
        Ok(chunks)
    }
}

impl Wire for MessagesWire {
    fn data(&mut self, data: &str) -> Result<Vec<StreamChunk>, LlmError> {
        let value: Value = serde_json::from_str(data).map_err(|error| {
            LlmError::Malformed(format!(
                "invalid Messages event: {}",
                crate::redact(&error.to_string())
            ))
        })?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| LlmError::Malformed("invalid Messages event: no type".into()))?
            .to_owned();
        match kind.as_str() {
            "message_start" => {
                if let Some(usage) = parse::<Start>(value)?.message.usage {
                    self.usage.merge(&usage);
                }
                Ok(Vec::new())
            }
            "content_block_start" => self.start(&parse(value)?),
            "content_block_delta" => self.delta(&parse(value)?),
            "content_block_stop" => self.stop(&parse(value)?),
            "message_delta" => {
                let event: MessageDelta = parse(value)?;
                self.stop_reason = event.delta.stop_reason.or(self.stop_reason.take());
                if let Some(usage) = event.usage {
                    self.usage.merge(&usage);
                }
                Ok(Vec::new())
            }
            "message_stop" => self.finish(),
            "error" => Err(stream_error(
                value.get("error").and_then(|error| error.get("type")),
            )),
            _ => Ok(Vec::new()),
        }
    }

    fn done(&self) -> bool {
        self.done
    }
}

/// The usage one event reports, for the relay's usage reader: `message_start` carries the input
/// side and `message_delta` the output side. Other and invalid events report nothing.
pub(crate) fn usage_update(data: &str) -> Option<WireUsage> {
    let value: Value = serde_json::from_str(data).ok()?;
    match value.get("type")?.as_str()? {
        "message_start" => parse::<Start>(value).ok()?.message.usage,
        "message_delta" => parse::<MessageDelta>(value).ok()?.usage,
        _ => None,
    }
}

/// Maps a stream error by its type alone; the message is never read, since it can echo prompt
/// content. Request, account and billing types are permanent, so they never trigger a route switch.
fn stream_error(kind: Option<&Value>) -> LlmError {
    let kind = kind
        .and_then(Value::as_str)
        .filter(|kind| {
            !kind.is_empty()
                && kind.len() <= MAX_TYPE_BYTES
                && kind
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        })
        .unwrap_or("unknown");
    match kind {
        "rate_limit_error" => LlmError::RateLimited { retry_after: None },
        "overloaded_error" | "api_error" => LlmError::Unavailable { retry_after: None },
        _ => LlmError::Protocol(format!("model error: {kind}")),
    }
}
