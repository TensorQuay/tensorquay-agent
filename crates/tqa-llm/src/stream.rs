//! Shared streaming values and explicit assembly limits.

use crate::{LlmError, ToolCall};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// Maximum allocations permitted while parsing and collecting a response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamLimits {
    /// Maximum encoded bytes in one SSE event, including its field lines.
    pub max_sse_event_bytes: usize,
    /// Maximum combined text, reasoning, tool-id, tool-name and argument bytes.
    pub max_completion_bytes: usize,
    /// Maximum distinct tool calls, independent of their numeric indices.
    pub max_tool_calls: usize,
}

impl StreamLimits {
    /// Suggested limits; callers must select them explicitly.
    pub const RECOMMENDED: Self = Self {
        max_sse_event_bytes: 1024 * 1024,
        max_completion_bytes: 16 * 1024 * 1024,
        max_tool_calls: 128,
    };
}

/// A stream bound exceeded by untrusted response data.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StreamLimitKind {
    /// Encoded SSE event bytes.
    #[error("SSE event")]
    SseEvent,
    /// Accumulated completion bytes.
    #[error("completion")]
    Completion,
    /// Distinct tool-call count.
    #[error("tool calls")]
    ToolCalls,
}

/// Usage values preserve absent optional counters as None.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens reported by the server.
    pub prompt_tokens: u64,
    /// Output tokens reported by the server.
    pub completion_tokens: u64,
    /// Cached input tokens, when reported.
    pub cached_tokens: Option<u64>,
    /// Reasoning output tokens, when reported.
    pub reasoning_tokens: Option<u64>,
}

/// The server's terminal completion reason.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FinishReason {
    /// Normal completion or an explicit stop sequence.
    Stop,
    /// Execution should continue with tool results.
    ToolCalls,
    /// The output token limit was reached.
    Length,
    /// Output was withheld by a content filter.
    ContentFilter,
    /// An additive reason that the adapter does not interpret.
    Other(String),
}

/// One incremental provider result; call argument deltas are indexed, not positional.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StreamChunk {
    /// Visible text to append.
    TextDelta(String),
    /// Reasoning text to append.
    ReasoningDelta(String),
    /// One fragment of a structured call.
    ToolCallDelta {
        /// The call index within this completion.
        index: u32,
        /// The call identifier, normally supplied once.
        id: Option<String>,
        /// The tool name, normally supplied once.
        name: Option<String>,
        /// JSON argument text to append verbatim.
        arguments_delta: String,
    },
    /// One complete opaque reasoning item as JSON object text (ADR-0027).
    ReasoningState(String),
    /// Usage, often sent after the finish reason.
    Usage(Usage),
    /// The terminal model reason.
    Finish(FinishReason),
    /// A chunk that carried nothing: the server held that part of the reply back (a server-side tool-call parser
    /// buffering what the model wrote, for example). Counted, never shown.
    Withheld,
}

/// A cancellable stream whose owner releases network resources by dropping it.
pub type CompletionStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>;

/// Accumulated response values; the finish reason distinguishes complete from partial data.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Completion {
    /// Concatenated visible text.
    pub text: String,
    /// Concatenated reasoning text.
    pub reasoning: String,
    /// Tool calls sorted by their stream indices.
    pub tool_calls: Vec<ToolCall>,
    /// Opaque reasoning items in stream order.
    pub reasoning_state: Vec<String>,
    /// The last reported usage, if any.
    pub usage: Option<Usage>,
    /// A terminal reason, if the server supplied one.
    pub finish_reason: Option<FinishReason>,
    /// Chunks the server held back ([`StreamChunk::Withheld`]); a reply that delivered no call but has some was
    /// probably swallowed by the server's own tool-call parser.
    pub withheld_chunks: u32,
}

/// An owned completion snapshot retained on a cut, boxed to keep ordinary errors small.
pub type PartialCompletion = Box<Completion>;
