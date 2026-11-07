//! A hosted provider reached through its compatible Chat endpoint: its request rules (an `OpenAPI` schema subset) and its
//! list-wrapped error object.

use super::{Rule, Source, Violation, items, strict};
use serde_json::{Value, json};

pub(super) const PATH: &str = "/v1/chat/completions";

/// Where the endpoint puts a tool call's opaque signature on the call object, and requires it back: observed live on
/// the provider probe, 2026-09-27 (a single string leaf).
pub const THOUGHT_SIGNATURE_PATH: [&str; 3] = ["extra_content", "google", "thought_signature"];

/// The opaque signature the fake gives a tool call: derived from the call's id, so the rule that requires it back
/// needs no memory of earlier answers. Every call is signed (whether a real endpoint signs each of several parallel
/// calls is not confirmed).
#[must_use]
pub fn thought_signature(call_id: &str) -> String {
    let mut signature = String::from("tsig-");
    for byte in call_id.bytes() {
        signature.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        signature.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    signature
}

/// The signature at [`THOUGHT_SIGNATURE_PATH`] on a call object, when there is one.
pub(super) fn signature_of(call: &Value) -> Option<&str> {
    THOUGHT_SIGNATURE_PATH
        .iter()
        .try_fold(call, |node, key| node.get(*key))
        .and_then(Value::as_str)
}

/// Puts the call's signature on a streamed tool-call piece that carries its id.
pub(super) fn sign(call: &mut serde_json::Map<String, Value>) {
    let Some(id) = call.get("id").and_then(Value::as_str) else {
        return;
    };
    let mut extra = json!(thought_signature(id));
    for key in THOUGHT_SIGNATURE_PATH.iter().skip(1).rev() {
        extra = json!({ *key: extra });
    }
    call.insert(THOUGHT_SIGNATURE_PATH[0].into(), extra);
}

const ROLES: [&str; 4] = ["system", "user", "assistant", "tool"];

/// Schema keywords outside the function declarations' `OpenAPI` subset.
const UNSUPPORTED: [&str; 9] = [
    "additionalProperties",
    "$ref",
    "$defs",
    "definitions",
    "$schema",
    "const",
    "patternProperties",
    "if",
    "not",
];

/// `{"error":{"code","message","status"}}`, wrapped in a one-element list as the compatible endpoint returns
/// it (to confirm in the live suite).
pub(super) fn error_body(status: u16, violation: &Violation) -> Value {
    let kind = match status {
        404 => "NOT_FOUND",
        _ => "INVALID_ARGUMENT",
    };
    json!([{"error":{"code":status,"message":violation.message,"status":kind}}])
}

pub(super) const RULES: &[Rule] = &[
    Rule {
        name: "thought_signature",
        source: Source::Observed(
            "the live provider probe, 2026-09-27 17:02: a round trip without the call's signature, refused 400 in these words (the trailing documentation link, redacted in our excerpt, is left out)",
        ),
        status: 400,
        check: |body| {
            items(body, "messages").enumerate().find_map(|(position, message)| {
                if message["role"] != "assistant" {
                    return None;
                }
                let call = message["tool_calls"].as_array()?.iter().find(|call| {
                    let id = call["id"].as_str().unwrap_or_default();
                    signature_of(call) != Some(thought_signature(id).as_str())
                })?;
                let name = call["function"]["name"].as_str().unwrap_or_default();
                Some(Violation::new(format!(
                    "Function call is missing a thought_signature in functionCall parts. This is required for tools to work correctly, and missing thought_signature may lead to degraded model performance. Additional data, function call `default_api:{name}` , position {position}."
                )))
            })
        },
    },
    Rule {
        name: "known_roles",
        source: Source::Documented(
            "OpenAI compatibility: messages use the system, user, assistant and tool roles (the wording of the refusal is to confirm in the live suite)",
        ),
        status: 400,
        check: |body| {
            let index = items(body, "messages").position(|message| {
                !ROLES.contains(&message["role"].as_str().unwrap_or_default())
            })?;
            Some(Violation::new(format!(
                "Invalid value at 'messages[{index}].role': unsupported role"
            )))
        },
    },
    Rule {
        name: "schema_keywords",
        source: Source::Documented(
            "function calling: parameters use a subset of the OpenAPI 3.0 schema; other keywords are refused",
        ),
        status: 400,
        check: |body| {
            items(body, "tools").enumerate().find_map(|(index, tool)| {
                strict::walk(&tool["function"]["parameters"], "", &|node, _| {
                    UNSUPPORTED
                        .iter()
                        .find(|keyword| node.get(**keyword).is_some())
                        .map(|keyword| (*keyword).to_owned())
                })
                .map(|keyword| {
                    Violation::new(format!(
                        "Invalid JSON payload received. Unknown name \"{keyword}\" at 'tools[{index}].function_declarations[0].parameters': Cannot find field."
                    ))
                })
            })
        },
    },
];
