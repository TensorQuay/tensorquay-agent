//! The gateway's shared state, one struct every handler reads (moved out of `app.rs`, which had reached the
//! file-length limit; nothing of its own).
use crate::{
    GatewayConfig, GatewaySecrets,
    auth::{LoginRate, Passwords, TokenFailures},
    events::EventStore,
    link::LinkRegistry,
    storage::Storage,
    telemetry::Telemetry,
};
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub(crate) struct State {
    pub config: GatewayConfig,
    pub secrets: GatewaySecrets,
    pub pool: PgPool,
    /// Routes today, then egress rules and limits (ADR-0028); read one snapshot per operation.
    pub settings: Arc<crate::settings::Settings>,
    pub passwords: Passwords,
    pub dummy_hash: String,
    pub login_rate: LoginRate,
    /// Failed personal-token authentications per address.
    pub token_failures: TokenFailures,
    pub storage: Storage,
    pub events: EventStore,
    pub links: Arc<LinkRegistry>,
    pub runtime_tokens: crate::RuntimeTokens,
    pub manager: Arc<crate::SandboxManager>,
    pub secret_store: Arc<dyn crate::SecretStore>,
    /// The install CA for host keys, beside the install key (ADR-0037 §6); none without secret files.
    pub host_ca: Option<Arc<crate::host_ca::HostCa>>,
    /// Which keyed-call notices went out, for their rate limits (ADR-0037, Amendment 1).
    pub host_keys: crate::host_keys::Live,
    /// Tests only: keyed hosts that closed their connection (GW-I-180).
    #[cfg(feature = "testing")]
    pub keyed_closes: crate::egress::KeyedCloses,
    #[cfg(feature = "testing")]
    pub contract: super::contract_check::Check,
    /// The roots the proxy verifies a keyed host's certificate against: the public ones (ADR-0037 §4), never the
    /// install CA. Tests add their own with [`Gateway::trust_upstream_root`].
    pub upstream_roots: std::sync::RwLock<Arc<rustls::RootCertStore>>,
    /// What internal egress rules may not reach (ADR-0035 amendment), renewed by the manager.
    pub internal: Arc<crate::egress::internal::Guard>,
    /// The proxy's recent refusals, for Settings › Internet access (ADR-0035 point 3, GW-I-323).
    pub recent_refusals: Arc<crate::egress::recent::Recent>,
    /// When each secret due for rotation was last named in the log.
    pub reminders: crate::secrets::rotation::Reminders,
    /// The live passes that serve a workspace folder as a site (18.0 previews for real sites): in memory, so a
    /// restart forgets them.
    pub site_passes: crate::files::SitePasses,
    /// The queue of secret read records to their writer.
    pub read_audit: crate::secrets::reads::ReadAudit,
    pub relay: crate::relay::Relay,
    /// The MCP proxy's resolver for connector hosts (ADR-0031).
    pub mcp: crate::mcp_proxy::McpProxy,
    pub web: crate::web::Web,
    /// Wakes channel readers on a new message (ADR-0014).
    pub channels: crate::channels::Hub,
    /// Each agent team's version and its `team.changed` frames (ADR-0034, S6).
    pub team_changes: crate::teams::changed::Changes,
    pub telemetry: Arc<Telemetry>,
    /// Where the engine's environment images are read from (the built-in environments design).
    pub environment_images: crate::environments::Images,
    /// Wakes the background purge of deleted tasks and workspaces (delete means gone).
    pub purge: crate::purge::Purge,
    /// Wakes the agent teams' reconciler when a stop is left to it (the lead's `done`, the budget).
    pub reconciler: crate::teams::reconciler::Nudge,
    pub cancel: CancellationToken,
    pub tasks: TaskTracker,
}
