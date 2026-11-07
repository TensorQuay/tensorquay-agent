//! Compiled schema validation occurs before dispatch and does not resolve external resources.
use crate::{PreparedAction, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use tqa_types::{
    FallbackProblem, JsonType, MAX_PROBLEM_NAMES, ProblemName, ProblemPath, ToolCatalogBudget,
    ToolCatalogEntry, ToolCatalogLimit,
};

/// Registration failures do not include schema or argument contents.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// The count, encoded schema or complete catalog exceeds its shared limit.
    #[error(transparent)]
    CatalogLimit(#[from] ToolCatalogLimit),
    /// This name is already registered.
    #[error("duplicate tool name")]
    Duplicate,
    /// The schema is invalid or references an unavailable external resource.
    #[error("invalid tool parameter schema")]
    InvalidSchema,
    /// The builtin name uses a reserved namespace.
    #[error("reserved tool namespace")]
    ReservedName,
}
struct Entry {
    tool: Arc<dyn Tool>,
    spec: ToolSpec,
    validator: jsonschema::Validator,
}

/// Deterministically ordered tools with immutable compiled argument schemas.
#[derive(Default)]
pub struct ToolRegistry {
    entries: BTreeMap<String, Entry>,
    catalog: ToolCatalogBudget,
}
impl ToolRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Compiles the schema and registers the tool exactly once.
    ///
    /// # Errors
    /// Rejects catalog limits before cloning or compiling; also rejects duplicates,
    /// reserved names, non-object schemas and invalid schemas.
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        // The MCP namespace and the model-facing MCP prefix are reserved, so no builtin can shadow an MCP tool (SEC-011).
        let name = tool.spec().name.as_str();
        if name.starts_with("mcp.") || name.starts_with(crate::model_names::MCP_MODEL_PREFIX) {
            return Err(RegistryError::ReservedName);
        }
        self.insert(tool)
    }

    /// Registers an MCP server's tool, named `mcp.<server>.<tool>` (ADR-0011); a builtin name is refused here.
    ///
    /// # Errors
    /// Returns `ReservedName` for a builtin name, and otherwise the same errors as [`ToolRegistry::register`].
    pub fn register_mcp(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        if tool.spec().name.mcp_parts().is_none() {
            return Err(RegistryError::ReservedName);
        }
        self.insert(tool)
    }

    fn insert(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        if self.entries.len() == tqa_types::MAX_TOOLS {
            return Err(ToolCatalogLimit::Count.into());
        }
        let spec = tool.spec();
        let name = spec.name.as_str();
        if self.entries.contains_key(name) {
            return Err(RegistryError::Duplicate);
        }
        let catalog = self.catalog.include(ToolCatalogEntry {
            name,
            description: &spec.description,
            parameters: &spec.parameters,
            strict: true,
        })?;
        if !spec.parameters.is_object() {
            return Err(RegistryError::InvalidSchema);
        }
        let validator = jsonschema::validator_for(&spec.parameters)
            .map_err(|_| RegistryError::InvalidSchema)?;
        let name = name.to_owned();
        let spec = spec.clone();
        self.entries.insert(
            name,
            Entry {
                tool,
                spec,
                validator,
            },
        );
        self.catalog = catalog;
        Ok(())
    }
    /// Borrows a registered tool, if present.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.entries.get(name).map(|entry| &entry.tool)
    }
    /// Iterates over registered specifications in name order.
    pub fn specs(&self) -> impl Iterator<Item = &ToolSpec> {
        self.entries.values().map(|entry| &entry.spec)
    }
    /// The specifications the next model call is offered, in name order: those whose tool is [`Tool::offered`] now.
    pub fn offered_specs(&self) -> impl Iterator<Item = &ToolSpec> {
        self.entries
            .values()
            .filter(|entry| entry.tool.offered())
            .map(|entry| &entry.spec)
    }
    /// Looks up a tool and checks its schema without dispatch or permission effects.
    ///
    /// # Errors
    /// Returns a bounded known-tool list or value-free schema diagnostics.
    pub fn prepare(&self, name: &str, args: &Value) -> Result<&ToolSpec, ToolError> {
        Ok(&self.validated_entry(name, args)?.spec)
    }
    /// Fills each property the tool's schema allows to be null, and the call left out, with null, at every depth the
    /// schema describes. Builtin schemas require every property (strict endpoints need that), with optional ones
    /// nullable, so a model on a route that doesn't enforce the schema may leave one out; the run records the filled
    /// call, so validation, the prepared-action hash and dispatch all see the same arguments. An unknown tool, or a
    /// required property that isn't nullable, is left as it is.
    pub fn fill_nulls(&self, name: &str, args: &mut Value) {
        if let Some(entry) = self.entries.get(name) {
            fill_nulls(&entry.spec.parameters, args);
        }
    }
    /// For a tool whose `null` means absent ([`Tool::null_means_absent`], our strict-valid builtins), drops each `null`
    /// member the schema marks nullable, at every depth it describes: a strict model sends `null` for an optional
    /// argument it doesn't use. An empty or blank string for an optional string argument is dropped the same way: small
    /// models fill an optional string with `""` (a 13h Gate 5 demo ended on three refusals of `cursor: ""`). Any other tool's arguments are left exactly as sent. The run records the normalised
    /// call, so `tool.call`, approvals, policy matching and dispatch see the tool's real arguments (TOOL-U-034). A
    /// `null` for a non-nullable property stays, for validation to refuse; an unknown tool is left as it is.
    pub fn normalize_arguments(&self, name: &str, args: &mut Value) {
        if let Some(entry) = self
            .entries
            .get(name)
            .filter(|entry| entry.tool.null_means_absent())
        {
            drop_null_optionals(&entry.spec.parameters, args);
        }
    }
    /// What `name`'s schema refuses in `args`, named without any of their values, for the text protocol's retry
    /// sentence (ADR-0007): a missing property first (a model that wrote `path` for `pattern` then learns the real
    /// name), then an unexpected one with the properties taken there, then a wrong type, else the schema in general.
    /// A problem inside the arguments (`edits[0]`) names its place; one whose place can't be named by names and indices
    /// is the schema in general. Checked as [`Self::prepare`] checks, after filling nullable properties left out.
    /// `None` for an unknown tool, a tool whose own name isn't identifier-shaped (the caller then says the schema in
    /// general), or arguments the schema takes. A name the model wrote is kept only as [`ProblemName::model`] allows.
    #[must_use]
    pub fn schema_problem(&self, name: &str, args: &Value) -> Option<FallbackProblem> {
        let entry = self.entries.get(name)?;
        let schema = &entry.spec.parameters;
        let mut filled = args.clone();
        fill_nulls(schema, &mut filled);
        let errors: Vec<_> = entry.validator.iter_errors(&filled).collect();
        if errors.is_empty() {
            return None;
        }
        let tool = ProblemName::new(name)?;
        let first = |find: fn(
            &ProblemName,
            &Value,
            &jsonschema::ValidationError<'_>,
        ) -> Option<FallbackProblem>| {
            errors.iter().find_map(|error| find(&tool, schema, error))
        };
        Some(
            first(missing_problem)
                .or_else(|| first(unexpected_problem))
                .or_else(|| first(wrong_type_problem))
                .unwrap_or_else(|| FallbackProblem::OtherSchema { tool: tool.clone() }),
        )
    }
    fn validated_entry(&self, name: &str, args: &Value) -> Result<&Entry, ToolError> {
        let entry = self.entries.get(name).ok_or_else(|| self.unknown_tool())?;
        let mut filled = args.clone();
        fill_nulls(&entry.spec.parameters, &mut filled);
        let errors: Vec<_> = entry.validator.iter_errors(&filled).take(3).collect();
        if let Some(first) = errors.first() {
            let path = pointer(&first.instance_path().to_string());
            let detail = errors
                .iter()
                .map(|error| error_detail(error, &path))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ToolError::InvalidArgs { path, detail });
        }
        Ok(entry)
    }
    fn unknown_tool(&self) -> ToolError {
        let mut names = String::new();
        for name in self.entries.keys() {
            if names.len() + name.len() + 2 > 400 {
                names.push('…');
                break;
            }
            if !names.is_empty() {
                names.push_str(", ");
            }
            names.push_str(name);
        }
        ToolError::Unsupported(format!("unknown tool; known tools: {names}"))
    }
    /// Validates arguments against the compiled schema before calling the tool.
    ///
    /// # Errors
    /// Returns `Unsupported`, a JSON-pointer `InvalidArgs`, or the tool's failure.
    #[tracing::instrument(skip_all, fields(tool = name))]
    pub async fn call(
        &self,
        name: &str,
        args: Value,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        cx.check()?;
        let entry = self.validated_entry(name, &args)?;
        entry.tool.call(args, cx).await
    }
    /// Revalidates a consumed action and dispatches its original identity and arguments together.
    /// Core authorization and gateway grant verification remain separate mandatory boundaries.
    ///
    /// # Errors
    /// Returns the same schema and execution errors as an argument-only call.
    pub async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        cx.check()?;
        let entry = self.validated_entry(action.tool().as_str(), action.arguments())?;
        entry.tool.call_prepared(action, cx).await
    }
    /// A prepared action's screen before it runs, for its evidence ([`Tool::before`]); `None` for an unknown tool,
    /// arguments its schema refuses, or a call that takes no evidence.
    pub async fn before(
        &self,
        action: &PreparedAction,
        cx: &ToolContext,
    ) -> Option<crate::ToolImage> {
        let entry = self
            .validated_entry(action.tool().as_str(), action.arguments())
            .ok()?;
        entry.tool.before(action.arguments(), cx).await
    }
}

fn fill_nulls(schema: &Value, value: &mut Value) {
    match value {
        Value::Object(members) => {
            let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
                return;
            };
            for (key, property) in properties {
                match members.get_mut(key) {
                    Some(member) => fill_nulls(property, member),
                    None if nullable(property) => {
                        members.insert(key.clone(), Value::Null);
                    }
                    None => {}
                }
            }
        }
        Value::Array(items) => {
            if let Some(item) = schema.get("items") {
                for value in items {
                    fill_nulls(item, value);
                }
            }
        }
        _ => {}
    }
}

fn drop_null_optionals(schema: &Value, value: &mut Value) {
    match value {
        Value::Object(members) => {
            let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
                return;
            };
            members.retain(|key, member| {
                let Some(property) = properties.get(key) else {
                    return true;
                };
                let text = member.as_str();
                let blank = text.is_some_and(|text| text.trim().is_empty());
                // A null-word only where the field can't hold that word: a cursor (an opaque token), or a closed list
                // that doesn't list it, in any case (a list of ["none", "gzip"] keeps a real "None").
                let closed = key == "cursor" || property.get("enum").is_some();
                let word = closed
                    && text.is_some_and(|text| {
                        crate::builtin::null_word(text) && !lists(property, text)
                    });
                !(nullable(property)
                    && (member.is_null() || ((blank || word) && takes_string(property))))
            });
            for (key, member) in members.iter_mut() {
                if let Some(property) = properties.get(key) {
                    drop_null_optionals(property, member);
                }
            }
        }
        Value::Array(items) => {
            if let Some(item) = schema.get("items") {
                for value in items {
                    drop_null_optionals(item, value);
                }
            }
        }
        _ => {}
    }
}

/// Whether `schema`'s closed list holds `text` as one of its values, in any case.
fn lists(schema: &Value, text: &str) -> bool {
    schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .any(|value| value.trim().eq_ignore_ascii_case(text.trim()))
        })
}

fn nullable(schema: &Value) -> bool {
    match schema.get("type") {
        Some(Value::String(kind)) => kind == "null",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "null"),
        _ => false,
    }
}

/// Whether a property's schema allows a string: its `type` is `"string"`, or a list that holds it.
fn takes_string(schema: &Value) -> bool {
    schema.get("type").is_some_and(|kind| {
        kind == "string"
            || kind
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "string"))
    })
}

/// Whether every segment of a JSON pointer into the arguments is one the schema names: a key of its `properties`, or an
/// index under `items`. A map's keys (`additionalProperties`, `patternProperties`) are the model's own words, so a
/// place or property through one is never named in a problem; MCP tools bring such schemas (ADR-0007, amended).
/// The object an error is about: its place (none at the top level) and its schema. `None` when the place can't be
/// named.
fn problem_object<'a>(
    schema: &'a Value,
    pointer: &str,
) -> Option<(Option<ProblemPath>, Option<&'a Value>)> {
    let at = if pointer.is_empty() {
        None
    } else if schema_names(schema, pointer) {
        Some(ProblemPath::from_pointer(pointer)?)
    } else {
        // A key of a map (`additionalProperties`, `patternProperties`) is the model's own words, not ours.
        return None;
    };
    Some((at, schema_at(schema, pointer)))
}

/// A property the schema requires and the arguments leave out.
fn missing_problem(
    tool: &ProblemName,
    schema: &Value,
    error: &jsonschema::ValidationError<'_>,
) -> Option<FallbackProblem> {
    let jsonschema::error::ValidationErrorKind::Required { property } = error.kind() else {
        return None;
    };
    let property = property.as_str().and_then(ProblemName::new)?;
    let (at, node) = problem_object(schema, &error.instance_path().to_string())?;
    let expected = node
        .and_then(|node| node.get("properties")?.get(property.as_str()))
        .map(json_types)
        .unwrap_or_default();
    Some(FallbackProblem::MissingProperty {
        tool: tool.clone(),
        at,
        property,
        expected,
    })
}

/// A property the schema doesn't take, with the properties taken there.
fn unexpected_problem(
    tool: &ProblemName,
    schema: &Value,
    error: &jsonschema::ValidationError<'_>,
) -> Option<FallbackProblem> {
    let jsonschema::error::ValidationErrorKind::AdditionalProperties { unexpected } = error.kind()
    else {
        return None;
    };
    let (at, node) = problem_object(schema, &error.instance_path().to_string())?;
    let allowed = node
        .and_then(|node| node.get("properties")?.as_object())
        .map(property_names)
        .unwrap_or_default();
    Some(FallbackProblem::UnexpectedProperty {
        tool: tool.clone(),
        at,
        property: unexpected.iter().find_map(|key| ProblemName::model(key)),
        allowed,
    })
}

/// A property with a value of the wrong type, only where the schema names it: a map's key is the model's own words.
fn wrong_type_problem(
    tool: &ProblemName,
    schema: &Value,
    error: &jsonschema::ValidationError<'_>,
) -> Option<FallbackProblem> {
    if !matches!(
        error.kind(),
        jsonschema::error::ValidationErrorKind::Type { .. }
    ) {
        return None;
    }
    let path = error.instance_path().to_string();
    let (parent, last) = path.rsplit_once('/')?;
    if !schema_names(schema, &path) {
        return None;
    }
    let property = ProblemName::new(last)?;
    let (at, _) = problem_object(schema, parent)?;
    Some(FallbackProblem::WrongType {
        tool: tool.clone(),
        at,
        property,
        expected: schema_at(schema, &path).map(json_types).unwrap_or_default(),
    })
}

/// A schema's property names that fit a problem's name, at most [`MAX_PROBLEM_NAMES`].
fn property_names(properties: &serde_json::Map<String, Value>) -> Vec<ProblemName> {
    properties
        .keys()
        .filter_map(|key| ProblemName::new(key))
        .take(MAX_PROBLEM_NAMES)
        .collect()
}

fn schema_names(schema: &Value, pointer: &str) -> bool {
    pointer
        .split('/')
        .skip(1)
        .try_fold(schema, |node, segment| {
            if segment.bytes().all(|byte| byte.is_ascii_digit()) && node.get("items").is_some() {
                node.get("items")
            } else {
                node.get("properties")?
                    .get(segment.replace("~1", "/").replace("~0", "~"))
            }
        })
        .is_some()
}
/// The schema that describes the value at a JSON pointer into the arguments: an object's `properties`, an array's
/// `items`; `None` where the schema doesn't say.
fn schema_at<'a>(schema: &'a Value, pointer: &str) -> Option<&'a Value> {
    pointer
        .split('/')
        .skip(1)
        .try_fold(schema, |node, segment| {
            if segment.bytes().all(|byte| byte.is_ascii_digit()) && node.get("items").is_some() {
                node.get("items")
            } else {
                node.get("properties")?
                    .get(segment.replace("~1", "/").replace("~0", "~"))
            }
        })
}
/// The JSON types a schema's `type` names, in its order.
fn json_types(schema: &Value) -> Vec<JsonType> {
    let one = |name: &Value| match name.as_str()? {
        "string" => Some(JsonType::String),
        "number" => Some(JsonType::Number),
        "integer" => Some(JsonType::Integer),
        "boolean" => Some(JsonType::Boolean),
        "object" => Some(JsonType::Object),
        "array" => Some(JsonType::Array),
        "null" => Some(JsonType::Null),
        _ => None,
    };
    match schema.get("type") {
        Some(Value::Array(names)) => names.iter().filter_map(one).take(7).collect(),
        Some(name) => one(name).into_iter().collect(),
        None => Vec::new(),
    }
}
fn pointer(path: &str) -> String {
    if path.is_empty() {
        "/".into()
    } else {
        path.chars().take(80).collect()
    }
}
fn error_detail(error: &jsonschema::ValidationError<'_>, first_path: &str) -> String {
    let current = pointer(&error.instance_path().to_string());
    let detail = schema_detail(error.kind());
    if current == first_path {
        detail
    } else {
        format!("{current}: {detail}")
    }
}
fn schema_detail(kind: &jsonschema::error::ValidationErrorKind) -> String {
    use jsonschema::error::{TypeKind, ValidationErrorKind as K};
    match kind {
        K::Required { property } => format!("missing required property {}", bounded(property)),
        K::AdditionalProperties { unexpected } => unexpected
            .iter()
            .take(3)
            .map(|name| format!("unexpected property {}", bounded(&format_args!("{name:?}"))))
            .collect::<Vec<_>>()
            .join(", "),
        K::Type {
            kind: TypeKind::Single(kind),
        } => format!("expected {kind}"),
        K::Type {
            kind: TypeKind::Multiple(kinds),
        } => format!(
            "expected {}",
            kinds
                .iter()
                .map(|kind| kind.to_string())
                .collect::<Vec<_>>()
                .join(" or ")
        ),
        K::Enum { options } => format!("must be one of {}", bounded(options)),
        K::MinLength { limit } => format!("at least {limit} characters required"),
        K::MaxLength { limit } => format!("at most {limit} characters allowed"),
        K::MinItems { limit } => format!("at least {limit} items required"),
        K::MaxItems { limit } => format!("at most {limit} items allowed"),
        K::MinProperties { limit } => format!("at least {limit} properties required"),
        K::MaxProperties { limit } => format!("at most {limit} properties allowed"),
        K::Minimum { limit } => format!("must be at least {}", bounded(limit)),
        K::Maximum { limit } => format!("must be at most {}", bounded(limit)),
        K::ExclusiveMinimum { limit } => format!("must be greater than {}", bounded(limit)),
        K::ExclusiveMaximum { limit } => format!("must be less than {}", bounded(limit)),
        _ => "does not satisfy the schema".into(),
    }
}
fn bounded(value: &dyn std::fmt::Display) -> String {
    use std::fmt::Write as _;
    let mut output = BoundedText {
        text: String::new(),
        remaining: 120,
    };
    let _ = write!(output, "{value}");
    output.text
}
struct BoundedText {
    text: String,
    remaining: usize,
}
impl std::fmt::Write for BoundedText {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        for ch in text.chars() {
            if self.remaining == 0 {
                return Err(std::fmt::Error);
            }
            self.text.push(ch);
            self.remaining -= 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolName;
    struct ReservedTool(ToolSpec);
    #[async_trait::async_trait]
    impl Tool for ReservedTool {
        fn spec(&self) -> &ToolSpec {
            &self.0
        }
        async fn call(&self, _: Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
            Err(ToolError::Unsupported("reserved test tool".into()))
        }
    }
    #[test]
    fn tool_u_011_builtin_registration_defends_reserved_namespace() {
        let spec = ToolSpec {
            name: ToolName::builtin("mcp.server.tool"),
            description: String::new(),
            parameters: serde_json::json!({}),
            parallel_safe: true,
            changes_workspace: false,
        };
        let result = ToolRegistry::new().register(Arc::new(ReservedTool(spec)));
        assert!(matches!(result, Err(RegistryError::ReservedName)));
    }
    #[tokio::test]
    async fn tool_u_011_dispatch_preserves_the_registered_tool_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let clock = tqa_testkit::FakeClock::new();
        let cx = ToolContext {
            workspace: crate::WorkspaceRoot::open(directory.path())?,
            cancel: tokio_util::sync::CancellationToken::new(),
            clock: Arc::new(clock.clone()),
            sleeper: Arc::new(tqa_testkit::FakeSleeper::new(clock)),
            env_allow: vec![],
            limits: crate::ToolLimits::RECOMMENDED,
        };
        let spec = ToolSpec {
            name: ToolName::new("failure")?,
            description: "Failure propagation".into(),
            parameters: serde_json::json!({}),
            parallel_safe: true,
            changes_workspace: false,
        };
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(ReservedTool(spec)))?;
        assert_eq!(
            registry.call("failure", Value::Null, &cx).await,
            Err(ToolError::Unsupported("reserved test tool".into()))
        );
        Ok(())
    }
    /// A registry holding the named builtin tools, each accepting anything.
    fn registry_named(names: &[String]) -> Result<ToolRegistry, Box<dyn std::error::Error>> {
        let mut registry = ToolRegistry::new();
        for name in names {
            registry.register(Arc::new(ReservedTool(ToolSpec {
                name: ToolName::new(name.clone())?,
                description: String::new(),
                parameters: serde_json::json!({}),
                parallel_safe: true,
                changes_workspace: false,
            })))?;
        }
        Ok(registry)
    }

    fn known(registry: &ToolRegistry) -> String {
        match registry.unknown_tool() {
            ToolError::Unsupported(message) => message
                .strip_prefix("unknown tool; known tools: ")
                .unwrap_or(&message)
                .to_owned(),
            other => other.to_string(),
        }
    }

    // TOOL-U-011: the known-tools list is exactly bounded at 400 characters, separators included.
    #[test]
    fn tool_u_011_the_known_tools_list_is_cut_exactly_at_its_bound()
    -> Result<(), Box<dyn std::error::Error>> {
        let four = |count: usize| {
            (0..count)
                .map(|index| format!("t{index:03}"))
                .collect::<Vec<_>>()
        };
        // 67 four-character names with ", " between them are exactly 400 characters: all listed, no ellipsis.
        let listed = known(&registry_named(&four(67))?);
        assert_eq!(listed.chars().count(), 400);
        assert_eq!(listed, four(67).join(", "));
        // One more no longer fits: the first 67, then the ellipsis.
        assert_eq!(
            known(&registry_named(&four(68))?),
            format!("{}…", four(67).join(", "))
        );
        // 394 characters plus a six-character name and its separator would be 402: it is left out.
        let mut names = four(66);
        names.push("zzzzzz".into());
        assert_eq!(
            known(&registry_named(&names)?),
            format!("{}…", four(66).join(", "))
        );
        // Separators only between names.
        assert_eq!(
            known(&registry_named(&["alpha".into(), "beta".into()])?),
            "alpha, beta"
        );
        Ok(())
    }

    // CORE-U-018: a later error names its own path; the error at the reported path does not repeat it.
    #[test]
    fn core_u_018_only_errors_at_other_paths_carry_their_pointer()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(ReservedTool(ToolSpec {
            name: ToolName::new("typed")?,
            description: String::new(),
            parameters: serde_json::json!({"type": "object", "properties": {
                "a": {"type": "string"}, "b": {"type": "integer"}}}),
            parallel_safe: true,
            changes_workspace: false,
        })))?;
        let Err(ToolError::InvalidArgs { path, detail }) =
            registry.validated_entry("typed", &serde_json::json!({"a": 1, "b": "x"}))
        else {
            return Err("both properties have the wrong type".into());
        };
        let expected = match path.as_str() {
            "/a" => "expected string; /b: expected integer",
            "/b" => "expected integer; /a: expected string",
            other => return Err(format!("unexpected first path {other}").into()),
        };
        assert_eq!(detail, expected);
        Ok(())
    }

    fn detail_for(schema: &Value, instance: &Value) -> Result<String, Box<dyn std::error::Error>> {
        let validator = jsonschema::validator_for(schema)?;
        let error = validator
            .iter_errors(instance)
            .next()
            .ok_or("the instance is valid")?;
        Ok(schema_detail(error.kind()))
    }

    // CORE-U-018: each named constraint gets its own message, never the generic one.
    #[test]
    fn core_u_018_each_schema_constraint_is_named() -> Result<(), Box<dyn std::error::Error>> {
        use serde_json::json;
        let cases = [
            (
                json!({"type": ["string", "null"]}),
                json!(1),
                "expected null or string",
            ),
            (
                json!({"type": "object", "minProperties": 2}),
                json!({}),
                "at least 2 properties required",
            ),
            (
                json!({"type": "object", "maxProperties": 1}),
                json!({"a": 1, "b": 2}),
                "at most 1 properties allowed",
            ),
            (
                json!({"type": "integer", "maximum": 5}),
                json!(6),
                "must be at most 5",
            ),
            (
                json!({"type": "integer", "exclusiveMinimum": 5}),
                json!(5),
                "must be greater than 5",
            ),
        ];
        for (schema, instance, expected) in cases {
            assert_eq!(detail_for(&schema, &instance)?, expected);
        }
        Ok(())
    }

    // CORE-U-018: a value in a message is cut at exactly 120 characters.
    #[test]
    fn core_u_018_bounded_values_stop_at_exactly_120_characters() {
        let long = "x".repeat(200);
        assert_eq!(bounded(&long), "x".repeat(120));
        assert_eq!(bounded(&"y".repeat(120)), "y".repeat(120));
        assert_eq!(bounded(&"é".repeat(130)).chars().count(), 120);
    }
}
