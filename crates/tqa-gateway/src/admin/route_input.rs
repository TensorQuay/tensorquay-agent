//! The routes API's request inputs (ADR-0028): the path name, the body's route record, its display name and version.
//! Each is validated here once; the route record itself is validated by tqa-llm's validator in `settings::routes`.
use crate::{GatewayError, InvalidReason};
use serde_json::Value;

/// The longest route name: the `model_routes.name` check (migration 0034).
const NAME_BYTES: usize = 128;
/// The longest display name, in characters: the `model_routes.display_name` check (migration 0034).
const DISPLAY_NAME_CHARS: usize = 80;

fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}

/// Whether `name` is a valid route name: 1 through 128 bytes, no control characters.
pub(crate) fn name_valid(name: &str) -> bool {
    (1..=NAME_BYTES).contains(&name.len()) && !name.chars().any(char::is_control)
}

pub(super) fn name(name: &str) -> Result<(), GatewayError> {
    if name_valid(name) {
        Ok(())
    } else {
        Err(invalid("name", InvalidReason::RouteNameInvalid))
    }
}

pub(super) fn display_name(value: Option<Value>) -> Result<Option<String>, GatewayError> {
    let refused = || invalid("display_name", InvalidReason::DisplayNameInvalid);
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if display_name_clean(&text) => Ok(Some(text)),
        Some(_) => Err(refused()),
    }
}

/// A display name's rule (GW-I-060), for the API and for a route the file names (GW-I-216): 1 to 80 characters, none
/// of them a control character, and no surrounding spaces.
pub(crate) fn display_name_clean(text: &str) -> bool {
    (1..=DISPLAY_NAME_CHARS).contains(&text.chars().count())
        && !text.chars().any(char::is_control)
        && text.trim() == text
}

pub(super) fn version(value: Option<Value>) -> Result<Option<i64>, GatewayError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .filter(|version| *version >= 1)
            .map(Some)
            .ok_or_else(|| invalid("version", InvalidReason::VersionInvalid)),
    }
}

/// A routes API request body, validated: the route record, its display name and description, and the version read.
#[derive(Debug)]
pub(super) struct RouteBody {
    pub config: tqa_llm::RouteConfig,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub expected: Option<i64>,
}

/// The request body: every route field except `name`, plus `display_name?`, `description?` and `version?`.
pub(super) fn body(route: &str, bytes: &[u8]) -> Result<RouteBody, GatewayError> {
    let body_invalid = || invalid("body", InvalidReason::BodyInvalid);
    let Ok(Value::Object(mut fields)) = serde_json::from_slice::<Value>(bytes) else {
        return Err(body_invalid());
    };
    if fields.contains_key("name") {
        return Err(body_invalid());
    }
    let display_name = display_name(fields.remove("display_name"))?;
    let description = crate::description::from_json(fields.remove("description"))?;
    let expected = version(fields.remove("version"))?;
    fields.insert("name".into(), Value::String(route.to_owned()));
    let config = serde_json::from_value(Value::Object(fields)).map_err(|_| body_invalid())?;
    Ok(RouteBody {
        config,
        display_name,
        description,
        expected,
    })
}

#[cfg(test)]
#[path = "route_input_tests.rs"]
mod tests;
