//! MCP servers a session may use (revision 7, ADR-0011, ADR-0031): a stdio process inside the sandbox or a gateway
//! connector. A connector's URL is never on the wire: the runtime derives it from its own gateway origin, so a session
//! configuration cannot point a runtime at another host.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{
    DEFAULT_MCP_CALL_TIMEOUT_SECONDS, MAX_MCP_ARGS, MAX_MCP_CALL_TIMEOUT_SECONDS, MAX_MCP_ENV,
    MAX_MCP_ENV_VALUE_BYTES, MAX_MCP_NAME_BYTES, MAX_MCP_SERVERS, MAX_MCP_STRING_BYTES,
    MAX_MCP_TOOL_FILTER, MCP_NAME_RULE,
};

/// One MCP server; its tools are named `mcp.<name>.<tool>` inside the runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpServer {
    /// The server's name under [`MCP_NAME_RULE`], unique among the session's servers.
    #[serde(deserialize_with = "name")]
    #[schemars(schema_with = "name_schema")]
    pub name: String,
    /// How the runtime reaches the server.
    pub transport: McpTransport,
    /// An optional allow or deny list of the server's tool names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<McpToolFilter>,
    /// The longest one tool call may take, from 1 through 600 seconds.
    #[serde(default = "default_timeout", deserialize_with = "timeout")]
    #[schemars(range(min = 1, max = 600))]
    pub call_timeout_seconds: u32,
}

/// How the runtime reaches an MCP server.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum McpTransport {
    /// A process inside the sandbox, spoken to over its standard input and output.
    Stdio {
        /// An absolute path inside the sandbox, within 1024 bytes.
        #[serde(deserialize_with = "command")]
        #[schemars(schema_with = "command_schema")]
        command: String,
        /// At most 32 arguments of at most 1024 bytes each.
        #[serde(default, deserialize_with = "args")]
        #[schemars(schema_with = "args_schema")]
        args: Vec<String>,
        /// At most 32 variables; never a secret, and never a runtime `TQA_` name.
        #[serde(default, deserialize_with = "env")]
        #[schemars(schema_with = "env_schema")]
        env: BTreeMap<String, String>,
    },
    /// A connector the gateway proxies at `<gateway origin>/mcp/<connector>` with the runtime's own token.
    Gateway {
        /// The connector's name under [`MCP_NAME_RULE`].
        #[serde(deserialize_with = "name")]
        #[schemars(schema_with = "name_schema")]
        connector: String,
    },
}

/// Which of a server's tools the session may use.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum McpToolFilter {
    /// Only these tools.
    Allow(
        #[serde(deserialize_with = "tool_names")]
        #[schemars(schema_with = "tool_names_schema")]
        Vec<String>,
    ),
    /// Every tool except these.
    Deny(
        #[serde(deserialize_with = "tool_names")]
        #[schemars(schema_with = "tool_names_schema")]
        Vec<String>,
    ),
}

/// Whether a server or connector name follows [`MCP_NAME_RULE`] within [`MAX_MCP_NAME_BYTES`].
#[must_use]
pub fn valid_mcp_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() <= MAX_MCP_NAME_BYTES
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.last() != Some(&b'_')
        && !name.contains("__")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// Parses a session's MCP servers: at most eight, with names unique across both transports.
/// Every refusal names the list, whichever field inside it failed.
pub(crate) fn servers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<McpServer>, D::Error> {
    let servers = Vec::<McpServer>::deserialize(deserializer)
        .map_err(|error| D::Error::custom(format!("mcp: {error}")))?;
    if servers.len() > MAX_MCP_SERVERS {
        return Err(D::Error::custom("mcp: expected at most 8 servers"));
    }
    for (index, server) in servers.iter().enumerate() {
        if servers[..index]
            .iter()
            .any(|earlier| earlier.name == server.name)
        {
            return Err(D::Error::custom(
                "mcp: server names must be unique across stdio servers and connectors",
            ));
        }
    }
    Ok(servers)
}

pub(crate) fn servers_schema(generator: &mut SchemaGenerator) -> Schema {
    let item = generator.subschema_for::<McpServer>();
    json_schema!({"type":"array","maxItems":MAX_MCP_SERVERS,"items":item})
}

const fn default_timeout() -> u32 {
    DEFAULT_MCP_CALL_TIMEOUT_SECONDS
}

pub(crate) fn name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if !valid_mcp_name(&name) {
        return Err(D::Error::custom(
            "expected a name of 1 through 16 bytes: a lowercase letter, then lowercase letters, digits and single underscores, not ending in one",
        ));
    }
    Ok(name)
}
pub(crate) fn name_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string","pattern":MCP_NAME_RULE,"maxLength":MAX_MCP_NAME_BYTES})
}

fn timeout<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let seconds = u32::deserialize(deserializer)?;
    if !(1..=MAX_MCP_CALL_TIMEOUT_SECONDS).contains(&seconds) {
        return Err(D::Error::custom(
            "expected a call timeout of 1 through 600 seconds",
        ));
    }
    Ok(seconds)
}

fn command<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let command = String::deserialize(deserializer)?;
    if !command.starts_with('/') || command.len() > MAX_MCP_STRING_BYTES {
        return Err(D::Error::custom(
            "expected an absolute command path within 1024 bytes",
        ));
    }
    Ok(command)
}
fn command_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string","pattern":"^/","maxLength":MAX_MCP_STRING_BYTES})
}

fn args<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let args = Vec::<String>::deserialize(deserializer)?;
    if args.len() > MAX_MCP_ARGS || args.iter().any(|arg| arg.len() > MAX_MCP_STRING_BYTES) {
        return Err(D::Error::custom(
            "expected at most 32 arguments of at most 1024 bytes",
        ));
    }
    Ok(args)
}
fn args_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"array","maxItems":MAX_MCP_ARGS,
        "items":{"type":"string","maxLength":MAX_MCP_STRING_BYTES}})
}

/// An environment name: `[A-Z_][A-Z0-9_]{0,63}`, never in the runtime's own `TQA_` namespace.
fn env_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && !bytes[0].is_ascii_digit()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
        && !name.starts_with("TQA_")
}
fn env<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error> {
    let env = BTreeMap::<String, String>::deserialize(deserializer)?;
    if env.len() > MAX_MCP_ENV
        || env
            .iter()
            .any(|(name, value)| !env_name(name) || value.len() > MAX_MCP_ENV_VALUE_BYTES)
    {
        return Err(D::Error::custom(
            "expected at most 32 variables named [A-Z_][A-Z0-9_]{0,63} outside TQA_, with values within 4096 bytes",
        ));
    }
    Ok(env)
}
fn env_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"object","maxProperties":MAX_MCP_ENV,
        "propertyNames":{"pattern":"^[A-Z_][A-Z0-9_]{0,63}$","not":{"pattern":"^TQA_"}},
        "additionalProperties":{"type":"string","maxLength":MAX_MCP_ENV_VALUE_BYTES}})
}

fn tool_names<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let names = Vec::<String>::deserialize(deserializer)?;
    if names.len() > MAX_MCP_TOOL_FILTER
        || names
            .iter()
            .any(|name| name.is_empty() || name.len() > MAX_MCP_STRING_BYTES)
    {
        return Err(D::Error::custom(
            "expected at most 64 tool names of 1 through 1024 bytes",
        ));
    }
    Ok(names)
}
fn tool_names_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"array","maxItems":MAX_MCP_TOOL_FILTER,
        "items":{"type":"string","minLength":1,"maxLength":MAX_MCP_STRING_BYTES}})
}

/// An `mcp.server` event's tool count, at most [`MAX_MCP_TOOLS_PER_SERVER`].
pub(crate) fn tool_count<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let count = u32::deserialize(deserializer)?;
    if count as usize > crate::MAX_MCP_TOOLS_PER_SERVER {
        return Err(D::Error::custom("expected at most 64 tools"));
    }
    Ok(count)
}

/// An `mcp.server` event's reason code: 1 through 64 bytes of `[a-z_]`.
pub(crate) fn reason<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let reason = String::deserialize(deserializer)?;
    if !(1..=64).contains(&reason.len())
        || !reason
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    {
        return Err(D::Error::custom(
            "expected a reason code of 1 through 64 bytes of [a-z_]",
        ));
    }
    Ok(Some(reason))
}
pub(crate) fn reason_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string","pattern":"^[a-z_]{1,64}$"})
}
