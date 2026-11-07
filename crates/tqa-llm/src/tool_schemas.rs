//! Per-route tool-schema settings: whether each tool goes out with `strict: true`, and whether
//! its schema is reduced to a basic keyword set some endpoints need. Applied to a request body's tools in both the Chat
//! (`function.parameters`/`function.strict`) and the Responses (`parameters`/`strict`) shape, by the adapters when they
//! encode and by the relay when it forwards what a runtime built. Reducing a schema only loosens what the endpoint
//! enforces: the registry still validates every call against the full schema.
use serde_json::{Map, Value};

/// When tools are sent with `strict: true`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StrictTools {
    /// Exactly the tools whose schema meets the strict rules (`tqa_types::strict_compatible`).
    #[default]
    Auto,
    /// Every tool; an endpoint that enforces strict mode refuses an incompatible schema, visibly.
    On,
    /// No tool, for an endpoint that refuses the field.
    Off,
}

impl StrictTools {
    /// The documented values; absent is `auto`, anything else is `None`.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some("auto") => Some(Self::Auto),
            Some("on") => Some(Self::On),
            Some("off") => Some(Self::Off),
            Some(_) => None,
        }
    }
}

/// Which JSON Schema keywords tool schemas keep.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SchemaKeywords {
    /// Every keyword, as the tool declares it.
    #[default]
    Full,
    /// Only [`BASIC_KEYWORDS`], at every depth.
    Basic,
}

impl SchemaKeywords {
    /// The documented values; absent is `full`, anything else is `None`.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some("full") => Some(Self::Full),
            Some("basic") => Some(Self::Basic),
            Some(_) => None,
        }
    }
}

/// The keywords a basic schema keeps: types (null unions included), descriptions, properties and required ones, array
/// items and counts, enumerations, string formats, numeric bounds, and `anyOf`. `additionalProperties` is left out: an
/// endpoint that needs the basic set may refuse it, and a closed object only matters in strict mode, which `auto` then
/// no longer chooses.
pub const BASIC_KEYWORDS: [&str; 12] = [
    "type",
    "description",
    "properties",
    "required",
    "items",
    "enum",
    "format",
    "minimum",
    "maximum",
    "minItems",
    "maxItems",
    "anyOf",
];

/// A schema with only [`BASIC_KEYWORDS`], at every depth: each property's schema, `items` and every `anyOf` branch are
/// reduced too; property names are kept as they are.
#[must_use]
pub fn reduce_schema(schema: &Value) -> Value {
    let Value::Object(members) = schema else {
        return schema.clone();
    };
    let mut reduced = Map::new();
    for (key, value) in members {
        if !BASIC_KEYWORDS.contains(&key.as_str()) {
            continue;
        }
        let value = match (key.as_str(), value) {
            ("properties", Value::Object(properties)) => Value::Object(
                properties
                    .iter()
                    .map(|(name, property)| (name.clone(), reduce_schema(property)))
                    .collect(),
            ),
            ("items", Value::Object(_)) => reduce_schema(value),
            ("anyOf", Value::Array(branches)) => {
                Value::Array(branches.iter().map(reduce_schema).collect())
            }
            _ => value.clone(),
        };
        reduced.insert(key.clone(), value);
    }
    Value::Object(reduced)
}

/// The route's settings applied to tool specifications, for an adapter to encode and budget (SEC-012) exactly what
/// goes out: a basic schema reduced, and `strict` decided on the schema as sent.
#[must_use]
pub fn shape_specs(
    strict: StrictTools,
    keywords: SchemaKeywords,
    tools: &[crate::ToolSpec],
) -> Vec<crate::ToolSpec> {
    tools
        .iter()
        .map(|tool| {
            let parameters = match keywords {
                SchemaKeywords::Full => tool.parameters.clone(),
                SchemaKeywords::Basic => reduce_schema(&tool.parameters),
            };
            let strict = match strict {
                StrictTools::Auto => tqa_types::strict_compatible(&parameters),
                StrictTools::On => true,
                StrictTools::Off => false,
            };
            crate::ToolSpec {
                parameters,
                strict,
                ..tool.clone()
            }
        })
        .collect()
}

/// Applies the settings to every function tool in `body["tools"]`, in either tool shape. Anything that is not a
/// function tool with a parameters object is left as it is.
pub fn shape_tools(strict: StrictTools, keywords: SchemaKeywords, body: &mut Value) {
    if let Some(tools) = body.get_mut("tools") {
        shape_tool_list(strict, keywords, tools);
    }
}

/// [`shape_tools`] on the `tools` value itself, for the relay's one shaping pipeline.
pub(crate) fn shape_tool_list(strict: StrictTools, keywords: SchemaKeywords, tools: &mut Value) {
    let Some(tools) = tools.as_array_mut() else {
        return;
    };
    for tool in tools {
        // Chat nests the definition under `function`; Responses keeps it at the top level.
        let definition = match tool.get("function") {
            Some(Value::Object(_)) => tool.get_mut("function"),
            _ => Some(tool),
        };
        let Some(definition) = definition.and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(parameters) = definition.get_mut("parameters") else {
            continue;
        };
        if !parameters.is_object() {
            continue;
        }
        if keywords == SchemaKeywords::Basic {
            *parameters = reduce_schema(parameters);
        }
        let strict_value = match strict {
            StrictTools::Auto => tqa_types::strict_compatible(parameters),
            StrictTools::On => true,
            StrictTools::Off => false,
        };
        definition.insert("strict".into(), Value::Bool(strict_value));
    }
}
