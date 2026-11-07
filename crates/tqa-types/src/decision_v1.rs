//! The `decision/v1` protocol (ADR-0013 point 5): what the loop sends an external decision engine, through the gateway
//! relay, and the one decision it answers. The request is bounded and carries the exposure's optional parts only when
//! the engine's history exposure allows them; the response is parsed strictly.
use crate::{DecisionPoint, RunId, ToolCallId, ToolStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The largest answer an engine may give, in bytes.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// The largest request the relay forwards to an engine, in bytes.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// A consultation: the step, what it may choose among, and what the exposure allows it to see.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Where the loop consults the engine.
    pub point: DecisionPoint,
    /// The run.
    pub run_id: RunId,
    /// The step, from 1.
    pub step: u32,
    /// The step's attempt, from 1.
    pub attempt: u32,
    /// The routes a `call_model` may name: the conversation's own first, then its declared fallbacks.
    pub routes: Vec<Route>,
    /// The step's tools.
    pub tools: Vec<Tool>,
    /// The run's budget use against its limits.
    pub budget: Budget,
    /// The previous step's calls in journal order.
    pub previous_calls: Vec<Call>,
    /// For `after_response`: the response's calls, by id and name, with arguments only under exposure `calls` or
    /// `full`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_calls: Vec<ResponseCall>,
    /// For `after_response` under exposure `full`: the response's visible text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_text: Option<String>,
    /// Under exposure `full`: the conversation as the model sees it, at most its last 64 KiB of text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Vec<TranscriptLine>>,
}

/// A route a decision may name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Route {
    /// The route's name.
    #[serde(deserialize_with = "crate::validation::route_name")]
    #[schemars(length(min = 1, max = 256))]
    pub name: String,
    /// Its context window in tokens, when declared.
    pub context_window: Option<u32>,
    /// Its output limit in tokens, when declared.
    pub max_output: Option<u32>,
    /// Whether it calls tools natively.
    pub native_tools: bool,
}

/// A tool of the step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// Its name.
    #[serde(deserialize_with = "crate::decision::tool")]
    #[schemars(schema_with = "crate::tool_name::tool_name_schema")]
    pub name: String,
    /// Its description.
    pub description: String,
    /// Its parameter schema.
    pub parameters: Value,
}

/// The run's budget use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Steps completed.
    pub steps_used: u32,
    /// The step limit.
    pub max_steps: u32,
    /// Output tokens spent.
    pub output_tokens_used: u64,
    /// The output-token limit.
    pub max_output_tokens: u64,
}

/// A call of the previous step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Call {
    /// The tool, a valid tool name, so that under exposure `none` a name can't carry the conversation's text.
    #[serde(deserialize_with = "crate::decision::tool")]
    #[schemars(schema_with = "crate::tool_name::tool_name_schema")]
    pub name: String,
    /// Its terminal status, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ToolStatus>,
    /// Its arguments, under exposure `calls` or `full`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// A call of the response being selected from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResponseCall {
    /// The call's id, which a `select` names.
    pub id: ToolCallId,
    /// The tool, a valid tool name, so that under exposure `none` a name can't carry the conversation's text.
    #[serde(deserialize_with = "crate::decision::tool")]
    #[schemars(schema_with = "crate::tool_name::tool_name_schema")]
    pub name: String,
    /// Its arguments, under exposure `calls` or `full`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// One message of the transcript.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TranscriptLine {
    /// `user`, `assistant`, `tool` or `system`.
    pub role: String,
    /// The message's text.
    pub text: String,
}

/// The engine's answer: exactly one decision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    /// Call `route` with these tools.
    CallModel {
        /// The conversation's own route or a declared fallback.
        #[serde(deserialize_with = "crate::validation::route_name")]
        #[schemars(length(min = 1, max = 256))]
        route: String,
        /// A subset of the step's tools.
        #[serde(deserialize_with = "crate::decision::tools")]
        #[schemars(schema_with = "crate::decision::tools_schema")]
        tools: Vec<String>,
    },
    /// Run one call without a model call.
    Act {
        /// A tool of the step.
        #[serde(deserialize_with = "crate::decision::tool")]
        #[schemars(schema_with = "crate::tool_name::tool_name_schema")]
        tool: String,
        /// Its arguments, a JSON object.
        #[serde(deserialize_with = "object")]
        #[schemars(schema_with = "crate::decision::object_schema")]
        arguments: Value,
    },
    /// Compact the conversation now. A struct variant, so an unknown field is refused like everywhere else.
    Compact {},
    /// Stop the run as failed.
    Stop {
        /// 1 through 200 characters, no control characters.
        #[serde(deserialize_with = "crate::decision::stop_reason")]
        #[schemars(schema_with = "crate::decision::stop_reason_schema")]
        reason: String,
    },
    /// Keep these calls of the response.
    Select {
        /// At most 64 call ids.
        #[serde(deserialize_with = "crate::decision::call_ids")]
        #[schemars(length(max = 64))]
        call_ids: Vec<ToolCallId>,
    },
}

/// Why an answer isn't a decision.
#[derive(Debug, thiserror::Error)]
pub enum ResponseError {
    /// Over [`MAX_RESPONSE_BYTES`].
    #[error("decision/v1: the answer is over 64 KiB")]
    TooLarge,
    /// Not exactly one valid decision.
    #[error("decision/v1: not a decision: {0}")]
    Invalid(#[from] serde_json::Error),
}

/// Parses an engine's answer: at most 64 KiB, exactly one decision, nothing unknown.
///
/// # Errors
/// [`ResponseError`] for an oversized answer or one that isn't exactly one valid decision.
pub fn parse_response(bytes: &[u8]) -> Result<Response, ResponseError> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(ResponseError::TooLarge);
    }
    Ok(serde_json::from_slice(bytes)?)
}

fn object<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
    let value = Value::deserialize(deserializer)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "decision/v1: act arguments must be an object",
        ))
    }
}
