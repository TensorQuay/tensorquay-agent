//! The runtime's MCP client (ADR-0011, ADR-0031): stdio servers inside the sandbox, supervised per session.
//! Only the stdio client is public, for developer tooling (ext-check) that checks a server with the runtime's own
//! bounds; supervision, connectors and the registry tools stay inside the runtime.
pub(crate) mod gateway;
mod stdio;
pub(crate) mod supervisor;
pub(crate) mod tool;

pub(crate) use gateway::GatewayServer;
pub use stdio::{CallOutput, ListedTool, McpCallError, McpFailure, StdioServer, Stopped};
pub(crate) use supervisor::McpSupervisor;

/// The bound on a server's spawn, handshake and first listing (ADR-0011 decision 3).
pub const HANDSHAKE: std::time::Duration = std::time::Duration::from_secs(10);
