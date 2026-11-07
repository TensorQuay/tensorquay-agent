//! The model provider contract and provider adapters, including server-sent event streaming,
//! retries, token limits and the model registry.

mod anthropic_messages;
mod codec;
mod collect;
mod error;
mod http;
pub mod instructions_role;
mod limiter;
mod openai_chat;
mod openai_responses;
mod provider;
mod provider_message;
mod registry;
pub mod relay_shaping;
mod request;
mod retry;
mod route;
mod sse;
mod stream;
mod time;
pub mod tool_schemas;
#[cfg(test)]
mod tool_schemas_tests;
mod transport;

pub use anthropic_messages::{MESSAGES_VERSION, MESSAGES_VERSION_HEADER};
pub use codec::{decode_response, encode_request};
pub use collect::{StreamAssembler, collect};
pub use error::{ConnectionFailure, LlmError, RELAY_THROTTLE_HEADER, ThrottleScope, TimeoutKind};
pub use http::{HttpConfig, Peer};
pub use limiter::{LimiterPermit, LimiterRefusal, QueueBounds, RouteLimiter};
pub use openai_chat::list_models;
pub use provider::{ModelProvider, provider_for, request_body_len};
// Diagnostic redaction lives in tqa-types, the lowest crate everything that logs engine or upstream text depends on.
pub use registry::{MAX_REGISTRY_BYTES, Registry, RegistryError, RouteInvalid};
pub use request::{CompletionRequest, ContentPart, Message, Role, ToolCall, ToolSpec};
pub use retry::{Jitter, RetryPolicy, RetryPolicyError, ThreadRngJitter, retry};
pub use route::{
    Auth, CacheHint, FIRST_RESPONSE_LOCAL, FIRST_RESPONSE_RANGE, FIRST_RESPONSE_REMOTE, Limits,
    Protocol, Reasoning, Route, RouteConfig,
};
pub use stream::{
    Completion, CompletionStream, FinishReason, PartialCompletion, StreamChunk, StreamLimitKind,
    StreamLimits, Usage,
};
pub use time::{SystemClock, TokioSleeper};
pub use tqa_types::MAX_FALLBACK_ROUTES;
pub use tqa_types::redact;
pub use transport::RUN_HEADER;

/// The reasoning-state item type that carries one Chat tool call's opaque extra (`extra_content`), kept as received
/// and sent back on that call (ADR-0027).
pub const CHAT_TOOL_CALL_EXTRA: &str = "chat.tool_call_extra";
