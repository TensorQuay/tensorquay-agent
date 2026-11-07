//! The Responses provider as its public API documents it: its request rules and its error object. It accepts
//! `system` and `developer` input messages; the hosted backend that refuses them is `openai_responses_backend`.

use super::{Rule, Source, Violation, items, strict, valid_tool_name};
use serde_json::{Value, json};

pub(super) const PATH: &str = "/v1/responses";

/// `{"error":{"message","type","param","code"}}`, the documented error object.
pub(super) fn error_body(_status: u16, violation: &Violation) -> Value {
    json!({"error":{
        "message": violation.message,
        "type": "invalid_request_error",
        "param": violation.param,
        "code": violation.code,
    }})
}

pub(super) const RULES: &[Rule] = &[
    Rule {
        name: "tool_name_pattern",
        source: Source::Documented("function tools: name matches ^[a-zA-Z0-9_-]{1,64}$"),
        status: 400,
        check: |body| {
            let (index, name) = items(body, "tools").enumerate().find_map(|(index, tool)| {
                let name = tool["name"].as_str().unwrap_or_default();
                (!valid_tool_name(name)).then(|| (index, name.to_owned()))
            })?;
            Some(Violation {
                message: format!(
                    "Invalid 'tools[{index}].name': string does not match pattern. Expected a string that matches the pattern '^[a-zA-Z0-9_-]+$'. Got '{name}'."
                ),
                param: Some(format!("tools[{index}].name")),
                code: Some("invalid_value"),
            })
        },
    },
    Rule {
        name: "strict_object_root",
        source: Source::Documented("structured outputs: the root schema is an object, not anyOf"),
        status: 400,
        check: |body| strict::first(body, strict::object_root),
    },
    Rule {
        name: "strict_supported_keywords",
        source: Source::Documented(
            "structured outputs: composition keywords such as allOf, not and if/then/else are not supported",
        ),
        status: 400,
        check: |body| strict::first(body, strict::supported_keywords),
    },
    Rule {
        name: "strict_closed_objects",
        source: Source::Documented(
            "structured outputs: additionalProperties must be false on every object",
        ),
        status: 400,
        check: |body| strict::first(body, strict::closed_objects),
    },
    Rule {
        name: "strict_all_required",
        source: Source::Documented(
            "structured outputs: every property is listed in required (optional ones are nullable)",
        ),
        status: 400,
        check: |body| strict::first(body, strict::all_required),
    },
];
