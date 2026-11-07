//! LLM-U-041: the per-route tool-schema settings. `strict` auto, on and off; `schema_keywords` full and basic, the
//! basic allow-list applied at every depth; both tool shapes (Chat and Responses); never widened.
use serde_json::{Value, json};

use super::tool_schemas::{SchemaKeywords, StrictTools, reduce_schema, shape_tools};

/// A strict-valid schema.
fn valid() -> Value {
    json!({"type": "object", "additionalProperties": false, "required": ["path", "limit"],
        "properties": {"path": {"type": "string"}, "limit": {"type": ["integer", "null"], "minimum": 1}}})
}

/// A schema a strict endpoint refuses (an optional property, an open object).
fn invalid() -> Value {
    json!({"type": "object", "properties": {"q": {"type": "string"}}, "required": []})
}

fn chat(parameters: &Value) -> Value {
    json!({"tools": [{"type": "function", "function": {"name": "t", "description": "d", "parameters": parameters, "strict": true}}]})
}

fn responses(parameters: &Value) -> Value {
    json!({"tools": [{"type": "function", "name": "t", "description": "d", "parameters": parameters, "strict": true}]})
}

fn strict_of(body: &Value) -> Option<bool> {
    let tool = &body["tools"][0];
    tool.get("function").unwrap_or(tool)["strict"].as_bool()
}

fn parameters_of(body: &Value) -> Value {
    let tool = &body["tools"][0];
    tool.get("function").unwrap_or(tool)["parameters"].clone()
}

#[test]
fn llm_u_041_strict_follows_the_mode_in_both_tool_shapes() {
    for shape in [chat as fn(&Value) -> Value, responses] {
        for (mode, schema, expected) in [
            (StrictTools::Auto, valid(), true),
            (StrictTools::Auto, invalid(), false),
            (StrictTools::On, valid(), true),
            (StrictTools::On, invalid(), true),
            (StrictTools::Off, valid(), false),
            (StrictTools::Off, invalid(), false),
        ] {
            let mut body = shape(&schema);
            shape_tools(mode, SchemaKeywords::Full, &mut body);
            assert_eq!(strict_of(&body), Some(expected), "{mode:?} {schema}");
            assert_eq!(
                parameters_of(&body),
                schema,
                "full leaves the schema as it is"
            );
        }
    }
}

#[test]
fn llm_u_041_basic_keeps_exactly_the_allow_list_at_every_depth() {
    let schema = json!({
        "type": "object", "description": "root", "additionalProperties": false, "title": "T",
        "$schema": "http://json-schema.org/draft-07/schema#", "required": ["name", "list", "either", "count"],
        "properties": {
            "name": {"type": ["string", "null"], "pattern": "^[a-z]+$", "minLength": 1, "maxLength": 9,
                "format": "date", "enum": ["a", null], "const": "a", "default": "a"},
            "count": {"type": "integer", "minimum": 1, "maximum": 9, "exclusiveMinimum": 0, "multipleOf": 1},
            "list": {"type": "array", "minItems": 1, "maxItems": 3,
                "items": {"type": "object", "additionalProperties": false, "required": ["x"],
                    "properties": {"x": {"type": "string", "pattern": "^x$"}}}},
            "either": {"anyOf": [{"type": "string", "maxLength": 3}, {"$ref": "#/$defs/d"}]}
        },
        "$defs": {"d": {"type": "string"}}
    });
    let reduced = reduce_schema(&schema);
    assert_eq!(
        reduced,
        json!({
            "type": "object", "description": "root",
            "required": ["name", "list", "either", "count"],
            "properties": {
                "name": {"type": ["string", "null"], "format": "date", "enum": ["a", null]},
                "count": {"type": "integer", "minimum": 1, "maximum": 9},
                "list": {"type": "array", "minItems": 1, "maxItems": 3,
                    "items": {"type": "object", "required": ["x"],
                        "properties": {"x": {"type": "string"}}}},
                "either": {"anyOf": [{"type": "string"}, {}]}
            }
        })
    );
    // Applied to a request's tools, in both shapes; auto then judges the reduced schema.
    for shape in [chat as fn(&Value) -> Value, responses] {
        let mut body = shape(&schema);
        shape_tools(StrictTools::Auto, SchemaKeywords::Basic, &mut body);
        assert_eq!(parameters_of(&body), reduced);
    }
}

#[test]
fn llm_u_041_a_body_without_tools_or_with_odd_tools_is_left_alone() {
    for body in [
        json!({"model": "m"}),
        json!({"tools": "not a list"}),
        json!({"tools": [{"type": "web_search"}]}),
    ] {
        let mut shaped = body.clone();
        shape_tools(StrictTools::On, SchemaKeywords::Basic, &mut shaped);
        assert_eq!(shaped, body);
    }
}

#[test]
fn llm_u_041_the_settings_parse_only_their_documented_values() {
    assert_eq!(StrictTools::parse(None), Some(StrictTools::Auto));
    assert_eq!(StrictTools::parse(Some("on")), Some(StrictTools::On));
    assert_eq!(StrictTools::parse(Some("off")), Some(StrictTools::Off));
    assert_eq!(StrictTools::parse(Some("auto")), Some(StrictTools::Auto));
    assert_eq!(StrictTools::parse(Some("On")), None);
    assert_eq!(StrictTools::parse(Some("yes")), None);
    assert_eq!(SchemaKeywords::parse(None), Some(SchemaKeywords::Full));
    assert_eq!(
        SchemaKeywords::parse(Some("basic")),
        Some(SchemaKeywords::Basic)
    );
    assert_eq!(
        SchemaKeywords::parse(Some("full")),
        Some(SchemaKeywords::Full)
    );
    assert_eq!(SchemaKeywords::parse(Some("minimal")), None);
}
