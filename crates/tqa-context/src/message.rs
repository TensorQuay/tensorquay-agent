//! Provider-independent messages; the core maps them into provider requests.
use serde::{Deserialize, Serialize};
use tqa_types::{ToolCallId, WireProtocol};

/// The speaker or tool-response role of a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Fixed instructions or tool definitions.
    System,
    /// User input.
    User,
    /// A completed model response.
    Assistant,
    /// An authoritative tool response.
    Tool,
}

/// A prepared call with its original JSON argument text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallRecord {
    /// The identifier used to join its tool result.
    pub call_id: ToolCallId,
    /// The requested tool name.
    pub name: String,
    /// JSON argument text, without provider-specific wrapping.
    pub arguments: String,
}

/// One opaque reasoning state recorded with a response (ADR-0027). The core replays `item` only
/// to the same route and model; a state without `item` exceeded its bounds when recorded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReasoningStateRecord {
    /// The protocol of the route that produced it.
    pub protocol: WireProtocol,
    /// The route that produced it.
    pub route: String,
    /// The route's model when it was produced.
    pub model: String,
    /// The provider item as JSON text, or `None` when it was not kept.
    pub item: Option<String>,
}

/// One projected conversation message with separate reasoning and tool fields.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// The message's role.
    pub role: Role,
    /// Visible text or tool-result content.
    pub text: String,
    /// Model reasoning, kept separate from visible text.
    pub reasoning: String,
    /// Prepared calls in model order.
    pub tool_calls: Vec<CallRecord>,
    /// The call answered by a tool message.
    pub tool_call_id: Option<ToolCallId>,
    /// Opaque reasoning states of an assistant response, in stream order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_states: Vec<ReasoningStateRecord>,
}
impl Message {
    /// Creates a text message with empty reasoning and no tool fields.
    #[must_use]
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
            reasoning: String::new(),
            tool_calls: vec![],
            tool_call_id: None,
            reasoning_states: Vec::new(),
        }
    }
}
