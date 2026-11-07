//! Gateway request shaping and usage observation for a transparent protocol relay.

use crate::{LlmError, Protocol, Route, StreamChunk, Usage};
use serde_json::{Map, Value};

#[cfg(test)]
mod tests;

/// Applies route policy while retaining standard request fields and unknown round-trip data.
///
/// The model is pinned and route extras apply over the request, merging key by key where both
/// sides hold an object. For Chat, reasoning uses the exact configured field name and routes
/// reporting usage always request it in stream options. For Responses, the effort goes in
/// `reasoning.effort`, streaming is forced and `store` is always false (the history is ours).
/// For Chat and Responses, each tool's `strict` and schema follow the route's tool-schema settings.
/// For Messages, streaming is forced and `thinking` is only checked: enabled thinking must carry a
/// budget the route maps, adaptive thinking needs a route without budgets (whose effort, in
/// `output_config.effort`, must be allowed and defaults to the route's), and a request without
/// thinking (a lost reasoning state) stays without it. Thinking blocks, signatures and cache
/// markers pass through unchanged.
///
/// # Errors
/// Rejects unsupported protocols, invalid routes, non-object bodies and invalid reasoning/options.
pub fn shape_request(route: &Route, body: &mut Value) -> Result<(), LlmError> {
    match route.protocol {
        Protocol::OpenAiChat => {}
        Protocol::OpenAiResponses => return shape_responses(route, body),
        Protocol::AnthropicMessages => return shape_messages(route, body),
    }
    crate::registry::validate_route(route)
        .map_err(|reason| LlmError::Protocol(reason.to_string()))?;
    let body = body
        .as_object_mut()
        .ok_or_else(|| LlmError::Protocol("relay body must be an object".into()))?;
    let reasoning = reasoning(route, body)?;
    // The runtime always asks for usage; a route set to `stream_usage = false` (a server that refuses the field)
    // has the request sent without it.
    if route.stream_usage() {
        include_usage(body)?;
    } else {
        body.remove("stream_options");
    }
    body.insert("model".into(), Value::String(route.model.clone()));
    shape_content(route, body);
    crate::request::merge_over(body, &route.extra_body);
    if let Some((field, value)) = reasoning {
        body.remove("reasoning_effort");
        body.insert(field, Value::String(value));
    }
    Ok(())
}

/// The route's content shaping, in one fixed order, just before the route's extras: the instructions role, then the
/// tool schemas. Each touches its own part of the body (the leading instructions; the tools); extras can set neither.
fn shape_content(route: &Route, body: &mut Map<String, Value>) {
    crate::instructions_role::shape(route.protocol, route.instructions_role(), body);
    if route.protocol != Protocol::AnthropicMessages
        && let Some(tools) = body.get_mut("tools")
    {
        crate::tool_schemas::shape_tool_list(route.strict_tools(), route.schema_keywords(), tools);
    }
}

fn reasoning(
    route: &Route,
    body: &Map<String, Value>,
) -> Result<Option<(String, String)>, LlmError> {
    let Some(policy) = &route.reasoning else {
        return Ok(None);
    };
    let effort = match body.get("reasoning_effort") {
        Some(value) => value
            .as_str()
            .ok_or_else(|| LlmError::Protocol("reasoning effort must be a string".into()))?,
        None => &policy.default,
    };
    if !policy.allowed.iter().any(|allowed| allowed == effort) {
        return Err(LlmError::Protocol(
            "reasoning effort is not allowed by the route".into(),
        ));
    }
    Ok(Some((policy.field.clone(), effort.into())))
}

fn shape_responses(route: &Route, body: &mut Value) -> Result<(), LlmError> {
    crate::registry::validate_route(route)
        .map_err(|reason| LlmError::Protocol(reason.to_string()))?;
    let body = body
        .as_object_mut()
        .ok_or_else(|| LlmError::Protocol("relay body must be an object".into()))?;
    let effort = match &route.reasoning {
        None => None,
        Some(policy) => {
            let requested = match body.get("reasoning") {
                None | Some(Value::Null) => None,
                Some(Value::Object(reasoning)) => reasoning.get("effort"),
                Some(_) => {
                    return Err(LlmError::Protocol("reasoning must be an object".into()));
                }
            };
            let effort = match requested {
                None | Some(Value::Null) => &policy.default,
                Some(value) => value.as_str().ok_or_else(|| {
                    LlmError::Protocol("reasoning effort must be a string".into())
                })?,
            };
            if !policy.allowed.iter().any(|allowed| allowed == effort) {
                return Err(LlmError::Protocol(
                    "reasoning effort is not allowed by the route".into(),
                ));
            }
            Some(effort.to_owned())
        }
    };
    body.insert("model".into(), Value::String(route.model.clone()));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("store".into(), Value::Bool(false));
    shape_content(route, body);
    crate::request::merge_over(body, &route.extra_body);
    if let Some(effort) = effort {
        let reasoning = body
            .entry("reasoning")
            .or_insert_with(|| Value::Object(Map::new()));
        if reasoning.is_null() {
            *reasoning = Value::Object(Map::new());
        }
        if let Some(reasoning) = reasoning.as_object_mut() {
            reasoning.insert("effort".into(), Value::String(effort));
        }
    }
    Ok(())
}

fn shape_messages(route: &Route, body: &mut Value) -> Result<(), LlmError> {
    crate::registry::validate_route(route)
        .map_err(|reason| LlmError::Protocol(reason.to_string()))?;
    let body = body
        .as_object_mut()
        .ok_or_else(|| LlmError::Protocol("relay body must be an object".into()))?;
    check_thinking(route, body.get("thinking"))?;
    adaptive_effort(route, body)?;
    body.insert("model".into(), Value::String(route.model.clone()));
    body.insert("stream".into(), Value::Bool(true));
    shape_content(route, body);
    crate::request::merge_over(body, &route.extra_body);
    Ok(())
}

/// Enabled thinking must use one of the route's budgets; disabled or absent thinking passes.
fn check_thinking(route: &Route, thinking: Option<&Value>) -> Result<(), LlmError> {
    let refused = |detail: &str| LlmError::Protocol(format!("thinking {detail}"));
    let thinking = match thinking {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Object(thinking)) => thinking,
        Some(_) => return Err(refused("must be an object")),
    };
    match thinking.get("type").and_then(Value::as_str) {
        Some("disabled") => Ok(()),
        Some("adaptive") => match &route.reasoning {
            Some(reasoning) if reasoning.budget_tokens.is_none() => Ok(()),
            _ => Err(refused("is not adaptive on this route")),
        },
        Some("enabled") => {
            let budgets = route
                .reasoning
                .as_ref()
                .and_then(|reasoning| reasoning.budget_tokens.as_ref())
                .ok_or_else(|| refused("is not enabled on this route"))?;
            let budget = thinking.get("budget_tokens").and_then(Value::as_u64);
            if budget
                .is_some_and(|budget| budgets.values().any(|mapped| u64::from(*mapped) == budget))
            {
                Ok(())
            } else {
                Err(refused("budget is not one the route maps"))
            }
        }
        _ => Err(refused("type is not supported")),
    }
}

/// On an adaptive-thinking route (a reasoning policy without budgets), `output_config.effort` is one
/// of the route's allowed values, the route's default when the request names none.
fn adaptive_effort(route: &Route, body: &mut Map<String, Value>) -> Result<(), LlmError> {
    let Some(policy) = route
        .reasoning
        .as_ref()
        .filter(|policy| policy.budget_tokens.is_none())
    else {
        return Ok(());
    };
    let refused = |detail: &str| LlmError::Protocol(format!("output_config {detail}"));
    let config = body
        .entry("output_config")
        .or_insert_with(|| Value::Object(Map::new()));
    if config.is_null() {
        *config = Value::Object(Map::new());
    }
    let config = config
        .as_object_mut()
        .ok_or_else(|| refused("must be an object"))?;
    let effort = match config.get("effort") {
        None | Some(Value::Null) => policy.default.clone(),
        Some(Value::String(effort)) => effort.clone(),
        Some(_) => return Err(refused("effort must be a string")),
    };
    if !policy.allowed.contains(&effort) {
        return Err(LlmError::Protocol(
            "reasoning effort is not allowed by the route".into(),
        ));
    }
    config.insert("effort".into(), Value::String(effort));
    Ok(())
}

fn include_usage(body: &mut Map<String, Value>) -> Result<(), LlmError> {
    let options = body
        .entry("stream_options")
        .or_insert_with(|| Value::Object(Map::new()));
    if options.is_null() {
        *options = Value::Object(Map::new());
    }
    let options = options
        .as_object_mut()
        .ok_or_else(|| LlmError::Protocol("stream options must be an object".into()))?;
    options.insert("include_usage".into(), Value::Bool(true));
    Ok(())
}

/// The usage of one relayed response, observed event by event without modifying response bytes.
///
/// Chat and Responses report usage whole in one event, so the latest report wins, exactly as
/// [`read_usage`] reads it. Messages splits it: `message_start` carries the input side and
/// `message_delta` the output side, so the reader combines them. Invalid events change nothing.
///
/// It also counts what the response streamed ([`StreamedBytes`]), event by event, so a response that ends before its
/// usage arrives (a cancelled step) can still be estimated. Nothing is held back for it: the adapters' wires buffer a
/// thinking block or a call's arguments until it ends, which a cancelled step never reaches. An event that doesn't
/// parse adds nothing.
pub struct UsageReader {
    protocol: Protocol,
    latest: Option<Usage>,
    messages: crate::anthropic_messages::MessagesUsage,
    calls: crate::openai_chat::wire::CallIndex,
    streamed: StreamedBytes,
}

/// The output bytes a relayed response streamed, counted as the runtime's output meter counts them: text and
/// reasoning deltas, and a tool call's id, name and argument fragments. `reasoning` is the part of `output` that was
/// reasoning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamedBytes {
    /// Every counted byte.
    pub output: usize,
    /// The reasoning deltas' bytes, also in `output`.
    pub reasoning: usize,
}

impl StreamedBytes {
    fn add(&mut self, bytes: usize, reasoning: bool) {
        self.output = self.output.saturating_add(bytes);
        if reasoning {
            self.reasoning = self.reasoning.saturating_add(bytes);
        }
    }

    fn count(&mut self, chunk: &StreamChunk) {
        let (bytes, reasoning) = match chunk {
            StreamChunk::TextDelta(text) => (text.len(), 0),
            StreamChunk::ReasoningDelta(text) => (text.len(), text.len()),
            StreamChunk::ToolCallDelta {
                id,
                name,
                arguments_delta,
                ..
            } => (
                arguments_delta
                    .len()
                    .saturating_add(id.as_ref().map_or(0, String::len))
                    .saturating_add(name.as_ref().map_or(0, String::len)),
                0,
            ),
            _ => (0, 0),
        };
        self.output = self.output.saturating_add(bytes);
        self.reasoning = self.reasoning.saturating_add(reasoning);
    }
}

impl UsageReader {
    /// A reader for one response on a route of this protocol.
    #[must_use]
    pub fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            latest: None,
            messages: crate::anthropic_messages::MessagesUsage::default(),
            calls: crate::openai_chat::wire::CallIndex::default(),
            streamed: StreamedBytes::default(),
        }
    }

    /// Observes one SSE event's data.
    pub fn observe(&mut self, sse_event_json: &str) {
        self.count(sse_event_json);
        if self.protocol == Protocol::AnthropicMessages {
            if let Some(update) = crate::anthropic_messages::usage_update(sse_event_json) {
                self.messages.merge(&update);
            }
        } else if let Some(usage) = read_usage(self.protocol, sse_event_json) {
            self.latest = Some(usage);
        }
    }

    /// The usage observed so far; `None` until the server reported it.
    #[must_use]
    pub fn usage(&self) -> Option<Usage> {
        match self.protocol {
            Protocol::AnthropicMessages => self.messages.usage(),
            _ => self.latest.clone(),
        }
    }

    /// Adds one event's streamed output: Chat through its own decoder (deltas arrive whole), Responses and Messages
    /// from their delta events' fields.
    fn count(&mut self, data: &str) {
        match self.protocol {
            Protocol::OpenAiChat => self.count_chat(data),
            Protocol::OpenAiResponses | Protocol::AnthropicMessages => {
                let Ok(event) = serde_json::from_str::<Value>(data) else {
                    return;
                };
                for (bytes, reasoning) in streamed_parts(self.protocol, &event) {
                    self.streamed.add(bytes, reasoning);
                }
            }
        }
    }

    /// A Chat record through the adapter's own decoder; the `[DONE]` sentinel carries nothing.
    fn count_chat(&mut self, data: &str) {
        if data.trim() == "[DONE]" {
            return;
        }
        let Ok(chunks) = crate::openai_chat::wire::decode_in(data, &mut self.calls) else {
            return;
        };
        for chunk in &chunks {
            self.streamed.count(chunk);
        }
    }

    /// The output bytes streamed so far.
    #[must_use]
    pub fn streamed(&self) -> StreamedBytes {
        self.streamed
    }
}

/// One Responses or Messages event's output fragments as (bytes, is reasoning): text, reasoning and refusal deltas, a
/// call's argument fragments, and a call's id and name when it opens (as the runtime's output meter counts them).
fn streamed_parts(protocol: Protocol, event: &Value) -> Vec<(usize, bool)> {
    let len = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).map_or(0, str::len);
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match (protocol, kind) {
        (
            Protocol::OpenAiResponses,
            "response.output_text.delta"
            | "response.refusal.delta"
            | "response.function_call_arguments.delta",
        ) => {
            vec![(len(event, "delta"), false)]
        }
        (
            Protocol::OpenAiResponses,
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta",
        ) => {
            vec![(len(event, "delta"), true)]
        }
        (Protocol::OpenAiResponses, "response.output_item.added") => match event.get("item") {
            Some(item) if item.get("type").and_then(Value::as_str) == Some("function_call") => {
                vec![(len(item, "call_id") + len(item, "name"), false)]
            }
            _ => Vec::new(),
        },
        (Protocol::AnthropicMessages, "content_block_start") => match event.get("content_block") {
            Some(block) => match block.get("type").and_then(Value::as_str) {
                Some("text") => vec![(len(block, "text"), false)],
                Some("thinking") => vec![(len(block, "thinking"), true)],
                Some("tool_use") => vec![(len(block, "id") + len(block, "name"), false)],
                _ => Vec::new(),
            },
            None => Vec::new(),
        },
        (Protocol::AnthropicMessages, "content_block_delta") => match event.get("delta") {
            Some(delta) => match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => vec![(len(delta, "text"), false)],
                Some("thinking_delta") => vec![(len(delta, "thinking"), true)],
                Some("input_json_delta") => vec![(len(delta, "partial_json"), false)],
                _ => Vec::new(),
            },
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Extracts the usage one event reports through the existing protocol decoder, without modifying
/// response bytes. Invalid events and events without usage return `None`. Messages reports usage
/// across several events, so it always returns `None` here; use [`UsageReader`].
#[must_use]
pub fn read_usage(protocol: Protocol, sse_event_json: &str) -> Option<Usage> {
    match protocol {
        Protocol::OpenAiChat => {}
        Protocol::OpenAiResponses => {
            return crate::openai_responses::terminal_usage(sse_event_json);
        }
        Protocol::AnthropicMessages => return None,
    }
    crate::openai_chat::wire::decode(sse_event_json)
        .ok()?
        .into_iter()
        .find_map(|chunk| {
            if let StreamChunk::Usage(usage) = chunk {
                Some(usage)
            } else {
                None
            }
        })
}
