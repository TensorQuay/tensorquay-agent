//! Whether a tool's parameter schema meets the strict rules an endpoint documents for `strict: true` (the Chat
//! Completions and Responses protocols' documented function-calling "Strict mode" and structured-output "Supported
//! schemas", read 2026-09-27). A strict endpoint refuses a
//! request with any tool that breaks them, so a tool is sent strict only when this holds (LLM-U-034, CORE-U-051). It is
//! conservative: a keyword the documentation does not list is not vouched for.
use serde_json::{Map, Value};

/// The keywords a strict schema may use (types, the documented per-type constraints, composition by `anyOf` below the
/// root, definitions and references, and descriptions).
const KEYWORDS: [&str; 23] = [
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "const",
    "anyOf",
    "$ref",
    "$defs",
    "description",
    "pattern",
    "format",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minItems",
    "maxItems",
    // The older spelling of `$defs`.
    "definitions",
];
/// Object nesting, counting the root as the first level.
const MAX_DEPTH: usize = 10;
/// Object properties in the whole schema.
const MAX_PROPERTIES: usize = 5000;
/// Enum values in the whole schema.
const MAX_ENUM_VALUES: usize = 1000;
/// Characters of property names, definition names, enum values and const values in the whole schema.
const MAX_STRING_CHARS: usize = 120_000;

/// Whether `schema` can be sent with `strict: true` without the endpoint refusing the request.
#[must_use]
pub fn strict_compatible(schema: &Value) -> bool {
    let Some(root) = schema.as_object() else {
        return false;
    };
    if root.get("type") != Some(&Value::from("object")) || root.contains_key("anyOf") {
        return false;
    }
    let mut totals = Totals::default();
    walk(root, 1, &mut totals)
        && totals.properties <= MAX_PROPERTIES
        && totals.enum_values <= MAX_ENUM_VALUES
        && totals.string_chars <= MAX_STRING_CHARS
}

#[derive(Default)]
struct Totals {
    properties: usize,
    enum_values: usize,
    string_chars: usize,
}

/// Checks one schema object at object depth `depth` and everything below it.
fn walk(schema: &Map<String, Value>, depth: usize, totals: &mut Totals) -> bool {
    if schema.keys().any(|key| !KEYWORDS.contains(&key.as_str())) {
        return false;
    }
    count_values(schema, totals);
    let is_object = schema
        .get("type")
        .is_some_and(|kind| names_type(kind, "object"))
        || schema.contains_key("properties");
    if is_object && !closed_object(schema, depth, totals) {
        return false;
    }
    let below = if is_object { depth + 1 } else { depth };
    let nested_ok = |value: &Value, totals: &mut Totals| {
        value
            .as_object()
            .is_some_and(|nested| walk(nested, below, totals))
    };
    if let Some(items) = schema.get("items")
        && !nested_ok(items, totals)
    {
        return false;
    }
    if let Some(branches) = schema.get("anyOf") {
        let Some(branches) = branches.as_array() else {
            return false;
        };
        if !branches.iter().all(|branch| nested_ok(branch, totals)) {
            return false;
        }
    }
    ["$defs", "definitions"].iter().all(|key| {
        schema
            .get(*key)
            .is_none_or(|definitions| definitions_ok(definitions, totals))
    })
}

/// Each definition, checked at the root's depth: where it is used is not known here.
fn definitions_ok(definitions: &Value, totals: &mut Totals) -> bool {
    let Some(definitions) = definitions.as_object() else {
        return false;
    };
    definitions.iter().all(|(name, definition)| {
        totals.string_chars += name.chars().count();
        definition
            .as_object()
            .is_some_and(|nested| walk(nested, 1, totals))
    })
}

/// An object schema: closed, every property required and nothing else required, properties within bounds.
fn closed_object(schema: &Map<String, Value>, depth: usize, totals: &mut Totals) -> bool {
    if depth > MAX_DEPTH || schema.get("additionalProperties") != Some(&Value::Bool(false)) {
        return false;
    }
    let empty = Map::new();
    let properties = match schema.get("properties") {
        None => &empty,
        Some(Value::Object(properties)) => properties,
        Some(_) => return false,
    };
    let required: Vec<&str> = match schema.get("required") {
        None => Vec::new(),
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
        Some(_) => return false,
    };
    let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
    let mut sorted_required = required.clone();
    names.sort_unstable();
    sorted_required.sort_unstable();
    sorted_required.dedup();
    if names != sorted_required || required.len() != sorted_required.len() {
        return false;
    }
    totals.properties += properties.len();
    for (name, property) in properties {
        totals.string_chars += name.chars().count();
        if !property
            .as_object()
            .is_some_and(|nested| walk(nested, depth + 1, totals))
        {
            return false;
        }
    }
    true
}

fn count_values(schema: &Map<String, Value>, totals: &mut Totals) {
    if let Some(Value::Array(values)) = schema.get("enum") {
        totals.enum_values += values.len();
        totals.string_chars += values.iter().map(value_chars).sum::<usize>();
    }
    if let Some(value) = schema.get("const") {
        totals.string_chars += value_chars(value);
    }
}

fn value_chars(value: &Value) -> usize {
    value.as_str().map_or(0, |text| text.chars().count())
}

fn names_type(kind: &Value, name: &str) -> bool {
    match kind {
        Value::String(kind) => kind == name,
        Value::Array(kinds) => kinds.iter().any(|kind| kind.as_str() == Some(name)),
        _ => false,
    }
}
