//! The one tool-name rule for tool catalogs and the action hash (PROTO-U-016, ADR-0011, ADR-0031).
//! A name is a builtin, `[a-z][a-z0-9_]{0,63}`, or an MCP tool, `mcp.<server>.<tool>`: the server under the MCP name rule
//! and the tool the server's own name, 1 to 128 bytes of `[A-Za-z0-9_.-]`, split at the first two dots.

use schemars::{Schema, generate::SchemaGenerator, json_schema};

use crate::{MAX_MCP_TOOL_BYTES, valid_mcp_name};

/// Largest builtin tool name, in bytes.
const MAX_BUILTIN_TOOL_BYTES: usize = 64;
/// The builtin name pattern, for schemas.
pub(crate) const BUILTIN_PATTERN: &str = "^[a-z][a-z0-9_]{0,63}$";
/// The MCP name shape, for schemas; the server's `__` and trailing `_` are refused by [`MCP_SERVER_REFUSED`].
const MCP_PATTERN: &str = "^mcp\\.[a-z][a-z0-9_]{0,15}\\.[A-Za-z0-9_.-]{1,128}$";
/// A server segment with `__` or a trailing `_`.
const MCP_SERVER_REFUSED: &str = "^mcp\\.[^.]*(__|_\\.)";

/// Whether a name is a builtin tool name, `[a-z][a-z0-9_]{0,63}`.
#[must_use]
pub fn valid_builtin_tool_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() <= MAX_BUILTIN_TOOL_BYTES
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// The server and tool of an MCP tool name `mcp.<server>.<tool>`, or `None` for anything else.
#[must_use]
pub fn split_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let (server, tool) = name.strip_prefix("mcp.")?.split_once('.')?;
    (valid_mcp_name(server)
        && (1..=MAX_MCP_TOOL_BYTES).contains(&tool.len())
        && tool
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')))
    .then_some((server, tool))
}

/// Whether a name is a builtin or an MCP tool name.
#[must_use]
pub fn valid_tool_name(name: &str) -> bool {
    valid_builtin_tool_name(name) || split_mcp_tool_name(name).is_some()
}

/// The schema of a tool name, either form.
pub(crate) fn tool_name_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string","anyOf":[
        {"pattern":BUILTIN_PATTERN},
        {"pattern":MCP_PATTERN,"not":{"pattern":MCP_SERVER_REFUSED}}
    ]})
}
