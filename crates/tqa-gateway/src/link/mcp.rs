//! The MCP servers a session may use (revision 7, ADR-0011, ADR-0031): the conversation's catalogue servers, which the
//! runtime starts inside the sandbox, and its connectors, which it reaches through the gateway's proxy. Only entries
//! that exist and apply now are delivered: a disabled or deleted connector, or a server whose templates exclude the
//! conversation's, is left out at once (the proxy refuses them anyway). Pure, so every rule is a unit test (GW-U-046).
use tqa_types::{MAX_MCP_SERVERS, McpServer, McpToolFilter, McpTransport};

use crate::{
    config::{McpCatalogue, McpToolFilterInput},
    connectors::ConnectorDocument,
};

/// The session's `mcp` entries: the catalogue servers in the conversation's order, then its connectors, at most the
/// runtime's bound (writes keep the two sets within it together).
pub(super) fn entries(
    catalogue: &McpCatalogue,
    connectors: &[ConnectorDocument],
    template: &str,
    (servers, chosen): (&[String], &[String]),
) -> Vec<McpServer> {
    let stdio = servers.iter().filter_map(|name| {
        let server = catalogue.get(name)?;
        let applies = server.templates().is_empty()
            || server.templates().iter().any(|allowed| allowed == template);
        applies.then(|| McpServer {
            name: server.name().to_owned(),
            transport: McpTransport::Stdio {
                command: server.command().to_owned(),
                args: server.args().to_vec(),
                env: server.env().clone(),
            },
            tools: server.tools().map(|filter| match filter {
                McpToolFilterInput::Allow(names) => McpToolFilter::Allow(names.clone()),
                McpToolFilterInput::Deny(names) => McpToolFilter::Deny(names.clone()),
            }),
            call_timeout_seconds: server.call_timeout_seconds(),
        })
    });
    let gateway = chosen.iter().filter_map(|name| {
        let connector = connectors
            .iter()
            .find(|document| &document.name == name && document.enabled)?;
        Some(McpServer {
            name: connector.name.clone(),
            transport: McpTransport::Gateway {
                connector: connector.name.clone(),
            },
            tools: connector.tools.as_ref().and_then(|filter| {
                match (&filter.allow, &filter.deny) {
                    (Some(allow), None) => Some(McpToolFilter::Allow(allow.clone())),
                    (None, Some(deny)) => Some(McpToolFilter::Deny(deny.clone())),
                    // Validation stores exactly one list; the proxy enforces the stored filter whatever the runtime is told.
                    _ => None,
                }
            }),
            call_timeout_seconds: connector.call_timeout_seconds,
        })
    });
    stdio.chain(gateway).take(MAX_MCP_SERVERS).collect()
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
