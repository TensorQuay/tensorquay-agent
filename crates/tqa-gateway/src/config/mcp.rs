//! The install's MCP server catalogue, `[[mcp.servers]]` (ADR-0011 decision 2): the only place a command enters. A
//! profile and a conversation only narrow it. Validated once at start; every bound is a named constant (§1.4, §1.13).
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::{sandbox::environment_entry_ok, templates::Template};
use crate::GatewayError;

/// The servers one install may define; the runtime starts at most this many per session (ADR-0011 decision 3).
pub(crate) const MAX_MCP_SERVERS: usize = 8;
const MAX_MCP_ARGS: usize = 32;
const MAX_MCP_ARG_BYTES: usize = 1024;
const MAX_MCP_ENV: usize = 32;
/// Tool names in one allow or deny list (the revision-7 `McpServer` bound).
const MAX_MCP_TOOL_FILTER: usize = 64;
const MAX_MCP_TOOL_NAME_BYTES: usize = 128;
const MAX_MCP_COMMAND_BYTES: usize = 4096;
const DEFAULT_MCP_CALL_TIMEOUT_SECONDS: u32 = 60;
const MAX_MCP_CALL_TIMEOUT_SECONDS: u32 = 600;
/// Messages one user may send through the connector proxy per minute (ADR-0031): the default, and the bound an
/// administrator's override shares (`settings/limits.rs`).
const DEFAULT_MCP_CALLS_PER_MINUTE: u32 = 60;
const MAX_MCP_CALLS_PER_MINUTE: u32 = 6_000;
/// A server name, shared with connectors (they share the `mcp__<name>__<tool>` namespace): 1 to 16 characters.
const MAX_MCP_NAME_CHARS: usize = 16;
/// Where a command may never live: the workspace (repository-supplied code), the runtime's state and token, and
/// temporary files.
const FORBIDDEN_COMMAND_ROOTS: [&str; 3] = ["/workspace", "/run/tqa", "/tmp"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct McpInput {
    #[serde(default)]
    servers: Vec<ServerInput>,
    #[serde(default = "default_calls_per_minute")]
    calls_per_minute: u32,
}

impl Default for McpInput {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            calls_per_minute: DEFAULT_MCP_CALLS_PER_MINUTE,
        }
    }
}

const fn default_calls_per_minute() -> u32 {
    DEFAULT_MCP_CALLS_PER_MINUTE
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerInput {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    templates: Vec<String>,
    tools: Option<FilterInput>,
    #[serde(default = "default_timeout")]
    call_timeout_seconds: u32,
    #[serde(default)]
    enabled_by_default: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilterInput {
    allow: Option<Vec<String>>,
    deny: Option<Vec<String>>,
}

const fn default_timeout() -> u32 {
    DEFAULT_MCP_CALL_TIMEOUT_SECONDS
}

/// Which of a server's tools are offered: only the listed ones, or all but the listed ones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ToolFilter {
    Allow(Vec<String>),
    Deny(Vec<String>),
}

/// One validated catalogue entry.
#[derive(Clone, Debug)]
pub(crate) struct McpServer {
    name: String,
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    templates: Vec<String>,
    tools: Option<ToolFilter>,
    call_timeout_seconds: u32,
    enabled_by_default: bool,
}

impl McpServer {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn command(&self) -> &str {
        &self.command
    }
    pub(crate) fn args(&self) -> &[String] {
        &self.args
    }
    pub(crate) const fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }
    pub(crate) fn templates(&self) -> &[String] {
        &self.templates
    }
    pub(crate) const fn tools(&self) -> Option<&ToolFilter> {
        self.tools.as_ref()
    }
    pub(crate) const fn call_timeout_seconds(&self) -> u32 {
        self.call_timeout_seconds
    }
    pub(crate) const fn enabled_by_default(&self) -> bool {
        self.enabled_by_default
    }
}

/// The install's catalogue, in file order.
#[derive(Clone, Debug)]
pub(crate) struct McpCatalogue {
    servers: Vec<McpServer>,
    /// `[mcp] calls_per_minute`: the seed of the `mcp.calls_per_minute` limit (ADR-0028).
    calls_per_minute: u32,
}

impl Default for McpCatalogue {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            calls_per_minute: DEFAULT_MCP_CALLS_PER_MINUTE,
        }
    }
}

impl McpCatalogue {
    pub(crate) const fn calls_per_minute(&self) -> u32 {
        self.calls_per_minute
    }

    pub(crate) fn servers(&self) -> &[McpServer] {
        &self.servers
    }

    pub(crate) fn get(&self, name: &str) -> Option<&McpServer> {
        self.servers.iter().find(|server| server.name == name)
    }

    /// The servers a new conversation starts with until profiles exist: those enabled by default, in file order.
    pub(crate) fn defaults(&self) -> Vec<String> {
        self.servers
            .iter()
            .filter(|server| server.enabled_by_default)
            .map(|server| server.name.clone())
            .collect()
    }
}

/// The shared MCP server and connector name rule: `[a-z][a-z0-9_]{0,15}`, with no `__` and no trailing `_`, so
/// `mcp__<server>__<tool>` always splits back (ADR-0031 decision 7).
pub(crate) fn valid_server_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=MAX_MCP_NAME_CHARS).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        && !name.contains("__")
        && !name.ends_with('_')
}

/// Validates the catalogue against the configured templates.
///
/// # Errors
/// A configuration error naming the rule a server breaks.
pub(super) fn mcp(input: McpInput, templates: &[Template]) -> Result<McpCatalogue, GatewayError> {
    let invalid = GatewayError::Configuration;
    if input.servers.len() > MAX_MCP_SERVERS {
        return Err(invalid("too many [[mcp.servers]] (at most 8)"));
    }
    if !(1..=MAX_MCP_CALLS_PER_MINUTE).contains(&input.calls_per_minute) {
        return Err(invalid("invalid [mcp] calls_per_minute (1 to 6000)"));
    }
    let mut names = BTreeSet::new();
    let mut servers = Vec::with_capacity(input.servers.len());
    for server in input.servers {
        if !valid_server_name(&server.name) {
            return Err(invalid("invalid [[mcp.servers]] name"));
        }
        if !names.insert(server.name.clone()) {
            return Err(invalid("duplicate [[mcp.servers]] name"));
        }
        if !valid_command(&server.command) {
            return Err(invalid("invalid [[mcp.servers]] command"));
        }
        if server.args.len() > MAX_MCP_ARGS
            || server
                .args
                .iter()
                .any(|arg| arg.len() > MAX_MCP_ARG_BYTES || arg.contains('\0'))
        {
            return Err(invalid("invalid [[mcp.servers]] args"));
        }
        if server.env.len() > MAX_MCP_ENV
            || !server
                .env
                .iter()
                .all(|(key, value)| environment_entry_ok(key, value))
        {
            return Err(invalid("invalid [[mcp.servers]] env"));
        }
        if !server
            .templates
            .iter()
            .all(|name| templates.iter().any(|template| template.name() == name))
        {
            return Err(invalid(
                "[[mcp.servers]] templates must name configured templates",
            ));
        }
        let tools = server.tools.map(filter).transpose()?;
        if !(1..=MAX_MCP_CALL_TIMEOUT_SECONDS).contains(&server.call_timeout_seconds) {
            return Err(invalid("invalid [[mcp.servers]] call_timeout_seconds"));
        }
        servers.push(McpServer {
            name: server.name,
            command: server.command,
            args: server.args,
            env: server.env,
            templates: server.templates,
            tools,
            call_timeout_seconds: server.call_timeout_seconds,
            enabled_by_default: server.enabled_by_default,
        });
    }
    Ok(McpCatalogue {
        servers,
        calls_per_minute: input.calls_per_minute,
    })
}

/// An absolute, normalised path (no `.`, `..` or empty components, no trailing `/`) outside the forbidden roots.
fn valid_command(command: &str) -> bool {
    let Some(relative) = command.strip_prefix('/') else {
        return false;
    };
    let normal = !relative.is_empty()
        && relative
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    let forbidden = FORBIDDEN_COMMAND_ROOTS.iter().any(|root| {
        command == *root
            || command
                .strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('/'))
    });
    command.len() <= MAX_MCP_COMMAND_BYTES && !command.contains('\0') && normal && !forbidden
}

/// Exactly one non-empty list of distinct, bounded, printable tool names.
fn filter(input: FilterInput) -> Result<ToolFilter, GatewayError> {
    let (names, allow) = match (input.allow, input.deny) {
        (Some(allow), None) => (allow, true),
        (None, Some(deny)) => (deny, false),
        _ => return Err(GatewayError::Configuration("invalid [[mcp.servers]] tools")),
    };
    let distinct = names.iter().collect::<BTreeSet<_>>().len() == names.len();
    let bounded = names.iter().all(|name| {
        !name.is_empty()
            && name.len() <= MAX_MCP_TOOL_NAME_BYTES
            && !name.chars().any(char::is_control)
    });
    if names.is_empty() || names.len() > MAX_MCP_TOOL_FILTER || !distinct || !bounded {
        return Err(GatewayError::Configuration("invalid [[mcp.servers]] tools"));
    }
    Ok(if allow {
        ToolFilter::Allow(names)
    } else {
        ToolFilter::Deny(names)
    })
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
