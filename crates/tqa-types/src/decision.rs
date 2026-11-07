//! The decision engine's record in the journal (ADR-0013 point 4): where it was consulted, what it chose as applied,
//! and why it was set aside.
use crate::ToolCallId;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;

/// The most tools a `call_model` choice names.
pub const MAX_DECISION_TOOLS: usize = 256;
/// The most calls a `select` choice names, the core's per-response bound.
pub const MAX_DECISION_CALLS: usize = 64;
/// The longest `stop` reason, in characters.
pub const MAX_DECISION_STOP_REASON_CHARS: usize = 200;
/// The largest `act` arguments, serialized, in bytes: the bound of an engine's whole answer.
pub const MAX_DECISION_ACT_BYTES: usize = 64 * 1024;

/// The longest time an engine may be given to answer, in milliseconds (ADR-0013 point 5).
pub const MAX_DECISION_TIMEOUT_MS: u32 = 10_000;
/// The longest decision engine name, in bytes.
pub const MAX_DECISION_ENGINE_NAME_BYTES: usize = 64;
/// The revision that carries `decision_engine` in `session.config` (ADR-0013 point 7); an older runtime never gets it.
pub const DECISION_ENGINE_REVISION: u16 = 9;

/// Whether `name` is a decision engine name: a lower-case letter, then lower-case letters, digits, `-` or `_`, at most
/// 64 bytes. It names the engine's relay path and its usage key.
#[must_use]
pub fn valid_decision_engine_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && name.len() <= MAX_DECISION_ENGINE_NAME_BYTES
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

/// The decision engine a session consults, as the runtime sees it (revision 9): its name, its relay path on the gateway,
/// its deadline and its exposure; never its address or credential, which stay with the gateway.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "EngineFields")]
pub struct DecisionEngineSnapshot {
    /// The engine's name.
    #[schemars(length(min = 1, max = 64), regex(pattern = r"^[a-z][a-z0-9_-]{0,63}$"))]
    pub name: String,
    /// `/relay/d/<name>`; the runtime posts to `<relay_path>/v1/before_step` and `/v1/after_response`.
    #[schemars(length(min = 11, max = 73))]
    pub relay_path: String,
    /// How long the engine has to answer, 1 through 10000 ms.
    #[schemars(range(min = 1, max = 10_000))]
    pub timeout_ms: u32,
    /// How much of the conversation the engine may see.
    pub exposure: DecisionExposure,
    /// The points the engine answers; absent means both. The loop doesn't consult it at the others: no request and
    /// no event, exactly as without an engine there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<DecisionPoint>>,
}

impl DecisionEngineSnapshot {
    /// Whether the engine answers `point`.
    #[must_use]
    pub fn answers(&self, point: DecisionPoint) -> bool {
        self.points
            .as_ref()
            .is_none_or(|points| points.contains(&point))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EngineFields {
    #[schemars(length(min = 1, max = 64), regex(pattern = r"^[a-z][a-z0-9_-]{0,63}$"))]
    name: String,
    #[schemars(
        length(min = 11, max = 73),
        regex(pattern = r"^/relay/d/[a-z][a-z0-9_-]{0,63}$")
    )]
    relay_path: String,
    #[schemars(range(min = 1, max = 10_000))]
    timeout_ms: u32,
    exposure: DecisionExposure,
    /// One or both points, each once; absent means both.
    #[serde(default)]
    #[schemars(schema_with = "points_schema")]
    points: Option<Vec<DecisionPoint>>,
}

fn points_schema(generator: &mut SchemaGenerator) -> Schema {
    let item = generator.subschema_for::<DecisionPoint>();
    // maxItems: the number of decision points (two today); a new point raises it with the enum.
    json_schema!({"type":"array","minItems":1,"maxItems":2,"uniqueItems":true,"items":item})
}

impl TryFrom<EngineFields> for DecisionEngineSnapshot {
    type Error = &'static str;

    fn try_from(fields: EngineFields) -> Result<Self, Self::Error> {
        if !valid_decision_engine_name(&fields.name) {
            return Err("decision_engine: invalid name");
        }
        if fields.relay_path != Self::relay_path_for(&fields.name) {
            return Err("decision_engine: relay_path must be /relay/d/<name>");
        }
        if !(1..=MAX_DECISION_TIMEOUT_MS).contains(&fields.timeout_ms) {
            return Err("decision_engine: timeout_ms out of range");
        }
        if let Some(points) = &fields.points {
            // Any number of points works here; the schema's maxItems is the number of points there are.
            let distinct = !points.is_empty()
                && points
                    .iter()
                    .enumerate()
                    .all(|(index, point)| !points[..index].contains(point));
            if !distinct {
                return Err("decision_engine: points must name one or both points, each once");
            }
        }
        Ok(Self {
            name: fields.name,
            relay_path: fields.relay_path,
            timeout_ms: fields.timeout_ms,
            exposure: fields.exposure,
            points: fields.points,
        })
    }
}

/// Parses an optional engine, naming the field in every refusal.
pub(crate) fn engine_snapshot<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DecisionEngineSnapshot>, D::Error> {
    DecisionEngineSnapshot::deserialize(deserializer)
        .map(Some)
        .map_err(|error| D::Error::custom(format!("decision_engine: {error}")))
}

pub(crate) fn engine_snapshot_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<DecisionEngineSnapshot>()
}

impl DecisionEngineSnapshot {
    /// The relay path of the engine called `name`.
    #[must_use]
    pub fn relay_path_for(name: &str) -> String {
        format!("/relay/d/{name}")
    }
}

/// How much of the conversation a decision engine may see (ADR-0013 point 5): an administrator's setting per engine.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DecisionExposure {
    /// Calls by id and name only; nothing of the transcript. The default.
    #[default]
    None,
    /// Also the calls' arguments.
    Calls,
    /// Also the response's text and the transcript.
    Full,
}

/// Where the loop consulted the engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DecisionPoint {
    /// Before a step's model call.
    BeforeStep,
    /// After a complete model response, selecting its calls.
    AfterResponse,
}

/// A decision as the loop applied it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum DecisionChoice {
    /// The step called `route` with these tools.
    CallModel {
        /// The route called: the conversation's own or a declared fallback.
        #[serde(deserialize_with = "crate::validation::route_name")]
        #[schemars(length(min = 1, max = 256))]
        route: String,
        /// The tools offered, in registry order.
        #[serde(deserialize_with = "tools")]
        #[schemars(schema_with = "tools_schema")]
        tools: Vec<String>,
    },
    /// The loop ran a call the engine originated.
    Act {
        /// The tool run.
        #[serde(deserialize_with = "tool")]
        #[schemars(schema_with = "crate::tool_name::tool_name_schema")]
        tool: String,
        /// Its arguments, a JSON object of at most 64 KiB serialized.
        #[serde(deserialize_with = "act_arguments")]
        #[schemars(schema_with = "object_schema")]
        arguments: Value,
    },
    /// The loop compacted the conversation now.
    Compact,
    /// The run stopped as failed with this reason.
    Stop {
        /// 1 through 200 characters, no control characters.
        #[serde(deserialize_with = "stop_reason")]
        #[schemars(schema_with = "stop_reason_schema")]
        reason: String,
    },
    /// The response's calls kept, by id; the others got an error result.
    Select {
        /// At most 64 call ids.
        #[serde(deserialize_with = "call_ids")]
        #[schemars(length(max = 64))]
        call_ids: Vec<ToolCallId>,
    },
}

/// Why the loop set an engine's answer aside and used the model's own decisions for the step.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DecisionFallbackReason {
    /// The engine could not be reached.
    Unreachable,
    /// The engine did not answer in time.
    Timeout,
    /// The answer wasn't a decision.
    Invalid,
    /// The decision named a route other than the conversation's own or a declared fallback.
    RouteNotAllowed,
    /// The decision named a tool outside the step's set.
    ToolNotAllowed,
    /// An `act` call's arguments failed the tool's schema.
    Schema,
    /// A second `compact` in one step.
    RepeatedCompact,
    /// A `compact` with nothing old enough to fold.
    CompactNothing,
    /// A fourth `act` in a row.
    ActLoop,
    /// Three fallbacks in a row: the engine is no longer asked in this run.
    CircuitOpen,
    /// The engine's budget for its period is spent (the gateway refused before sending): the engine is no longer
    /// asked in this run; a new run asks again (ADR-0013, the adapter amendment).
    BudgetSpent,
}

pub(crate) fn tool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if crate::valid_tool_name(&name) {
        Ok(name)
    } else {
        Err(D::Error::custom("decision: invalid tool name"))
    }
}

pub(crate) fn tools<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let names = Vec::<String>::deserialize(deserializer)?;
    if names.len() > MAX_DECISION_TOOLS {
        return Err(D::Error::custom("decision: at most 256 tools"));
    }
    if !names.iter().all(|name| crate::valid_tool_name(name)) {
        return Err(D::Error::custom("decision: invalid tool name"));
    }
    Ok(names)
}

pub(crate) fn tools_schema(generator: &mut SchemaGenerator) -> Schema {
    let item = crate::tool_name::tool_name_schema(generator);
    json_schema!({"type":"array","maxItems":MAX_DECISION_TOOLS,"items":item})
}

fn act_arguments<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
    let arguments = Value::deserialize(deserializer)?;
    if !arguments.is_object() {
        return Err(D::Error::custom(
            "decision: act arguments must be an object",
        ));
    }
    let size = serde_json::to_vec(&arguments).map_or(usize::MAX, |bytes| bytes.len());
    if size > MAX_DECISION_ACT_BYTES {
        return Err(D::Error::custom("decision: act arguments over 64 KiB"));
    }
    Ok(arguments)
}

pub(crate) fn object_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"object"})
}

pub(crate) fn stop_reason<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let reason = String::deserialize(deserializer)?;
    let chars = reason.chars().count();
    if chars == 0 || chars > MAX_DECISION_STOP_REASON_CHARS || reason.chars().any(char::is_control)
    {
        return Err(D::Error::custom(
            "decision: a stop reason is 1 through 200 characters without control characters",
        ));
    }
    Ok(reason)
}

pub(crate) fn stop_reason_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string","minLength":1,"maxLength":MAX_DECISION_STOP_REASON_CHARS})
}

pub(crate) fn call_ids<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ToolCallId>, D::Error> {
    let ids = Vec::<ToolCallId>::deserialize(deserializer)?;
    if ids.len() > MAX_DECISION_CALLS {
        return Err(D::Error::custom("decision: at most 64 call ids"));
    }
    Ok(ids)
}

/// An engine name: a route name (an engine is a route kind, ADR-0013 point 6).
pub(crate) fn engine<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    crate::validation::route_name(deserializer)
}
