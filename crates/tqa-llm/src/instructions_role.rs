//! The `instructions_role` route setting: where a route's merged instructions go. The adapters always send the portable
//! default (Chat: one leading `system` message; Responses: the top-level `instructions`; Messages: the top-level
//! `system`), and the relay rewrites only that for a route that names another place (LLM-U-038, LLM-U-039).

use crate::Protocol;
use serde_json::{Map, Value, json};

/// Where a route's merged instructions go.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InstructionsRole {
    /// The protocol's own top-level field: Responses' `instructions`, Messages' `system`.
    #[default]
    TopLevel,
    /// One leading `system` message (Chat's default).
    System,
    /// One leading `developer` message.
    Developer,
}

impl InstructionsRole {
    /// The protocol's default.
    #[must_use]
    pub const fn default_for(protocol: Protocol) -> Self {
        match protocol {
            Protocol::OpenAiChat => Self::System,
            Protocol::OpenAiResponses | Protocol::AnthropicMessages => Self::TopLevel,
        }
    }
    /// The values a protocol accepts, as a route writes them.
    #[must_use]
    pub const fn allowed(protocol: Protocol) -> &'static [&'static str] {
        match protocol {
            Protocol::OpenAiChat => &["system", "developer"],
            Protocol::OpenAiResponses => &["top_level", "developer"],
            Protocol::AnthropicMessages => &["top_level"],
        }
    }
    /// The setting for a protocol: absent is the protocol's default; a value it doesn't accept is `None`.
    #[must_use]
    pub fn parse(protocol: Protocol, value: Option<&str>) -> Option<Self> {
        let Some(value) = value else {
            return Some(Self::default_for(protocol));
        };
        if !Self::allowed(protocol).contains(&value) {
            return None;
        }
        Some(match value {
            "system" => Self::System,
            "developer" => Self::Developer,
            _ => Self::TopLevel,
        })
    }
}

/// Refuses a value the protocol doesn't accept, naming the accepted ones.
pub(crate) fn validate(protocol: Protocol, value: Option<&str>) -> Result<(), crate::RouteInvalid> {
    if InstructionsRole::parse(protocol, value).is_some() {
        return Ok(());
    }
    Err(crate::RouteInvalid::InstructionsRoleInvalid(
        match protocol {
            Protocol::OpenAiChat => "system or developer",
            Protocol::OpenAiResponses => "top_level or developer",
            Protocol::AnthropicMessages => "top_level",
        },
    ))
}

/// Rewrites the default place of the instructions to the route's; the protocol's default changes nothing. The
/// instructions are never lost: on Responses they move only into an input list (a string input becomes its one user
/// message), and empty instructions stay as they are rather than becoming an empty developer message.
pub(crate) fn shape(protocol: Protocol, role: InstructionsRole, body: &mut Map<String, Value>) {
    if role == InstructionsRole::default_for(protocol) {
        return;
    }
    match (protocol, role) {
        // Only the first message, and only when it's the merged system message.
        (Protocol::OpenAiChat, InstructionsRole::Developer) => {
            if let Some(first) = body
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .and_then(|messages| messages.first_mut())
                .filter(|first| first["role"] == "system")
            {
                first["role"] = json!("developer");
            }
        }
        // Non-empty top-level instructions become one leading developer input message. A string input is the one
        // user message it stands for; a body with no input, or an input of another shape, keeps its instructions where
        // they are, and so do empty instructions (no empty developer message is made).
        (Protocol::OpenAiResponses, InstructionsRole::Developer) => {
            let text = match body.get("instructions") {
                Some(Value::String(text)) if !text.is_empty() => text.clone(),
                _ => return,
            };
            let mut input = match body.get("input") {
                Some(Value::Array(items)) => items.clone(),
                Some(Value::String(content)) => {
                    vec![json!({"type":"message","role":"user","content":content})]
                }
                _ => return,
            };
            input.insert(
                0,
                json!({"type":"message","role":"developer","content":text}),
            );
            body.remove("instructions");
            body.insert("input".into(), Value::Array(input));
        }
        // Validation admits no other pair.
        _ => {}
    }
}
