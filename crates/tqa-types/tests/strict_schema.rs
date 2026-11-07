//! LLM-U-034: `strict_compatible` accepts exactly the schemas a strict endpoint documents as supported (the Chat
//! Completions and Responses protocols' documented function-calling "Strict mode" and structured-output "Supported
//! schemas", read 2026-09-27): a root object, every object closed
//! with `additionalProperties: false` and every property required, only supported keywords, and within the size bounds.
//! Anything it can't vouch for is incompatible, which only costs strict decoding, never a refused request.
use serde_json::{Value, json};
use tqa_types::strict_compatible;

fn object(properties: &Value) -> Value {
    let required: Vec<String> = properties
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

#[test]
fn llm_u_034_closed_objects_with_every_property_required_are_compatible() {
    for schema in [
        json!({"type": "object", "properties": {}, "additionalProperties": false}),
        object(&json!({"path": {"type": "string"}, "create": {"type": "boolean"}})),
        // Optional expressed as nullable, and every documented keyword.
        object(&json!({
            "name": {"type": "string", "pattern": "^[a-z]+$", "minLength": 1, "maxLength": 64, "description": "d"},
            "when": {"type": ["string", "null"], "format": "date-time"},
            "count": {"type": "integer", "minimum": 0, "maximum": 10, "exclusiveMinimum": -1, "exclusiveMaximum": 11, "multipleOf": 1},
            "unit": {"type": ["string", "null"], "enum": ["F", "C", null]},
            "kind": {"const": "x"},
            "items": {"type": "array", "minItems": 1, "maxItems": 50, "items": object(&json!({"a": {"type": "string"}}))},
            "either": {"anyOf": [{"type": "string"}, object(&json!({"b": {"type": "number"}}))]},
            "step": {"$ref": "#/$defs/step"},
        })),
    ] {
        let mut schema = schema;
        if schema["properties"].get("step").is_some() {
            schema["$defs"] = json!({"step": object(&json!({"text": {"type": "string"}}))});
        }
        assert!(strict_compatible(&schema), "{schema}");
    }
}

fn base() -> Value {
    object(&json!({"path": {"type": "string"}}))
}

/// The base schema with one change.
fn with(change: &dyn Fn(&mut Value)) -> Value {
    let mut schema = base();
    change(&mut schema);
    schema
}

#[test]
fn llm_u_034_each_documented_violation_is_incompatible() {
    for (why, schema) in [
        ("an optional property", with(&|s| s["required"] = json!([]))),
        (
            "required naming no property",
            with(&|s| s["required"] = json!(["path", "other"])),
        ),
        (
            "additionalProperties missing",
            with(&|s| {
                s.as_object_mut().map(|m| m.remove("additionalProperties"));
            }),
        ),
        (
            "additionalProperties true",
            with(&|s| s["additionalProperties"] = json!(true)),
        ),
        (
            "a nested object left open",
            with(&|s| s["properties"]["path"] = json!({"type": "object", "properties": {}})),
        ),
        (
            "an optional nested property",
            with(&|s| {
                s["properties"]["path"] = json!({"type": "object", "properties": {"a": {"type": "string"}}, "additionalProperties": false});
            }),
        ),
        (
            "the root is not an object",
            json!({"type": "array", "items": {"type": "string"}}),
        ),
        ("the root is anyOf", json!({"anyOf": [base(), base()]})),
        (
            "not",
            with(&|s| s["properties"]["path"]["not"] = json!({"pattern": "^0+$"})),
        ),
        (
            "root not",
            with(&|s| s["not"] = json!({"required": ["path"]})),
        ),
        (
            "allOf",
            with(&|s| s["properties"]["path"]["allOf"] = json!([{"type": "string"}])),
        ),
        (
            "if/then/else",
            with(&|s| s["if"] = json!({"required": ["path"]})),
        ),
        (
            "dependentRequired",
            with(&|s| s["dependentRequired"] = json!({"path": []})),
        ),
        (
            "dependentSchemas",
            with(&|s| s["dependentSchemas"] = json!({})),
        ),
    ] {
        assert!(!strict_compatible(&schema), "{why}: {schema}");
    }
}

#[test]
fn llm_u_034_undocumented_keywords_and_broken_nesting_are_incompatible() {
    for (why, schema) in [
        // Keywords the documentation does not list: not vouched for.
        (
            "default",
            with(&|s| s["properties"]["path"]["default"] = json!(".")),
        ),
        (
            "$schema",
            with(&|s| s["$schema"] = json!("http://json-schema.org/draft-07/schema#")),
        ),
        ("title", with(&|s| s["title"] = json!("T"))),
        (
            "patternProperties",
            with(&|s| s["patternProperties"] = json!({})),
        ),
        (
            "an anyOf branch that breaks the rules",
            with(&|s| {
                s["properties"]["path"] =
                    json!({"anyOf": [{"type": "string"}, {"type": "object", "properties": {}}]});
            }),
        ),
        (
            "array items that break the rules",
            with(&|s| {
                s["properties"]["path"] =
                    json!({"type": "array", "items": {"type": "object", "properties": {}}});
            }),
        ),
        (
            "a definition that breaks the rules",
            with(&|s| s["$defs"] = json!({"d": {"type": "object"}})),
        ),
        ("a schema that is not an object", json!(true)),
        (
            "a property schema that is not an object",
            with(&|s| s["properties"]["path"] = json!(true)),
        ),
    ] {
        assert!(!strict_compatible(&schema), "{why}: {schema}");
    }
}

/// An object nested `depth` levels below the root.
fn nested(depth: usize) -> Value {
    (0..depth).fold(object(&json!({"leaf": {"type": "string"}})), |inner, _| {
        object(&json!({"inner": inner}))
    })
}

#[test]
fn llm_u_034_the_documented_bounds_are_exact() {
    // Up to 10 levels of nesting.
    assert!(strict_compatible(&nested(9)));
    assert!(!strict_compatible(&nested(10)));
    // Up to 5000 object properties in all.
    let properties = |count: usize| {
        object(&Value::Object(
            (0..count)
                .map(|index| (format!("p{index}"), json!({"type": "boolean"})))
                .collect(),
        ))
    };
    assert!(strict_compatible(&properties(5000)));
    assert!(!strict_compatible(&properties(5001)));
    // Up to 1000 enum values in all.
    let enums = |count: usize| {
        object(&json!({"e": {"type": "integer", "enum": (0..count).collect::<Vec<usize>>()}}))
    };
    assert!(strict_compatible(&enums(1000)));
    assert!(!strict_compatible(&enums(1001)));
    // Property names, definition names, enum values and const values: at most 120,000 characters in all.
    let long =
        |length: usize| object(&json!({"s": {"type": "string", "enum": ["x".repeat(length - 1)]}}));
    assert!(strict_compatible(&long(120_000)));
    assert!(!strict_compatible(&long(120_001)));
}
