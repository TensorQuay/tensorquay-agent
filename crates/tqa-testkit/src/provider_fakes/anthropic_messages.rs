//! The hosted Messages provider: its request rules and its error object.

use super::{Rule, Source, Violation, items, valid_tool_name};
use serde_json::{Value, json};

pub(super) const PATH: &str = "/v1/messages";

/// `{"type":"error","error":{"type","message"}}`, the documented error shape; the type follows the status.
pub(super) fn error_body(status: u16, violation: &Violation) -> Value {
    let kind = match status {
        404 => "not_found_error",
        _ => "invalid_request_error",
    };
    json!({"type":"error","error":{"type":kind,"message":violation.message}})
}

pub(super) const RULES: &[Rule] = &[
    Rule {
        name: "max_tokens_required",
        source: Source::Documented("Messages API reference: max_tokens is required"),
        status: 400,
        check: |body| {
            body.get("max_tokens")
                .is_none()
                .then(|| Violation::new("max_tokens: Field required"))
        },
    },
    Rule {
        name: "no_system_role",
        source: Source::Documented(
            "Messages API reference: roles are user and assistant; the system prompt is the top-level system parameter",
        ),
        status: 400,
        check: |body| {
            items(body, "messages")
                .any(|message| message["role"] == "system")
                .then(|| {
                    Violation::new(
                        "messages: Unexpected role \"system\". The Messages API accepts a top-level `system` parameter, not \"system\" as an input message role.",
                    )
                })
        },
    },
    Rule {
        name: "system_text_non_empty",
        source: Source::Observed(
            "observed on 27 Sep 2026 from the hosted Messages API: HTTP 400 for a system text block \"\"",
        ),
        status: 400,
        check: |body| {
            let empty = match &body["system"] {
                Value::String(text) => text.is_empty(),
                Value::Array(blocks) => blocks.iter().any(|block| {
                    block["type"] == "text" && block["text"].as_str().is_some_and(str::is_empty)
                }),
                _ => false,
            };
            empty.then(|| Violation::new("system: text content blocks must be non-empty"))
        },
    },
    Rule {
        name: "first_turn_user",
        source: Source::Documented("Messages API reference: the first message uses the user role"),
        status: 400,
        check: |body| {
            let first = items(body, "messages").next()?;
            (first["role"] != "user")
                .then(|| Violation::new("messages: first message must use the \"user\" role"))
        },
    },
    Rule {
        name: "tool_name_pattern",
        source: Source::Documented("tool definitions: name matches ^[a-zA-Z0-9_-]{1,64}$"),
        status: 400,
        check: |body| {
            let index = items(body, "tools")
                .position(|tool| !valid_tool_name(tool["name"].as_str().unwrap_or_default()))?;
            Some(Violation::new(format!(
                "tools.{index}.custom.name: String should match pattern '^[a-zA-Z0-9_-]{{1,64}}$'"
            )))
        },
    },
    Rule {
        name: "input_schema_object",
        source: Source::Documented(
            "tool definitions: input_schema is a JSON Schema object with type object",
        ),
        status: 400,
        check: |body| {
            let index =
                items(body, "tools").position(|tool| tool["input_schema"]["type"] != "object")?;
            Some(Violation::new(format!(
                "tools.{index}.custom.input_schema.type: Input should be 'object'"
            )))
        },
    },
    Rule {
        name: "thinking_temperature",
        source: Source::Documented(
            "extended thinking: temperature can't be changed while thinking is enabled",
        ),
        status: 400,
        check: |body| {
            let thinking = body["thinking"]["type"] == "enabled";
            let changed = body
                .get("temperature")
                .and_then(Value::as_f64)
                .is_some_and(|temperature| (temperature - 1.0).abs() > f64::EPSILON);
            (thinking && changed).then(|| {
                Violation::new("`temperature` may only be set to 1 when thinking is enabled.")
            })
        },
    },
];
