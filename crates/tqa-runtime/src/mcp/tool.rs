//! One MCP server tool as a registry tool (ADR-0031): named `mcp.<server>.<tool>`, classed by `tighten`,
//! filtered by the server's allow or deny list, and bounded before it reaches the catalogue. Everything the server
//! says (a result, a result it marks as an error, an error's message) reaches the model in an untrusted-content block
//! naming the server and the tool (SEC-040).

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::Value;
use tqa_tools::{Tool, ToolContext, ToolError, ToolName, ToolOutput, ToolSpec};
use tqa_types::{McpServer, McpToolFilter};

use super::{CallOutput, GatewayServer, ListedTool, McpCallError, StdioServer};

/// The longest description kept from a server, in bytes.
const MAX_DESCRIPTION_BYTES: usize = 2048;

/// Where a tool's calls go.
#[derive(Clone)]
pub enum Backend {
    /// A stdio server inside the sandbox.
    Stdio(Arc<StdioServer>),
    /// A connector through the gateway's MCP proxy.
    Gateway(Arc<GatewayServer>),
}

/// A tool the runtime registers for one server.
pub struct McpTool {
    backend: Backend,
    server_name: String,
    tool: String,
    timeout: Duration,
    spec: ToolSpec,
}

impl McpTool {
    /// The tool for a listed entry, or `None` when the filter excludes it or it breaks a bound (skipped, and counted).
    #[must_use]
    pub fn new(backend: Backend, config: &McpServer, listed: &ListedTool) -> Option<Self> {
        if !allowed(config.tools.as_ref(), &listed.name) {
            return None;
        }
        let name = ToolName::mcp(&config.name, &listed.name).ok()?;
        if !listed.input_schema.is_object() {
            return None;
        }
        Some(Self {
            backend,
            server_name: config.name.clone(),
            tool: listed.name.clone(),
            timeout: Duration::from_secs(u64::from(config.call_timeout_seconds)),
            spec: ToolSpec {
                name,
                description: cap(&listed.description),
                parameters: listed.input_schema.clone(),
                parallel_safe: true,
                changes_workspace: false,
            },
        })
    }
}

/// Whether the server's allow or deny list lets a tool through; no list lets every tool through.
pub(super) fn allowed(filter: Option<&McpToolFilter>, tool: &str) -> bool {
    match filter {
        None => true,
        Some(McpToolFilter::Allow(names)) => names.iter().any(|name| name == tool),
        Some(McpToolFilter::Deny(names)) => !names.iter().any(|name| name == tool),
        Some(_) => false,
    }
}

fn cap(description: &str) -> String {
    let mut end = description.len().min(MAX_DESCRIPTION_BYTES);
    while !description.is_char_boundary(end) {
        end -= 1;
    }
    description[..end].to_owned()
}

impl McpTool {
    async fn run(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let output = match &self.backend {
            Backend::Stdio(server) => server
                .call(&self.tool, args, self.timeout, &cx.cancel)
                .await
                .map_err(|error| self.stdio_error(error))?,
            Backend::Gateway(server) => server
                .call(&self.tool, args, &cx.cancel)
                .await
                .map_err(|failure| failure.into_tool_error(&self.server_name, &self.tool))?,
        };
        Ok(result(&self.server_name, &self.tool, &output))
    }

    fn stdio_error(&self, error: McpCallError) -> ToolError {
        stdio_error(&self.server_name, &self.tool, error)
    }
}

/// A stdio call's failure for the model; a protocol error's detail carries the server's own message, so it's marked.
fn stdio_error(server: &str, tool: &str, error: McpCallError) -> ToolError {
    match error {
        McpCallError::Timeout => ToolError::Timeout,
        McpCallError::Cancelled => ToolError::Cancelled,
        McpCallError::Crashed => ToolError::mcp(
            "mcp_server_exited",
            &format!("MCP server {server} exited; it is restarted once at the next run"),
        ),
        McpCallError::Protocol(detail) => ToolError::mcp(
            "mcp_error",
            &format!("MCP server {server}: {}", mark(server, tool, &detail)),
        ),
    }
}

/// A tool's own answer, marked: text, and a failed result when the tool said `isError`.
fn result(server: &str, tool: &str, output: &CallOutput) -> ToolOutput {
    let mut result = ToolOutput::text(mark(server, tool, &output.text));
    if output.is_error {
        result.exit_code = Some(1);
    }
    result
}

/// A server's text in an untrusted-content block with source `mcp` and the registry's server and tool names.
pub(super) fn mark(server: &str, tool: &str, text: &str) -> String {
    tqa_tools::untrusted::wrap("mcp", &[("server", server), ("tool", tool)], text)
        // The names are fixed and valid, so this never happens; the text is still never passed on unmarked.
        .unwrap_or_else(|_| "[the server's text could not be marked and was dropped]".to_owned())
}

#[cfg(test)]
mod tests;

#[async_trait]
impl Tool for McpTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        self.run(args, cx).await
    }
}
