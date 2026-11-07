//! Typed streaming JSON envelopes and mappings to provider-independent chunks.

use crate::{FinishReason, LlmError, StreamChunk, Usage};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
}
#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    delta: Delta,
    finish_reason: Option<String>,
}
#[derive(Default, Deserialize)]
struct Delta {
    role: Option<String>,
    content: Option<String>,
    reasoning_content: Option<String>,
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Vec<CallDelta>,
}
#[derive(Deserialize)]
struct CallDelta {
    /// Absent on some compatible endpoints; see [`CallIndex`].
    index: Option<u32>,
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    function: Option<FunctionDelta>,
    /// Opaque per-call data some endpoints attach and require back (ADR-0027, Chat tool-call extras).
    extra_content: Option<serde_json::Value>,
}
#[derive(Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}
#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    prompt_tokens_details: Option<PromptDetails>,
    completion_tokens_details: Option<CompletionDetails>,
}
#[derive(Deserialize)]
struct PromptDetails {
    cached_tokens: Option<u64>,
}
#[derive(Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<u64>,
}

/// A stream's tool-call numbering when an endpoint leaves out `index` (LLM-U-042): a stream's only call may omit it and
/// is call 0, its later pieces continuing it; a second call without one, or calls with and without one in the same
/// stream, are refused, because their order would be a guess.
#[derive(Debug, Default)]
pub(crate) struct CallIndex {
    indexed: bool,
    unindexed: Unindexed,
    /// Each call's id once known, by index, so a later piece's extra is matched to its call.
    ids: std::collections::BTreeMap<u32, String>,
    /// Extras that came before their call's id, by index.
    waiting: std::collections::BTreeMap<u32, serde_json::Map<String, serde_json::Value>>,
}

/// The one call without an index, if any: its id once one was given, and whether it has its name.
#[derive(Debug, Default)]
enum Unindexed {
    #[default]
    None,
    Call {
        id: Option<String>,
        named: bool,
    },
}

impl CallIndex {
    /// The `chat.tool_call_extra` state a piece completes, if any: its call's extra once both it and the id are known.
    fn extra(
        &mut self,
        index: u32,
        id: Option<&str>,
        extra: Option<serde_json::Value>,
    ) -> Option<StreamChunk> {
        if let Some(id) = id {
            self.ids.insert(index, id.to_owned());
        }
        if let Some(serde_json::Value::Object(extra)) = extra {
            self.waiting.insert(index, extra);
        }
        let id = self.ids.get(&index)?;
        let extra = self.waiting.remove(&index)?;
        Some(StreamChunk::ReasoningState(
            serde_json::json!({"type": crate::CHAT_TOOL_CALL_EXTRA, "call_id": id, "extra_content": extra})
                .to_string(),
        ))
    }

    fn resolve(
        &mut self,
        index: Option<u32>,
        id: Option<&str>,
        name: Option<&str>,
    ) -> Result<u32, LlmError> {
        let refused = || {
            LlmError::Malformed(
                "tool calls without an index can't be ordered: more than one call, or calls with and without an index"
                    .into(),
            )
        };
        match (index, &mut self.unindexed) {
            (Some(_), Unindexed::Call { .. }) => Err(refused()),
            (Some(index), Unindexed::None) => {
                self.indexed = true;
                Ok(index)
            }
            (None, _) if self.indexed => Err(refused()),
            (None, slot @ Unindexed::None) => {
                *slot = Unindexed::Call {
                    id: id.map(str::to_owned),
                    named: name.is_some(),
                };
                Ok(0)
            }
            (None, Unindexed::Call { id: known, named }) => {
                // Another id starts another call; so does a second name, unless the piece repeats call 0's id.
                let same_id = known
                    .as_deref()
                    .zip(id)
                    .is_some_and(|(known, id)| known == id);
                let other_id = known
                    .as_deref()
                    .zip(id)
                    .is_some_and(|(known, id)| known != id);
                if other_id || (name.is_some() && *named && !same_id) {
                    return Err(refused());
                }
                if known.is_none() {
                    *known = id.map(str::to_owned);
                }
                *named |= name.is_some();
                Ok(0)
            }
        }
    }
}

/// One data field on its own, as a stream's first: for readers that need no stream state.
pub(crate) fn decode(data: &str) -> Result<Vec<StreamChunk>, LlmError> {
    decode_in(data, &mut CallIndex::default())
}

/// One data field within a stream whose tool-call numbering so far is `calls`.
pub(crate) fn decode_in(data: &str, calls: &mut CallIndex) -> Result<Vec<StreamChunk>, LlmError> {
    let envelope: Envelope = serde_json::from_str(data)
        .map_err(|error| LlmError::Malformed(crate::redact(&error.to_string())))?;
    let mut result = Vec::new();
    for choice in envelope.choices {
        if choice.index != 0 {
            return Err(LlmError::Malformed(
                "only a single completion choice is supported".into(),
            ));
        }
        // A chunk that carries nothing at all is one the server held back, for example while its own tool-call parser
        // buffers a call the model wrote in its native format (LLM-U-046). A role-only or empty-text chunk isn't one.
        let delta = &choice.delta;
        if delta.role.is_none()
            && delta.content.is_none()
            && delta.reasoning_content.is_none()
            && delta.reasoning.is_none()
            && delta.tool_calls.is_empty()
            && choice.finish_reason.is_none()
        {
            result.push(StreamChunk::Withheld);
        }
        if let Some(text) = choice.delta.content {
            result.push(StreamChunk::TextDelta(text));
        }
        if let Some(text) = choice.delta.reasoning_content.or(choice.delta.reasoning) {
            result.push(StreamChunk::ReasoningDelta(text));
        }
        for call in choice.delta.tool_calls {
            if call.kind.as_ref().is_some_and(|kind| kind != "function") {
                return Err(LlmError::Malformed("unsupported tool-call kind".into()));
            }
            let (name, arguments_delta) = call.function.map_or((None, String::new()), |function| {
                (function.name, function.arguments.unwrap_or_default())
            });
            let index = calls.resolve(call.index, call.id.as_deref(), name.as_deref())?;
            let extra = calls.extra(index, call.id.as_deref(), call.extra_content);
            result.push(StreamChunk::ToolCallDelta {
                index,
                id: call.id,
                name,
                arguments_delta,
            });
            result.extend(extra);
        }
        if let Some(reason) = choice.finish_reason {
            result.push(StreamChunk::Finish(match reason.as_str() {
                "stop" => FinishReason::Stop,
                "tool_calls" => FinishReason::ToolCalls,
                "length" => FinishReason::Length,
                "content_filter" => FinishReason::ContentFilter,
                _ => FinishReason::Other(reason),
            }));
        }
    }
    if let Some(usage) = envelope.usage {
        result.push(StreamChunk::Usage(Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            cached_tokens: usage
                .prompt_tokens_details
                .and_then(|details| details.cached_tokens),
            reasoning_tokens: usage
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens),
        }));
    }
    Ok(result)
}
