//! Each Docker sandbox's own internal network (ADR-0021 amendment): created from the dedicated subnet pool with the
//! gateway container attached as `gateway`, and removed with the sandbox, so sandboxes never share a network.

use std::collections::HashMap;

use bollard::models::{
    EndpointSettings, Ipam, IpamConfig, NetworkConnectRequest, NetworkCreateRequest,
    NetworkDisconnectRequest,
};

use super::{DockerBackend, io, pool::Cidr};
use crate::SandboxError;

/// How many subnets one create tries when Docker refuses them as overlapping another network.
const MAX_ALLOCATE_ATTEMPTS: usize = 4;
/// The name every sandbox resolves to the gateway on its own network.
pub(super) const GATEWAY_ALIAS: &str = "gateway";
/// The container label naming the sandbox's network, read back on destroy.
pub(super) const NETWORK_LABEL: &str = "tqa.network";
/// The network label holding its subnet, read back to rebuild the allocator and to release on destroy.
pub(super) const SUBNET_LABEL: &str = "tqa.subnet";

/// A sandbox network's name: short enough for every Docker and bridge limit, unique per create.
pub(super) fn network_name(nanos: u128) -> String {
    format!("tqa-sbx-{nanos:x}")
}

impl DockerBackend {
    /// Creates `name` with a subnet from the pool and attaches the gateway container. On failure nothing is left: the
    /// network is removed and its subnet returned.
    ///
    /// # Errors
    /// `Capacity` when the pool is exhausted; `Preflight` without a gateway container; transport errors.
    pub(super) async fn attach_network(&self, name: &str) -> Result<Cidr, SandboxError> {
        let gateway = self
            .config
            .gateway_container
            .clone()
            .ok_or(SandboxError::Preflight(
                "per-sandbox networks need the gateway container's name",
            ))?;
        let subnet = self.create_sandbox_network(name).await?;
        let connected = io::request(
            self.config.create_timeout,
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
        .await;
        if let Err(error) = connected {
            self.discard_network(name, subnet).await;
            return Err(error);
        }
        Ok(subnet)
    }

    /// Undoes a network while another error is being returned: a failure is logged and left to the sweep, never
    /// allowed to mask the original error.
    pub(super) async fn discard_network(&self, name: &str, subnet: Cidr) {
        if let Err(cleanup) = self.detach_network(name, Some(subnet)).await {
            tracing::warn!(
                code = cleanup.code(),
                "sandbox network cleanup failed; left to the sweep"
            );
        }
    }

    /// Allocates a subnet and creates the network, skipping subnets Docker refuses as overlapping.
    async fn create_sandbox_network(&self, name: &str) -> Result<Cidr, SandboxError> {
        for _ in 0..MAX_ALLOCATE_ATTEMPTS {
            let subnet = self.allocate()?;
            let created = tokio::time::timeout(
                self.config.create_timeout,
                self.docker
                    .create_network(self.network_request(name, subnet)),
            )
            .await;
            match created {
                Ok(Ok(_)) => return Ok(subnet),
                Ok(Err(error)) if overlapping(&error) => {
                    tracing::warn!(subnet = %subnet, "Docker refused a pool subnet as overlapping; skipping it");
                    self.with_allocator(|allocator| allocator.mark_unusable(subnet))?;
                }
                Ok(Err(error)) => {
                    self.with_allocator(|allocator| allocator.release(subnet))?;
                    return Err(io::map_error(&error));
                }
                Err(_) => {
                    // A timed-out create may still have reached the daemon: remove it by name before releasing.
                    self.discard_network(name, subnet).await;
                    return Err(SandboxError::Timeout);
                }
            }
        }
        Err(SandboxError::Backend(
            "every tried pool subnet overlaps another network".into(),
        ))
    }

    fn network_request(&self, name: &str, subnet: Cidr) -> NetworkCreateRequest {
        NetworkCreateRequest {
            name: name.into(),
            driver: Some("bridge".into()),
            internal: Some(true),
            enable_ipv6: Some(false),
            ipam: Some(Ipam {
                config: Some(vec![IpamConfig {
                    subnet: Some(subnet.to_string()),
                    gateway: Some(subnet.bridge().to_string()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            labels: Some(HashMap::from([
                ("tqa.instance".into(), self.config.network_prefix.clone()),
                (SUBNET_LABEL.into(), subnet.to_string()),
            ])),
            ..Default::default()
        }
    }

    /// Disconnects the gateway, removes the network and returns its subnet. Every step treats "already gone" as done,
    /// so a retry after a crash at any point converges. `known` is the subnet when the caller has it (create unwinding);
    /// otherwise it is read from the network's label.
    ///
    /// # Errors
    /// A network owned by another instance, one still in use by a sandbox, or transport errors.
    pub(super) async fn detach_network(
        &self,
        name: &str,
        known: Option<Cidr>,
    ) -> Result<(), SandboxError> {
        let inspected = io::request(
            self.config.operation_timeout,
            self.docker.inspect_network(name, None),
        )
        .await;
        let network = match inspected {
            Ok(network) => network,
            Err(SandboxError::NotFound) => {
                if let Some(subnet) = known {
                    self.with_allocator(|allocator| allocator.release(subnet))?;
                }
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let labels = network.labels.unwrap_or_default();
        if labels.get("tqa.instance") != Some(&self.config.network_prefix) {
            return Err(SandboxError::Preflight(
                "network name belongs to another owner",
            ));
        }
        let subnet = known.or_else(|| labels.get(SUBNET_LABEL).and_then(|text| Cidr::parse(text)));
        if let Some(gateway) = &self.config.gateway_container {
            let disconnected = io::request(
                self.config.operation_timeout,
                self.docker.disconnect_network(
                    name,
                    NetworkDisconnectRequest {
                        container: gateway.clone(),
                        force: Some(true),
                    },
                ),
            )
            .await;
            match disconnected {
                // Docker answers 403 when the container is not attached: nothing to undo.
                Ok(()) | Err(SandboxError::NotFound) => {}
                Err(error) if !gateway_attached(network.containers.as_ref(), gateway) => {
                    tracing::debug!(code = error.code(), "gateway was not attached");
                }
                Err(error) => return Err(error),
            }
        }
        match io::request(
            self.config.operation_timeout,
            self.docker.remove_network(name),
        )
        .await
        {
            Ok(()) | Err(SandboxError::NotFound) => {}
            Err(error) => return Err(error),
        }
        if let Some(subnet) = subnet {
            self.with_allocator(|allocator| allocator.release(subnet))?;
        }
        Ok(())
    }

    /// At start: the gateway container must be running; the pool must not overlap any other Docker network or this
    /// process's routes; the allocator is rebuilt from this instance's labelled networks.
    ///
    /// # Errors
    /// `Preflight` naming the problem (never a value beyond the conflicting network), or transport errors.
    pub(super) async fn prepare_networks(&self) -> Result<(), SandboxError> {
        self.check_gateway_container().await?;
        let (own, subnets) = self.network_inventory().await?;
        let (allocator, ignored) = super::pool::Allocator::rebuild(
            self.config.network_pool,
            own.iter().map(String::as_str),
        );
        if !ignored.is_empty() {
            tracing::warn!(
                count = ignored.len(),
                "labelled sandbox networks outside the pool were ignored"
            );
        }
        let owned = own
            .iter()
            .filter_map(|text| Cidr::parse(text))
            .collect::<std::collections::BTreeSet<_>>();
        let routes = tokio::fs::read_to_string("/proc/net/route")
            .await
            .map(|text| super::pool::routes(&text))
            .unwrap_or_default();
        let conflict = self
            .config
            .network_pool
            .conflict(subnets.iter().chain(&routes).map(String::as_str), &owned);
        if let Some(conflict) = conflict {
            tracing::error!(%conflict, pool = %self.config.network_pool.cidr(), "the sandbox network pool overlaps an existing network or route");
            return Err(SandboxError::Preflight(
                "the sandbox network pool overlaps an existing network or route (see the log)",
            ));
        }
        *self
            .allocator
            .lock()
            .map_err(|_| SandboxError::Backend("subnet allocator poisoned".into()))? = allocator;
        Ok(())
    }

    /// The configured gateway container exists and is running.
    async fn check_gateway_container(&self) -> Result<(), SandboxError> {
        let gateway = self
            .config
            .gateway_container
            .as_deref()
            .ok_or(SandboxError::Preflight(
                "per-sandbox networks need [sandbox] gateway_container",
            ))?;
        let running = io::request(
            self.config.operation_timeout,
            self.docker.inspect_container(gateway, None),
        )
        .await
        .map_err(|error| match error {
            SandboxError::NotFound => {
                SandboxError::Preflight("the configured gateway container does not exist")
            }
            other => other,
        })?
        .state
        .and_then(|state| state.running);
        if running == Some(true) {
            Ok(())
        } else {
            Err(SandboxError::Preflight(
                "the configured gateway container is not running",
            ))
        }
    }

    /// This instance's per-sandbox subnets (from labels) and every Docker network's IPAM subnets.
    async fn network_inventory(&self) -> Result<(Vec<String>, Vec<String>), SandboxError> {
        let networks = io::request(
            self.config.operation_timeout,
            self.docker
                .list_networks(None::<bollard::query_parameters::ListNetworksOptions>),
        )
        .await?;
        let mut own = Vec::new();
        let mut subnets = Vec::new();
        for network in &networks {
            let labels = network.labels.as_ref();
            let ours = labels.and_then(|labels| labels.get("tqa.instance"))
                == Some(&self.config.network_prefix);
            if ours && let Some(subnet) = labels.and_then(|labels| labels.get(SUBNET_LABEL)) {
                own.push(subnet.clone());
            }
            subnets.extend(
                network
                    .ipam
                    .as_ref()
                    .and_then(|ipam| ipam.config.as_ref())
                    .into_iter()
                    .flatten()
                    .filter_map(|config| config.subnet.clone()),
            );
        }
        Ok((own, subnets))
    }

    fn allocate(&self) -> Result<Cidr, SandboxError> {
        self.with_allocator(super::pool::Allocator::allocate)?
            .ok_or(SandboxError::Capacity)
    }

    fn with_allocator<T>(
        &self,
        use_it: impl FnOnce(&mut super::pool::Allocator) -> T,
    ) -> Result<T, SandboxError> {
        let mut allocator = self
            .allocator
            .lock()
            .map_err(|_| SandboxError::Backend("subnet allocator poisoned".into()))?;
        Ok(use_it(&mut allocator))
    }
}

/// Docker's answer when an explicit subnet overlaps an existing network.
fn overlapping(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError { status_code: 403, message }
            if message.contains("overlap")
    )
}

/// Whether the inspected network lists the gateway container among its endpoints (by name).
fn gateway_attached(
    containers: Option<&HashMap<String, bollard::models::EndpointResource>>,
    gateway: &str,
) -> bool {
    containers.is_some_and(|containers| {
        containers
            .values()
            .any(|container| container.name.as_deref() == Some(gateway))
    })
}
