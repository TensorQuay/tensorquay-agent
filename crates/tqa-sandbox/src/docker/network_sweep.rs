//! Reconciles per-sandbox networks with the sandboxes that exist (ADR-0021 amendment): after a gateway restart the new
//! gateway container is attached again; a network whose sandbox is gone is removed and its subnet returned; a sandbox
//! still on the former shared network is reported so the gateway can retire it.

use std::collections::{HashMap, HashSet};

use bollard::{
    models::{EndpointSettings, NetworkConnectRequest},
    query_parameters::{ListContainersOptions, ListNetworksOptions},
};

use super::{
    DockerBackend, io, records,
    sandbox_network::{GATEWAY_ALIAS, NETWORK_LABEL, SUBNET_LABEL},
};
use crate::{SandboxError, SandboxHandle};

/// At most this many sandbox networks are reconciled per pass; the rest wait for the next one.
pub const MAX_SWEEP_NETWORKS: usize = 4096;

/// What one reconciliation pass did.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct NetworkSweep {
    /// Live sandbox networks the gateway was attached to again (only when asked to reattach).
    pub reattached: u32,
    /// Networks without a sandbox that were removed, returning their subnets.
    pub removed: u32,
    /// Networks that could not be reattached or removed; they keep their subnets and are tried again next pass.
    pub failed: u32,
    /// Sandboxes still on the former shared network, for the caller to retire (they resume in a new sandbox).
    pub legacy: Vec<SandboxHandle>,
}

impl DockerBackend {
    /// Reconciles this instance's sandbox networks. `reattach` is for start-up, when the gateway container is new and
    /// on none of them; the periodic pass only removes orphans. The development mode has nothing to reconcile.
    ///
    /// # Errors
    /// Transport errors from listing; failures on single networks are counted, not returned.
    pub async fn reconcile_networks(&self, reattach: bool) -> Result<NetworkSweep, SandboxError> {
        let mut sweep = NetworkSweep::default();
        if self.config.network != super::NetworkMode::Internal {
            return Ok(sweep);
        }
        // Creates hold this permit from network creation until their container exists, so a network is never
        // mistaken for an orphan halfway through a create.
        let _permit = tokio::time::timeout(self.config.operation_timeout, self.admission.acquire())
            .await
            .map_err(|_| SandboxError::Timeout)?
            .map_err(|_| SandboxError::Backend("creation admission closed".into()))?;
        let (used, legacy) = self.sandbox_networks().await?;
        sweep.legacy = legacy;
        let networks = self.labelled_networks().await?;
        for name in networks.into_iter().take(MAX_SWEEP_NETWORKS) {
            match self.sweep_one(&name, used.contains(&name), reattach).await {
                Swept::Reattached => sweep.reattached += 1,
                Swept::Removed => sweep.removed += 1,
                Swept::Failed => sweep.failed += 1,
                Swept::Untouched => {}
            }
        }
        Ok(sweep)
    }

    /// Rejoins a live sandbox's network (when asked), or removes one whose sandbox is gone.
    async fn sweep_one(&self, name: &str, live: bool, reattach: bool) -> Swept {
        let (result, done, what) = match (live, reattach) {
            (true, false) => return Swept::Untouched,
            (true, true) => (
                self.reattach(name).await,
                Swept::Reattached,
                "gateway could not rejoin a sandbox network",
            ),
            (false, _) => (
                self.detach_network(name, None).await,
                Swept::Removed,
                "orphaned sandbox network could not be removed",
            ),
        };
        match result {
            Ok(()) => done,
            Err(error) => {
                tracing::warn!(code = error.code(), "{what}");
                Swept::Failed
            }
        }
    }

    /// Subnets still available in the pool.
    ///
    /// # Errors
    /// Only a poisoned allocator lock.
    pub fn pool_free(&self) -> Result<u32, SandboxError> {
        Ok(self
            .allocator
            .lock()
            .map_err(|_| SandboxError::Backend("subnet allocator poisoned".into()))?
            .free())
    }

    /// The networks this instance's sandboxes name (in any state), and the sandboxes that name none.
    async fn sandbox_networks(
        &self,
    ) -> Result<(HashSet<String>, Vec<SandboxHandle>), SandboxError> {
        let containers = io::request(
            self.config.operation_timeout,
            self.docker.list_containers(Some(ListContainersOptions {
                all: true,
                filters: Some(HashMap::from([(
                    "label".into(),
                    vec![
                        "tqa.conversation".into(),
                        format!("tqa.instance={}", self.config.network_prefix),
                    ],
                )])),
                ..Default::default()
            })),
        )
        .await?;
        if containers.len() > self.config.max_containers {
            return Err(SandboxError::Backend(
                "container listing exceeded configured bound".into(),
            ));
        }
        let mut used = HashSet::new();
        let mut legacy = Vec::new();
        for container in containers {
            match container
                .labels
                .as_ref()
                .and_then(|labels| labels.get(NETWORK_LABEL))
            {
                Some(network) => {
                    used.insert(network.clone());
                }
                None => legacy.push(records::handle(container)?),
            }
        }
        Ok((used, legacy))
    }

    /// The names of this instance's per-sandbox networks (those carrying a subnet label), sorted.
    async fn labelled_networks(&self) -> Result<Vec<String>, SandboxError> {
        let networks = io::request(
            self.config.operation_timeout,
            self.docker.list_networks(Some(ListNetworksOptions {
                filters: Some(HashMap::from([(
                    "label".into(),
                    vec![
                        format!("tqa.instance={}", self.config.network_prefix),
                        SUBNET_LABEL.into(),
                    ],
                )])),
            })),
        )
        .await?;
        let mut names: Vec<String> = networks
            .into_iter()
            .filter_map(|network| network.name)
            .collect();
        names.sort();
        Ok(names)
    }

    /// Attaches the gateway container to a live sandbox's network; already attached is success.
    async fn reattach(&self, name: &str) -> Result<(), SandboxError> {
        let gateway = self
            .config
            .gateway_container
            .clone()
            .ok_or(SandboxError::Preflight(
                "per-sandbox networks need the gateway container's name",
            ))?;
        let connected = tokio::time::timeout(
            self.config.operation_timeout,
            self.docker.connect_network(
                name,
                NetworkConnectRequest {
                    container: gateway,
                    endpoint_config: Some(EndpointSettings {
                        aliases: Some(vec![GATEWAY_ALIAS.into()]),
                        ..Default::default()
                    }),
                },
            ),
        )
        .await
        .map_err(|_| SandboxError::Timeout)?;
        match connected {
            Ok(()) => Ok(()),
            Err(error) if already_attached(&error) => Ok(()),
            Err(error) => Err(io::map_error(&error)),
        }
    }
}

/// Docker's answer when the container already has an endpoint on the network.
fn already_attached(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError { status_code: 403, message }
            if message.contains("already exists")
    )
}

/// One network's outcome in a pass.
enum Swept {
    Reattached,
    Removed,
    Failed,
    Untouched,
}
