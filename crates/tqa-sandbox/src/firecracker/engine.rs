//! The container operations the Firecracker backend needs, behind a trait: the backend's logic is tested on a fake,
//! and the Docker implementation (`docker_engine`) stays a thin, bounded translation.

use std::{collections::HashMap, net::IpAddr, time::Duration};

use async_trait::async_trait;
use bollard::models::ContainerCreateBody;

use crate::SandboxError;

/// The label that marks a container as a Firecracker VM host.
pub(crate) const BACKEND_LABEL: (&str, &str) = ("tqa.backend", "firecracker");
/// The label that marks a short-lived storage helper. Helpers carry the instance label, so a torn-down stack's
/// sweep removes them, but never [`BACKEND_LABEL`], so they are never read as VM hosts (SBX-F-024).
pub(crate) const HELPER_LABEL: (&str, &str) = ("tqa.role", "helper");

/// The labels of every short-lived helper of `instance`: storage helpers and the preflight's probes alike.
pub(crate) fn helper_labels(instance: &str) -> HashMap<String, String> {
    HashMap::from([
        ("tqa.instance".to_owned(), instance.to_owned()),
        (HELPER_LABEL.0.to_owned(), HELPER_LABEL.1.to_owned()),
    ])
}

/// What the backend reads about one container.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Container {
    /// The container name, which is the sandbox identifier.
    pub(crate) name: String,
    /// Its labels.
    pub(crate) labels: HashMap<String, String>,
    /// Whether its main process is running.
    pub(crate) running: bool,
    /// The exit code once it has stopped.
    pub(crate) exit_code: Option<i64>,
    /// Its address on the backend's internal network.
    pub(crate) address: Option<IpAddr>,
}

/// One resource sample of a VM host container: the VMM process, which includes the guest's vCPUs and memory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Usage {
    pub(crate) cpu_nanos: u64,
    pub(crate) memory_bytes: u64,
    pub(crate) pids: u64,
}

/// The state of the backend's sandbox network.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkState {
    /// No network of that name exists.
    Missing,
    /// It exists but has a route out, so it cannot confine a guest (ADR-0021).
    NotInternal,
    /// It exists and is internal.
    Internal,
}

/// What a create of a named container found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Created {
    /// Created and started.
    Started,
    /// Another create holds the name.
    NameHeld,
}

/// Bounded container operations; every call observes the engine's own timeout.
#[async_trait]
pub(crate) trait Engine: Send + Sync {
    /// Whether an image is present locally.
    async fn image_present(&self, image: &str) -> Result<bool, SandboxError>;
    /// Whether the named network exists and is internal.
    async fn network(&self, name: &str) -> Result<NetworkState, SandboxError>;
    /// Every network's subnets and gateways (ADR-0035 amendment); none unless the engine says otherwise.
    async fn networks(&self) -> Result<crate::EngineNetworks, SandboxError> {
        Ok(crate::EngineNetworks::default())
    }
    /// Creates and starts a container named `name`, each request bounded by `bound`; a failure after creation removes
    /// it again. A name another create holds (409: an earlier attempt's, still in flight or landed late) is reported,
    /// not retried here (SBX-F-035).
    async fn create_and_start(
        &self,
        name: &str,
        body: ContainerCreateBody,
        bound: Duration,
    ) -> Result<Created, SandboxError>;
    /// Inspects a container; `NotFound` when it does not exist.
    async fn inspect(&self, name: &str) -> Result<Container, SandboxError>;
    /// Stops briefly and force-removes a container; an absent container succeeds.
    async fn remove(&self, name: &str) -> Result<(), SandboxError>;
    /// Lists this instance's Firecracker containers, running or not.
    async fn list(&self, instance: &str) -> Result<Vec<Container>, SandboxError>;
    /// The names of this instance's helper containers (label `tqa.role=helper`), running or not.
    async fn helpers(&self, instance: &str) -> Result<Vec<String>, SandboxError>;
    /// One resource sample.
    async fn usage(&self, name: &str) -> Result<Usage, SandboxError>;
    /// A stopped container's last `lines` lines of output (stdout and stderr), to explain why it ended; nothing when the
    /// engine can't say. Never an error: it only adds to a failure already being reported.
    async fn log_tail(&self, name: &str, lines: usize) -> Vec<String> {
        let _ = (name, lines);
        Vec::new()
    }
    /// Runs a short-lived helper container to completion within `bound` per request, removes it, and returns its exit
    /// code.
    async fn run(&self, body: ContainerCreateBody, bound: Duration) -> Result<i64, SandboxError>;
}
