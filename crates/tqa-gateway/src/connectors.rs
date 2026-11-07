//! Connectors (ADR-0031): remote MCP servers the gateway proxies to, with credentials that never leave the gateway.
pub(crate) mod credentials;
mod document;

pub use credentials::{ConnectorCredential, CredentialSource};
pub(crate) use document::{
    Auth, ConnectorDocument, Origin, Refusal, Scheme, ToolFilter, check_auth, check_url, validate,
};
