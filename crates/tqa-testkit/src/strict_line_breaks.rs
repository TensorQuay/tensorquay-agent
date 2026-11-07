//! A fake endpoint whose constrained decoding loses line breaks in strict tools' string arguments (TK-U-019), as a
//! local server was measured to (docs/evals/gate-5.md, "Cause found"). Only the scripted calls of a tool the request
//! sent with `strict: true` change: `function.strict` in the Chat shape, top-level `strict` in the Responses shape.
//! The Messages shape has no strict field, so its calls never change. Arguments that are not a JSON object are kept.
use crate::{Script, Scripted};
use serde_json::Value;
use std::collections::BTreeSet;

/// How a strict tool's string arguments lose their line breaks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrictLineBreaks {
    /// Every line break (`\n`, `\r`) is removed from each string argument, at any depth, so the lines run together:
    /// the shape most strict calls took on the measured server (3 of 5 raw answers; the others split an edit inside
    /// one call or kept only its first line).
    Strip,
}

/// The names of the tools `body` sends with `strict: true`.
pub(crate) fn strict_tools(body: &Value) -> BTreeSet<String> {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let definition = if tool["function"].is_object() {
                &tool["function"]
            } else {
                tool
            };
            (definition["strict"] == true)
                .then(|| definition["name"].as_str().map(str::to_owned))
                .flatten()
        })
        .collect()
}

/// `script` with each call of a tool in `strict` changed by `mode`.
pub(crate) fn apply(mode: StrictLineBreaks, strict: &BTreeSet<String>, script: &Script) -> Script {
    let mut events = Vec::with_capacity(script.events.len());
    for event in &script.events {
        match event {
            Scripted::ToolCall {
                name,
                arguments_json,
                split_into,
            } if strict.contains(name) => {
                let Ok(Value::Object(arguments)) = serde_json::from_str::<Value>(arguments_json)
                else {
                    events.push(event.clone());
                    continue;
                };
                let StrictLineBreaks::Strip = mode;
                events.push(Scripted::ToolCall {
                    name: name.clone(),
                    arguments_json: strip(Value::Object(arguments)).to_string(),
                    split_into: *split_into,
                });
            }
            _ => events.push(event.clone()),
        }
    }
    Script {
        header_delay: script.header_delay,
        events,
    }
}

fn strip(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace(['\n', '\r'], "")),
        Value::Array(items) => Value::Array(items.into_iter().map(strip).collect()),
        Value::Object(members) => Value::Object(
            members
                .into_iter()
                .map(|(key, value)| (key, strip(value)))
                .collect(),
        ),
        other => other,
    }
}
