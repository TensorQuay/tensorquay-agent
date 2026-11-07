use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use bollard::{
    Docker,
    models::ContainerInspectResponse,
    query_parameters::{ListContainersOptions, StatsOptions},
};
use futures::{FutureExt, StreamExt};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::{DockerBackendConfig, Preflight, io, records};
use crate::{
    BackendKind, ReadinessSource, SandboxBackend, SandboxError, SandboxHandle, SandboxSpec,
    SandboxStats, readiness,
};

/// Hardened Docker backend, scoped to one gateway instance and one explicit egress mode.
pub struct DockerBackend {
    pub(super) docker: Docker,
    pub(super) config: DockerBackendConfig,
    // One admitted creation prevents racing network policy initialization. No data lock is held.
    pub(super) admission: Semaphore,
    /// Which pool subnets per-sandbox networks hold; never locked across an await.
    pub(super) allocator: std::sync::Mutex<super::pool::Allocator>,
    /// How a sandbox's container create is retried while the daemon is too busy to answer (SBX-U-014).
    pub(super) retry: crate::retry::Retry,
}

impl DockerBackend {
    /// Connects using `DOCKER_HOST` or the platform socket and negotiates the API version.
    ///
    /// # Errors
    /// Returns validation, preflight, timeout or redacted transport errors.
    pub async fn connect(mut config: DockerBackendConfig) -> Result<Self, SandboxError> {
        config.validate()?;
        config.check_network_opt_in(std::env::var("TQA_ALLOW_DEV_HOST_GATEWAY").ok().as_deref())?;
        if config.network == super::NetworkMode::DevHostGateway {
            tracing::warn!("development host gateway disables external network isolation");
        }
        config.storage_root = tokio::time::timeout(
            config.operation_timeout,
            super::token::prepare(&config.storage_root),
        )
        .await
        .map_err(|_| SandboxError::Timeout)??;
        let docker = Docker::connect_with_defaults().map_err(|error| io::map_error(&error))?;
        let docker = io::request(config.operation_timeout, docker.negotiate_version()).await?;
        let (allocator, _) = super::pool::Allocator::rebuild(config.network_pool, []);
        let backend = Self {
            docker,
            config,
            admission: Semaphore::new(1),
            allocator: std::sync::Mutex::new(allocator),
            retry: crate::retry::Retry::default(),
        };
        backend.preflight().await?;
        if backend.config.network == super::NetworkMode::Internal {
            backend.prepare_networks().await?;
        }
        Ok(backend)
    }

    /// A backend over `docker` without connecting, preflight or network preparation, for tests with a fake engine.
    #[cfg(test)]
    pub(super) fn unconnected(docker: Docker, config: DockerBackendConfig) -> Self {
        let (allocator, _) = super::pool::Allocator::rebuild(config.network_pool, []);
        Self {
            docker,
            config,
            admission: Semaphore::new(1),
            allocator: std::sync::Mutex::new(allocator),
            retry: crate::retry::Retry::default(),
        }
    }

    /// Reports daemon capabilities without creating a container or changing host configuration.
    ///
    /// # Errors
    /// Returns `Preflight` for missing Linux, bridge or seccomp support, or transport errors.
    pub async fn preflight(&self) -> Result<Preflight, SandboxError> {
        let version = io::request(self.config.operation_timeout, self.docker.version()).await?;
        let info = io::request(self.config.operation_timeout, self.docker.info()).await?;
        let mut result = super::preflight::assess(version, info)?;
        if self.config.network == super::NetworkMode::DevHostGateway {
            result.enforceable_egress.clear();
        }
        Ok(result)
    }

    /// Checks for the configured immutable image without pulling or changing the daemon.
    ///
    /// # Errors
    /// Returns bounded transport failures; an absent image returns `false`.
    pub async fn image_present(&self, image: &crate::ImageRef) -> Result<bool, SandboxError> {
        match io::request(
            self.config.operation_timeout,
            self.docker.inspect_image(&image.to_string()),
        )
        .await
        {
            Ok(_) => Ok(true),
            Err(SandboxError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(super) async fn inspect_owned(
        &self,
        handle: &SandboxHandle,
    ) -> Result<ContainerInspectResponse, SandboxError> {
        if handle.kind != BackendKind::Docker {
            return Err(SandboxError::NotFound);
        }
        let inspected = io::request(
            self.config.operation_timeout,
            self.docker.inspect_container(&handle.id, None),
        )
        .await?;
        let labels = inspected
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .ok_or(SandboxError::NotFound)?;
        if labels.get("tqa.instance") != Some(&self.config.network_prefix)
            || labels.get("tqa.conversation") != Some(&handle.conversation_id.to_string())
        {
            return Err(SandboxError::NotFound);
        }
        Ok(inspected)
    }
}

#[async_trait]
impl SandboxBackend for DockerBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Docker
    }

    #[tracing::instrument(skip_all, fields(conversation_id = %spec.conversation_id))]
    async fn create(
        &self,
        spec: SandboxSpec,
        cancel: CancellationToken,
    ) -> Result<SandboxHandle, SandboxError> {
        Box::pin(self.create_sandbox(spec, &cancel)).await
    }

    async fn address(&self, handle: &SandboxHandle) -> Result<std::net::IpAddr, SandboxError> {
        let inspected = self.inspect_owned(handle).await?;
        if inspected.state.and_then(|state| state.running) != Some(true) {
            return Err(SandboxError::NotFound);
        }
        let address = inspected
            .network_settings
            .and_then(|settings| settings.networks)
            .and_then(|mut networks| {
                // A per-sandbox network is named on the container; the development mode shares the prefix network.
                let network = inspected
                    .config
                    .as_ref()
                    .and_then(|config| config.labels.as_ref())
                    .and_then(|labels| labels.get(super::sandbox_network::NETWORK_LABEL))
                    .cloned()
                    .unwrap_or_else(|| self.config.network_prefix.clone());
                networks.remove(&network)
            })
            .and_then(|network| network.ip_address)
            .and_then(|address| address.parse::<std::net::IpAddr>().ok())
            .filter(|address| !address.is_unspecified() && !address.is_multicast())
            .ok_or(SandboxError::Preflight(
                "sandbox has no internal network address",
            ))?;
        Ok(address)
    }

    #[tracing::instrument(skip_all, fields(conversation_id = %handle.conversation_id))]
    async fn wait_ready(
        &self,
        handle: &SandboxHandle,
        source: &dyn ReadinessSource,
        deadline: Duration,
        cancel: CancellationToken,
    ) -> Result<(), SandboxError> {
        readiness::poll(
            &handle.conversation_id,
            source,
            (deadline, self.config.readiness_interval),
            &cancel,
            &mut || {
                async { super::runtime_state::running(self.inspect_owned(handle).await?.state) }
                    .boxed()
            },
        )
        .await
    }

    #[tracing::instrument(skip_all, fields(conversation_id = %handle.conversation_id))]
    async fn stats(&self, handle: &SandboxHandle) -> Result<SandboxStats, SandboxError> {
        self.inspect_owned(handle).await?;
        let mut stream = self.docker.stats(
            &handle.id,
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        );
        let sample = tokio::time::timeout(self.config.operation_timeout, stream.next())
            .await
            .map_err(|_| SandboxError::Timeout)?
            .ok_or_else(|| SandboxError::Backend("missing stats sample".into()))?
            .map_err(|error| io::map_error(&error))?;
        records::stats(sample, handle.created_at.elapsed().unwrap_or_default())
    }

    #[tracing::instrument(skip_all, fields(conversation_id = %handle.conversation_id))]
    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.destroy_sandbox(handle).await
    }

    #[tracing::instrument(skip_all)]
    async fn list(&self) -> Result<Vec<SandboxHandle>, SandboxError> {
        let filters = HashMap::from([(
            "label".into(),
            vec![
                "tqa.conversation".into(),
                format!("tqa.instance={}", self.config.network_prefix),
            ],
        )]);
        let containers = io::request(
            self.config.operation_timeout,
            self.docker.list_containers(Some(ListContainersOptions {
                all: true,
                filters: Some(filters),
                ..Default::default()
            })),
        )
        .await?;
        if containers.len() > self.config.max_containers {
            return Err(SandboxError::Backend(
                "container listing exceeded configured bound".into(),
            ));
        }
        containers.into_iter().map(records::handle).collect()
    }

    async fn engine_networks(&self) -> Result<crate::EngineNetworks, SandboxError> {
        let networks = io::request(
            self.config.operation_timeout,
            self.docker
                .list_networks(None::<bollard::query_parameters::ListNetworksOptions>),
        )
        .await?;
        Ok(super::networks::engine_networks(&networks))
    }
}
