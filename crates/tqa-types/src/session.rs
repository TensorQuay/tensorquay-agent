//! Secret-free runtime-link configuration snapshots.
mod validation;
use crate::FrameError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The upstream wire protocol selected by a route; no adapter is constructed here.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WireProtocol {
    /// Chat-completions messages and tool calls.
    #[serde(rename = "openai_chat")]
    OpenAiChat,
    /// Responses requests and events.
    #[serde(rename = "openai_responses")]
    OpenAiResponses,
    /// Messages requests and content blocks.
    AnthropicMessages,
}

/// A route's declared caching capability; it never selects a provider by name.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CacheHint {
    /// Preserve stable prefixes for implicit cache reuse.
    Prefix,
    /// Support explicit cache-control blocks in the corresponding adapter.
    AnthropicCacheControl,
    /// No cache hint was declared; omitted on the wire.
    #[default]
    None,
}
impl CacheHint {
    /// Whether no hint was declared, so the field is left out of the wire form.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// Workspace facts and optional initialization; the runtime separately supplies a confined root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    /// Nonempty display text within 4096 UTF-8 bytes, without control characters.
    #[serde(deserialize_with = "validation::display")]
    #[schemars(length(min = 1, max = 4096))]
    pub display: String,
    /// Optional public repository to clone only when the workspace is initially empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clone: Option<crate::WorkspaceClone>,
}

/// A bounded set of reasoning choices with a default that belongs to the set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "ReasoningFields")]
pub struct ReasoningSnapshot {
    /// One through sixteen unique, nonempty values, each within 64 UTF-8 bytes.
    #[schemars(schema_with = "validation::reasoning_schema")]
    pub allowed: Vec<String>,
    /// The initially selected member of allowed.
    #[schemars(length(min = 1, max = 64))]
    pub default: String,
    /// The thinking token budget of every allowed value, for protocols that take a budget
    /// instead of an effort; each is 1024 through 1048576 and below the route's `max_output`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "validation::budget_schema")]
    pub budget_tokens: Option<BTreeMap<String, u32>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReasoningFields {
    #[schemars(schema_with = "validation::reasoning_schema")]
    allowed: Vec<String>,
    #[schemars(length(min = 1, max = 64))]
    default: String,
    #[serde(default)]
    #[schemars(schema_with = "validation::budget_schema")]
    budget_tokens: Option<BTreeMap<String, u32>>,
}
impl TryFrom<ReasoningFields> for ReasoningSnapshot {
    type Error = FrameError;
    fn try_from(fields: ReasoningFields) -> Result<Self, Self::Error> {
        let value = Self {
            allowed: fields.allowed,
            default: fields.default,
            budget_tokens: fields.budget_tokens,
        };
        value.validate()?;
        Ok(value)
    }
}

/// An immutable route snapshot with no credential, upstream endpoint or vendor settings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "RouteFields")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent route capabilities preserve the flat wire contract, as the registry's route does."
)]
pub struct RouteSnapshot {
    /// Nonempty logical registry name, bounded by the containing wire frame.
    #[schemars(length(min = 1))]
    pub name: String,
    /// The selected wire protocol.
    pub protocol: WireProtocol,
    /// The pinned model identifier, within 256 UTF-8 bytes.
    #[schemars(length(min = 1, max = 256))]
    pub model: String,
    /// Absolute ASCII relay path under /relay/r/, within 4096 bytes.
    #[schemars(length(min = 10, max = 4096))]
    pub relay_path: String,
    /// Positive context capacity when configured; otherwise the loop uses a conservative guess.
    #[schemars(range(min = 1))]
    pub context_window: Option<u32>,
    /// Positive output capacity, no larger than a configured context window.
    #[schemars(range(min = 1))]
    pub max_output: Option<u32>,
    /// Whether native structured calls are available.
    pub native_tools: bool,
    /// Whether the route accepts parallel calls in one response.
    pub parallel_tools: bool,
    /// Whether the route declares visual input support.
    pub vision: bool,
    /// Optional supported reasoning choices.
    pub reasoning: Option<ReasoningSnapshot>,
    /// The gateway's relay request body limit in bytes; absent from older gateways.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1024, max = 16_777_216))]
    pub max_request_bytes: Option<u32>,
    /// The route's caching capability; absent means none (revision 7).
    #[serde(default, skip_serializing_if = "CacheHint::is_none")]
    pub cache_hint: CacheHint,
    /// How long the relay waits for the model's first byte, 5 to 900 seconds (revision 10); the runtime's own wait
    /// follows it. Absent from an older gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 5, max = 900))]
    pub first_response_secs: Option<u32>,
    /// Whether the `computer` tool offers its coordinate actions on this route (revision 14, ADR-0039): an
    /// administrator's route setting, off by default, for a model whose grounding has passed its eval. Absent means
    /// off, and an older gateway never writes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pixel_grounding: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent route capabilities preserve the flat wire contract, as the registry's route does."
)]
struct RouteFields {
    #[schemars(length(min = 1))]
    name: String,
    protocol: WireProtocol,
    #[schemars(length(min = 1, max = 256))]
    model: String,
    #[schemars(length(min = 10, max = 4096))]
    relay_path: String,
    #[schemars(range(min = 1))]
    context_window: Option<u32>,
    #[schemars(range(min = 1))]
    max_output: Option<u32>,
    native_tools: bool,
    parallel_tools: bool,
    vision: bool,
    reasoning: Option<ReasoningSnapshot>,
    #[serde(default)]
    #[schemars(range(min = 1024, max = 16_777_216))]
    max_request_bytes: Option<u32>,
    #[serde(default)]
    cache_hint: CacheHint,
    #[serde(default)]
    #[schemars(range(min = 5, max = 900))]
    first_response_secs: Option<u32>,
    #[serde(default)]
    pixel_grounding: bool,
}
impl TryFrom<RouteFields> for RouteSnapshot {
    type Error = FrameError;
    fn try_from(fields: RouteFields) -> Result<Self, Self::Error> {
        let value = Self {
            name: fields.name,
            protocol: fields.protocol,
            model: fields.model,
            relay_path: fields.relay_path,
            context_window: fields.context_window,
            max_output: fields.max_output,
            native_tools: fields.native_tools,
            parallel_tools: fields.parallel_tools,
            vision: fields.vision,
            reasoning: fields.reasoning,
            max_request_bytes: fields.max_request_bytes,
            cache_hint: fields.cache_hint,
            first_response_secs: fields.first_response_secs,
            pixel_grounding: fields.pixel_grounding,
        };
        value.validate()?;
        Ok(value)
    }
}
