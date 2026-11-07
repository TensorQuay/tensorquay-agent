//! The MCP stdio checks (TK-U-013), run with the runtime's own client and registry rules, so a server
//! passes exactly when a run could use it. No listed tool is called unless a fixture asks for it.
use std::{fmt::Write, path::Path, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use tqa_runtime::mcp::{CallOutput, ListedTool, McpCallError, McpFailure, StdioServer, Stopped};
use tqa_tools::{
    ModelNames, RegistryError, Tool, ToolContext, ToolError, ToolName, ToolOutput, ToolRegistry,
    ToolSpec, builtin::BUILTIN_TOOL_NAMES,
};
use tqa_types::{McpServer, ToolCatalogLimit};

use crate::report::{Check, Report, ServerInfo, Status, ToolEntry};

/// The longest server-provided text (a tool or server name) quoted in a message or the report.
const MAX_QUOTED_CHARS: usize = 64;
/// The most fixture calls one run makes.
pub const MAX_CALLS: usize = 64;
/// The largest fixture file.
pub const MAX_CALLS_BYTES: u64 = 1024 * 1024;
/// The name of the probe for a tool the server did not list.
const UNLISTED_TOOL: &str = "ext_check_unlisted_tool";

/// The bounds the checks run under.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Spawn, handshake and first listing (the runtime's `HANDSHAKE`).
    pub handshake: Duration,
    /// Each call.
    pub call_timeout: Duration,
}

/// What a fixture call expects.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    /// A result the server did not mark as an error.
    Ok,
    /// An error result or a protocol error.
    Error,
}

/// One call from a `--calls` file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallFixture {
    /// The server's own tool name.
    pub tool: String,
    /// The arguments; an empty object by default.
    #[serde(default = "empty_object")]
    pub arguments: Value,
    /// The expected outcome.
    pub expect: Expect,
    /// Text the result must contain.
    #[serde(default)]
    pub contains: Option<String>,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

/// Reads a `--calls` file: a JSON array of at most [`MAX_CALLS`] calls, within [`MAX_CALLS_BYTES`].
///
/// # Errors
/// Returns a sentence naming the bound or the entry and field that failed, never a value.
pub fn load_calls(path: &Path) -> Result<Vec<CallFixture>, String> {
    let size = std::fs::metadata(path)
        .map_err(|_| "the --calls file cannot be read".to_owned())?
        .len();
    if size > MAX_CALLS_BYTES {
        return Err("the --calls file exceeds 1 MiB".to_owned());
    }
    let text =
        std::fs::read_to_string(path).map_err(|_| "the --calls file cannot be read".to_owned())?;
    let entries: Vec<Value> = serde_json::from_str(&text)
        .map_err(|_| "the --calls file must be a JSON array".to_owned())?;
    if entries.len() > MAX_CALLS {
        return Err(format!("the --calls file has more than {MAX_CALLS} calls"));
    }
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            serde_json::from_value(entry).map_err(|_| {
                format!("--calls entry {index} needs tool, expect (ok or error) and optional arguments and contains")
            })
        })
        .collect()
}

/// A listed tool as the registry sees it; ext-check calls tools through the server, never through this.
struct Listed(ToolSpec);

#[async_trait]
impl Tool for Listed {
    fn spec(&self) -> &ToolSpec {
        &self.0
    }
    async fn call(&self, _: Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        Err(ToolError::mcp(
            "not_callable",
            "ext-check calls the server directly",
        ))
    }
}

/// Runs every check against one stdio server and records them in `report`.
pub async fn check_stdio(
    server: &McpServer,
    workspace: &Path,
    calls: &[CallFixture],
    limits: &Limits,
    report: &mut Report,
) {
    let started = match StdioServer::start(server, workspace, limits.handshake).await {
        Ok(started) => started,
        Err(failure) => {
            let (code, message) = start_failure(failure);
            report.push(Check::new(code, Status::Fail, message));
            return;
        }
    };
    report.push(Check::new(
        "handshake",
        Status::Pass,
        "the server completed the handshake at protocol 2025-11-25 within its bound",
    ));
    report.server = started.server_info().map(|(name, version)| ServerInfo {
        name: quoted(&name),
        version: quoted(&version),
    });
    let listed = started.tools();
    report.push(Check::new(
        "tools_listed",
        Status::Pass,
        &format!("{} tools listed, within the bound of 64", listed.len()),
    ));
    report.tools_sha256 = Some(tools_hash(listed));
    let registry = discover(&server.name, listed, report);
    probe_unlisted(&started, listed, limits, report).await;
    let session = Session {
        server: &started,
        registry: &registry,
        name: &server.name,
        limits,
    };
    for call in calls {
        fixture(&session, call, report).await;
    }
    report.push(match started.stop().await {
        Stopped::OnInputClosed => Check::new(
            "exits_on_input_closed",
            Status::Pass,
            "the server exited once its input closed",
        ),
        Stopped::OnTerminate | Stopped::Killed => Check::new(
            "no_exit_on_eof",
            Status::Warn,
            "the server kept running after its input closed; the runtime ends its process group anyway",
        ),
    });
}

fn start_failure(failure: McpFailure) -> (&'static str, &'static str) {
    match failure {
        McpFailure::HandshakeTimeout => (
            "handshake_timeout",
            "the spawn, handshake and first listing did not finish within 10 s",
        ),
        McpFailure::OverLimit => ("too_many_tools", "the server lists more than 64 tools"),
        McpFailure::CommandNotFound => ("command_not_found", "the command could not be started"),
        McpFailure::TransportUnavailable => (
            "transport_unavailable",
            "the transport cannot be served here",
        ),
        McpFailure::ProtocolError => (
            "protocol_error",
            "the server broke the protocol: a line over 1 MiB, output that is not JSON-RPC, or an early exit",
        ),
    }
}

/// Registers every listed tool under the runtime's rules and records names, classes and warnings.
fn discover(server: &str, listed: &[ListedTool], report: &mut Report) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    let mut refused = false;
    for tool in listed {
        let Ok(name) = ToolName::mcp(server, &tool.name) else {
            refused = true;
            report.push(refusal(
                "tool_name_invalid",
                &tool.name,
                "is not a valid MCP tool name",
            ));
            continue;
        };
        let spec = ToolSpec {
            name,
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
            parallel_safe: true,
            changes_workspace: false,
        };
        if let Err(error) = registry.register_mcp(Arc::new(Listed(spec))) {
            refused = true;
            let (code, reason) = registry_refusal(&error);
            report.push(refusal(code, &tool.name, reason));
        }
    }
    if !refused {
        report.push(Check::new(
            "discovery",
            Status::Pass,
            "every tool registers under the runtime's rules",
        ));
    }
    record_names(server, &registry, report);
    registry
}

fn registry_refusal(error: &RegistryError) -> (&'static str, &'static str) {
    match error {
        RegistryError::InvalidSchema => (
            "schema_invalid",
            "has an input schema that is not a JSON object or does not compile",
        ),
        RegistryError::CatalogLimit(ToolCatalogLimit::SchemaBytes) => {
            ("schema_too_large", "has an input schema over 64 KiB")
        }
        RegistryError::CatalogLimit(_) => {
            ("catalogue_too_large", "takes the tool list over its bounds")
        }
        RegistryError::Duplicate => ("duplicate_tool", "is listed twice"),
        RegistryError::ReservedName => ("tool_name_invalid", "is not a valid MCP tool name"),
    }
}

fn refusal(code: &str, tool: &str, reason: &str) -> Check {
    Check::new(
        code,
        Status::Fail,
        &format!("tool {:?} {reason}", quoted(tool)),
    )
}

fn record_names(server: &str, registry: &ToolRegistry, report: &mut Report) {
    let names: Vec<ToolName> = registry.specs().map(|spec| spec.name.clone()).collect();
    let models = ModelNames::new(names.iter());
    let (mut folded, mut builtin) = (Vec::new(), Vec::new());
    for spec in registry.specs() {
        let Some((_, tool)) = spec.name.mcp_parts() else {
            continue;
        };
        let model_name = models.model(&spec.name).unwrap_or_default().to_owned();
        if model_name != format!("mcp__{server}__{tool}") {
            folded.push(format!("{:?} as {model_name}", quoted(tool)));
        }
        if BUILTIN_TOOL_NAMES.contains(&tool) {
            builtin.push(format!("{:?}", quoted(tool)));
        }
        report.tools.push(ToolEntry {
            name: quoted(tool),
            model_name,
        });
    }
    if !folded.is_empty() {
        report.push(Check::new(
            "name_folded",
            Status::Warn,
            &format!("models see {}", folded.join(", ")),
        ));
    }
    if !builtin.is_empty() {
        report.push(Check::new(
            "shadows_builtin",
            Status::Warn,
            &format!(
                "{} share a builtin's name; they are namespaced and never shadow it",
                builtin.join(", ")
            ),
        ));
    }
}

/// A call to a tool the server did not list must come back as an error, never a success.
async fn probe_unlisted(
    server: &StdioServer,
    listed: &[ListedTool],
    limits: &Limits,
    report: &mut Report,
) {
    let mut name = UNLISTED_TOOL.to_owned();
    while listed.iter().any(|tool| tool.name == name) {
        name.push('_');
    }
    let outcome = server
        .call(
            &name,
            empty_object(),
            limits.call_timeout,
            &CancellationToken::new(),
        )
        .await;
    report.push(match outcome {
        Ok(CallOutput { is_error: true, .. }) | Err(McpCallError::Protocol(_)) => Check::new(
            "unknown_tool",
            Status::Pass,
            "a call to a tool the server did not list is answered as an error",
        ),
        Ok(_) => Check::new(
            "unknown_tool_not_error",
            Status::Fail,
            "a call to a tool the server did not list is answered as a success",
        ),
        Err(error) => call_error(&error),
    });
}

fn call_error(error: &McpCallError) -> Check {
    match error {
        McpCallError::Timeout => Check::new(
            "call_timeout",
            Status::Fail,
            "a call did not return within its bound",
        ),
        _ => Check::new(
            "server_crashed",
            Status::Fail,
            "the server exited or closed its output during a call",
        ),
    }
}

/// A started server with its registered tools, for the fixture calls.
struct Session<'a> {
    server: &'a StdioServer,
    registry: &'a ToolRegistry,
    name: &'a str,
    limits: &'a Limits,
}

async fn fixture(session: &Session<'_>, call: &CallFixture, report: &mut Report) {
    let Session {
        server,
        registry,
        name: server_name,
        limits,
    } = *session;
    let tool = quoted(&call.tool);
    let internal = ToolName::mcp(server_name, &call.tool).ok();
    let Some(internal) = internal.filter(|name| registry.get(name.as_str()).is_some()) else {
        report.push(Check::new(
            "call_tool_not_listed",
            Status::Fail,
            &format!("the fixture calls {tool:?}, which the server did not list"),
        ));
        return;
    };
    // The run checks arguments against the schema before any call, so invalid ones are never sent.
    if registry
        .prepare(internal.as_str(), &call.arguments)
        .is_err()
    {
        report.push(Check::new(
            "call_arguments_invalid",
            Status::Fail,
            &format!("the fixture's arguments for {tool:?} fail its schema, so a run would never send them"),
        ));
        return;
    }
    let outcome = server
        .call(
            &call.tool,
            call.arguments.clone(),
            limits.call_timeout,
            &CancellationToken::new(),
        )
        .await;
    let is_error = match &outcome {
        Ok(output) => output.is_error,
        Err(McpCallError::Protocol(_)) => true,
        Err(error) => return report.push(call_error(error)),
    };
    let check = match (call.expect, is_error) {
        (Expect::Ok, true) => Check::new(
            "call_failed",
            Status::Fail,
            &format!("{tool:?} answered with an error"),
        ),
        (Expect::Error, false) => Check::new(
            "call_unexpected_ok",
            Status::Fail,
            &format!("{tool:?} succeeded where the fixture expects an error"),
        ),
        _ => match (&call.contains, &outcome) {
            (Some(expected), Ok(output)) if !output.text.contains(expected.as_str()) => Check::new(
                "call_text_missing",
                Status::Fail,
                &format!("{tool:?}'s result lacks the fixture's text"),
            ),
            _ => Check::new(
                "call",
                Status::Pass,
                &format!("{tool:?} answered as the fixture expects"),
            ),
        },
    };
    report.push(check);
}

/// SHA-256 over the listed tools as canonical JSON (keys sorted), so the same list always hashes the same.
fn tools_hash(listed: &[ListedTool]) -> String {
    let tools: Vec<Value> = listed
        .iter()
        .map(|tool| {
            canonical(&serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
                "annotations": tool.annotations,
            }))
        })
        .collect();
    let digest = Sha256::digest(Value::Array(tools).to_string().as_bytes());
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonical(&map[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// Server-provided text, bounded and without control characters.
fn quoted(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(MAX_QUOTED_CHARS)
        .collect()
}
