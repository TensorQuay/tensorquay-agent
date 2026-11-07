//! The documented strict-schema rules of function tools, checked on every tool sent with `strict: true`, worded as the
//! provider words its refusals.

use super::Violation;
use serde_json::Value;

/// Composition and conditional keywords a strict schema may not use.
const UNSUPPORTED: [&str; 9] = [
    "allOf",
    "not",
    "if",
    "then",
    "else",
    "dependentRequired",
    "dependentSchemas",
    "patternProperties",
    "unevaluatedProperties",
];

/// The first strict tool whose schema breaks `rule`, as the provider's refusal.
pub(super) fn first(body: &Value, rule: fn(&Value, &str) -> Option<String>) -> Option<Violation> {
    super::items(body, "tools")
        .enumerate()
        .filter(|(_, tool)| tool["strict"] == true)
        .find_map(|(index, tool)| {
            let name = tool["name"].as_str().unwrap_or_default();
            let reason = rule(&tool["parameters"], "")?;
            Some(Violation {
                message: format!("Invalid schema for function '{name}': {reason}"),
                param: Some(format!("tools[{index}].parameters")),
                code: Some("invalid_function_parameters"),
            })
        })
}

pub(super) fn object_root(schema: &Value, _: &str) -> Option<String> {
    (schema["type"] != "object" || schema.get("anyOf").is_some()).then(|| {
        format!(
            "schema must be a JSON Schema of 'type: \"object\"', got 'type: \"{}\"'.",
            schema["type"].as_str().unwrap_or("None")
        )
    })
}

pub(super) fn supported_keywords(schema: &Value, context: &str) -> Option<String> {
    walk(schema, context, &|node, context| {
        let keyword = UNSUPPORTED
            .iter()
            .find(|keyword| node.get(**keyword).is_some())?;
        Some(format!(
            "In context=({context}), '{keyword}' is not permitted."
        ))
    })
}

pub(super) fn closed_objects(schema: &Value, context: &str) -> Option<String> {
    walk(schema, context, &|node, context| {
        (is_object(node) && node.get("additionalProperties") != Some(&Value::Bool(false))).then(|| {
            format!("In context=({context}), 'additionalProperties' is required to be supplied and to be false.")
        })
    })
}

pub(super) fn all_required(schema: &Value, context: &str) -> Option<String> {
    walk(schema, context, &|node, context| {
        let properties = node.get("properties")?.as_object()?;
        let required: Vec<&str> = node["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let missing = properties
            .keys()
            .find(|key| !required.contains(&key.as_str()))?;
        Some(format!(
            "In context=({context}), 'required' is required to be supplied and to be an array including every key in properties. Missing '{missing}'."
        ))
    })
}

fn is_object(node: &Value) -> bool {
    node["type"] == "object"
        || node["type"]
            .as_array()
            .is_some_and(|types| types.iter().any(|kind| kind == "object"))
        || node.get("properties").is_some()
}

/// Applies `check` to the schema and every schema nested in it, depth first, naming each by its path.
pub(super) fn walk(
    node: &Value,
    context: &str,
    check: &dyn Fn(&Value, &str) -> Option<String>,
) -> Option<String> {
    if !node.is_object() {
        return None;
    }
    if let Some(reason) = check(node, context) {
        return Some(reason);
    }
    let children = node["properties"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, child)| (format!("'properties', '{key}'"), child))
        .chain(["$defs", "definitions"].into_iter().flat_map(|field| {
            node[field]
                .as_object()
                .into_iter()
                .flatten()
                .map(move |(key, child)| (format!("'{field}', '{key}'"), child))
        }))
        .chain(node.get("items").map(|child| ("'items'".to_owned(), child)))
        .chain(
            node["anyOf"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(index, child)| (format!("'anyOf', '{index}'"), child)),
        );
    for (step, child) in children {
        let path = if context.is_empty() {
            step
        } else {
            format!("{context}, {step}")
        };
        if let Some(reason) = walk(child, &path, check) {
            return Some(reason);
        }
    }
    None
}
