use std::{
    collections::HashSet,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tqa_types::ConversationId;

use crate::{ReadinessSource, SandboxError, SandboxSpec};

/// Backend binding, fixed for the lifetime of a conversation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// A hardened Docker Engine container.
    Docker,
    /// A future jailed microVM backend.
    Firecracker,
}

/// A backend identity and the immutable conversation binding used for reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxHandle {
    /// Runtime backend owning the identifier.
    pub kind: BackendKind,
    /// Backend-generated sandbox identifier.
    pub id: String,
    /// Validated conversation binding.
    pub conversation_id: ConversationId,
    /// UTC creation instant sampled once by create.
    pub created_at: SystemTime,
}

/// One bounded sample of cumulative resource accounting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SandboxStats {
    /// Cumulative CPU time in milliseconds.
    pub cpu_millis: u64,
    /// Current resident memory usage in bytes.
    pub memory_bytes: u64,
    /// Current process count.
    pub pids: u64,
    /// Duration since the recorded creation time.
    pub uptime: Duration,
}

/// Disposable execution boundary; all operations are bounded and destruction is idempotent.
#[async_trait]
pub trait SandboxBackend: Send + Sync {
    /// The backend implemented by this instance.
    fn kind(&self) -> BackendKind;
    /// Creates and starts a sandbox, cleaning up partial creation on failure.
    ///
    /// # Errors
    /// Returns validation, image, backend, timeout or cancellation errors.
    async fn create(
        &self,
        spec: SandboxSpec,
        cancel: CancellationToken,
    ) -> Result<SandboxHandle, SandboxError>;
    /// Reads the started sandbox's address on this backend's owned network.
    ///
    /// # Errors
    /// Rejects foreign or missing sandboxes and unavailable addresses.
    async fn address(&self, handle: &SandboxHandle) -> Result<std::net::IpAddr, SandboxError>;
    /// Waits for registration, failing early on container exit.
    ///
    /// # Errors
    /// Returns `RuntimeExited` with its process code when it exits before registration;
    /// otherwise returns backend, timeout, cancellation or missing-sandbox errors.
    async fn wait_ready(
        &self,
        handle: &SandboxHandle,
        readiness: &dyn ReadinessSource,
        deadline: Duration,
        cancel: CancellationToken,
    ) -> Result<(), SandboxError>;
    /// Fetches one bounded resource sample.
    ///
    /// # Errors
    /// Returns missing-sandbox, timeout or backend errors.
    async fn stats(&self, handle: &SandboxHandle) -> Result<SandboxStats, SandboxError>;
    /// Stops and removes a sandbox; an already absent sandbox succeeds.
    ///
    /// # Errors
    /// Returns timeout or backend errors if cleanup could not complete.
    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError>;
    /// Lists the labelled sandbox inventory for this gateway instance.
    ///
    /// # Errors
    /// Returns timeout or backend errors, including malformed managed labels.
    async fn list(&self) -> Result<Vec<SandboxHandle>, SandboxError>;
    /// The container engine's own networks: every network's subnets, and its gateway addresses (the host's address on
    /// each bridge). An internal egress rule never reaches them (ADR-0035 amendment). A backend without an engine has
    /// none.
    ///
    /// # Errors
    /// Returns timeout or backend errors when the engine can't be asked; the caller then treats them as unknown.
    async fn engine_networks(&self) -> Result<EngineNetworks, SandboxError> {
        Ok(EngineNetworks::default())
    }
}

/// The container engine's networks, as it reports them: subnets in CIDR form (IPv4 and IPv6) and gateway addresses.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EngineNetworks {
    /// Each network's subnets, as the engine writes them (an IPv4 or IPv6 network with its prefix).
    pub subnets: Vec<String>,
    /// Each network's gateway address: the host's own address on that network.
    pub gateways: Vec<std::net::IpAddr>,
}

/// Inventory differences; deciding to destroy or mark interrupted belongs to the gateway.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reconciliation {
    /// Live sandboxes absent from the gateway inventory.
    pub orphans: Vec<SandboxHandle>,
    /// Gateway records absent from the live backend inventory.
    pub missing: Vec<SandboxHandle>,
}

/// Computes differences by backend and identifier, preserving input order and making no I/O.
#[must_use]
pub fn reconcile(known: &[SandboxHandle], live: &[SandboxHandle]) -> Reconciliation {
    let identities = |handles: &[SandboxHandle]| {
        handles
            .iter()
            .map(|handle| (handle.kind, handle.id.clone()))
            .collect::<HashSet<_>>()
    };
    let known_ids = identities(known);
    let live_ids = identities(live);
    Reconciliation {
        orphans: live
            .iter()
            .filter(|handle| !known_ids.contains(&(handle.kind, handle.id.clone())))
            .cloned()
            .collect(),
        missing: known
            .iter()
            .filter(|handle| !live_ids.contains(&(handle.kind, handle.id.clone())))
            .cloned()
            .collect(),
    }
}
