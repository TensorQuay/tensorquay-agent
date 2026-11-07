//! Provider-independent completion inputs.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A conversation participant's message role.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Role {
    /// Shared run instructions.
    System,
    /// A user input.
    User,
    /// A model response, possibly containing tool calls.
    Assistant,
    /// The result of one prior tool call.
    Tool,
}

/// One ordered content part; additional modalities require an adapter extension.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ContentPart {
    /// UTF-8 message text.
    Text(String),
    /// Raw image bytes of a recognised format, sent only to routes with `vision`.
    Image {
        /// The format recognised from the bytes.
        media_type: tqa_types::ImageMediaType,
        /// The image file's bytes, never logged.
        data: Vec<u8>,
    },
    /// One opaque provider reasoning item as JSON object text, replayed unchanged on the
    /// assistant message that produced it (ADR-0027). Protocols without such items leave it out.
    ReasoningState(String),
}

/// A completed tool call with its original JSON argument representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCall {
    /// The server's tool-call identifier.
    pub id: String,
    /// The registered tool name.
    pub name: String,
    /// The assembled JSON argument document.
    pub arguments: String,
}

/// One conversation message; serialization preserves message order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    /// The message role.
    pub role: Role,
    /// Content parts in order.
    pub content: Vec<ContentPart>,
    /// Structured calls made by an assistant message.
    pub tool_calls: Vec<ToolCall>,
    /// The call answered by a tool-result message.
    pub tool_call_id: Option<String>,
    /// An optional participant or tool name.
    pub name: Option<String>,
}

/// A function tool whose input schema is passed through unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSpec {
    /// The callable tool name.
    pub name: String,
    /// The description supplied to the model.
    pub description: String,
    /// The JSON Schema for arguments.
    pub parameters: serde_json::Value,
    /// Whether strict schema adherence is requested.
    pub strict: bool,
}

/// A streaming completion request independent of its HTTP protocol.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompletionRequest {
    /// Ordered conversation history.
    pub messages: Vec<Message>,
    /// Available tools in caller-selected stable order.
    pub tools: Vec<ToolSpec>,
    /// Optional route-supported reasoning effort.
    pub reasoning_effort: Option<String>,
    /// Optional request-specific output token limit.
    pub max_output: Option<u32>,
    /// Optional sampling temperature.
    pub temperature: Option<f32>,
    /// Explicit stop sequences.
    pub stop: Vec<String>,
    /// Deterministically ordered diagnostic metadata, excluding credentials.
    pub metadata: BTreeMap<String, String>,
    /// Declare `tools` natively although the route writes calls as text: the one retry after the server's own
    /// tool-call parser took a call the model wrote in its native format, so the call arrives as native `tool_calls`
    /// (Chat only; Gate 5, 29 Sep).
    pub declare_tools: bool,
    /// The run this call belongs to, sent to the gateway's relay as `X-TQA-Run-ID` so its usage meter and secret-read
    /// audit name the run; never sent to a provider.
    pub run_id: Option<tqa_types::RunId>,
}

/// Checks the tool catalog against the shared count and byte budget.
pub(crate) fn check_catalog(tools: &[ToolSpec]) -> Result<(), crate::LlmError> {
    use tqa_types::{ToolCatalogBudget, ToolCatalogEntry, ToolCatalogLimit};
    if tools.len() > tqa_types::MAX_TOOLS {
        return Err(ToolCatalogLimit::Count.into());
    }
    let mut budget = ToolCatalogBudget::default();
    for tool in tools {
        budget = budget.include(ToolCatalogEntry {
            name: &tool.name,
            description: &tool.description,
            parameters: &tool.parameters,
            strict: tool.strict,
        })?;
    }
    Ok(())
}

/// The model's original argument text: a stored JSON string is unwrapped to its raw content.
pub(crate) fn raw_arguments(arguments: &str) -> String {
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::String(raw)) => raw,
        _ => arguments.into(),
    }
}

/// Adds the route's `extra_body` fields that the request does not set. Where both sides hold an
/// object, the objects merge the same way, key by key; the request's own values always win.
pub(crate) fn merge_absent(body: &mut Value, extra: &BTreeMap<String, Value>) {
    if let Some(body) = body.as_object_mut() {
        for (name, value) in extra {
            merge_field(body, name, value, false);
        }
    }
}

/// Applies the route's `extra_body` over a relayed request. Where both sides hold an object, the
/// objects merge key by key; any other route value replaces the request's.
pub(crate) fn merge_over(body: &mut Map<String, Value>, extra: &BTreeMap<String, Value>) {
    for (name, value) in extra {
        merge_field(body, name, value, true);
    }
}

fn merge_field(body: &mut Map<String, Value>, name: &str, value: &Value, replace: bool) {
    match (body.get_mut(name), value) {
        (Some(Value::Object(target)), Value::Object(extra)) => {
            for (name, value) in extra {
                merge_field(target, name, value, replace);
            }
        }
        (Some(_), _) if !replace => {}
        _ => {
            body.insert(name.into(), value.clone());
        }
    }
}

/// The marker that opens a system notice sent after the conversation started, as user text.
pub(crate) const NOTICE_PREFIX: &str = "[System notice] ";

/// Splits off the leading system messages (the system prompt, instructions and tool slots): their texts, and the rest
/// of the history.
pub(crate) fn leading_system(messages: &[crate::Message]) -> (Vec<String>, &[crate::Message]) {
    let leading = messages
        .iter()
        .take_while(|message| message.role == crate::Role::System)
        .count();
    let (system, rest) = messages.split_at(leading);
    (system.iter().map(plain_text).collect(), rest)
}

/// A system message's text parts, in order.
pub(crate) fn plain_text(message: &crate::Message) -> String {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            crate::ContentPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// A tool result that carries images (a `computer` screenshot, ADR-0039), split for a protocol whose tool results take
/// text only: the result as its text, and a user message holding a label and the images, which goes after the run
/// of tool results it belongs to. `None` for a message without images.
pub(crate) fn tool_images(message: &crate::Message) -> Option<(crate::Message, crate::Message)> {
    if message.role != Role::Tool
        || !message
            .content
            .iter()
            .any(|part| matches!(part, crate::ContentPart::Image { .. }))
    {
        return None;
    }
    let (images, text): (Vec<_>, Vec<_>) = message
        .content
        .iter()
        .cloned()
        .partition(|part| matches!(part, crate::ContentPart::Image { .. }));
    let label = format!(
        "The image from the result of tool call {}:",
        message.tool_call_id.as_deref().unwrap_or_default()
    );
    let result = crate::Message {
        content: text,
        ..message.clone()
    };
    let shown = crate::Message {
        role: Role::User,
        content: std::iter::once(crate::ContentPart::Text(label))
            .chain(images)
            .collect(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        name: None,
    };
    Some((result, shown))
}
