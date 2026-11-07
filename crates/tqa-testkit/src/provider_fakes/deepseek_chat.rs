//! A hosted Chat provider with a strict deserializer: its request rules and its error object.

use super::{Rule, Source, Violation, items, strict, valid_tool_name};
use serde_json::{Value, json};

pub(super) const PATH: &str = "/v1/chat/completions";

const ROLES: [&str; 4] = ["system", "user", "assistant", "tool"];

/// `{"error":{"message","type","param","code"}}`, the documented error object.
pub(super) fn error_body(_status: u16, violation: &Violation) -> Value {
    json!({"error":{
        "message": violation.message,
        "type": "invalid_request_error",
        "param": violation.param,
        "code": violation.code.unwrap_or("invalid_request_error"),
    }})
}

pub(super) const RULES: &[Rule] = &[
    Rule {
        name: "known_roles",
        source: Source::Documented(
            "chat completion API: message roles are system, user, assistant and tool (the wording of the refusal is to confirm in the live suite)",
        ),
        status: 422,
        check: |body| {
            let (index, role) =
                items(body, "messages")
                    .enumerate()
                    .find_map(|(index, message)| {
                        let role = message["role"].as_str().unwrap_or_default();
                        (!ROLES.contains(&role)).then(|| (index, role.to_owned()))
                    })?;
            Some(Violation::new(format!(
                "Failed to deserialize the JSON body into the target type: messages[{index}]: unknown variant `{role}`, expected one of `system`, `user`, `assistant`, `tool`"
            )))
        },
    },
    Rule {
        name: "tool_limits",
        source: Source::Documented(
            "function calling: at most 128 functions, each name matching ^[a-zA-Z0-9_-]{1,64}$",
        ),
        status: 400,
        check: |body| {
            let tools: Vec<&Value> = items(body, "tools").collect();
            if tools.len() > 128 {
                return Some(Violation::new("Too many tools: at most 128 are supported"));
            }
            let index = tools.iter().position(|tool| {
                !valid_tool_name(tool["function"]["name"].as_str().unwrap_or_default())
            })?;
            Some(Violation::new(format!(
                "Invalid 'tools[{index}].function.name': must match ^[a-zA-Z0-9_-]{{1,64}}$"
            )))
        },
    },
    Rule {
        name: "strict_closed_objects",
        source: Source::Documented(
            "function calling, strict mode: every object sets additionalProperties to false",
        ),
        status: 400,
        check: |body| chat_strict(body, strict::closed_objects),
    },
    Rule {
        name: "strict_all_required",
        source: Source::Documented("function calling, strict mode: every property is required"),
        status: 400,
        check: |body| chat_strict(body, strict::all_required),
    },
];

/// The strict rules on Chat's nested `function` objects.
fn chat_strict(body: &Value, rule: fn(&Value, &str) -> Option<String>) -> Option<Violation> {
    items(body, "tools")
        .enumerate()
        .filter(|(_, tool)| tool["function"]["strict"] == true)
        .find_map(|(index, tool)| {
            let name = tool["function"]["name"].as_str().unwrap_or_default();
            let reason = rule(&tool["function"]["parameters"], "")?;
            Some(Violation {
                message: format!("Invalid schema for function '{name}': {reason}"),
                param: Some(format!("tools[{index}].function.parameters")),
                code: Some("invalid_request_error"),
            })
        })
}
