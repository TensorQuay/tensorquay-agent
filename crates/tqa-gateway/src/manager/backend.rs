use crate::{GatewayConfig, GatewayError};
use std::sync::Arc;
use tqa_sandbox::{
    SandboxBackend,
    docker::{DockerBackend, DockerBackendConfig},
    firecracker::FirecrackerBackend,
};

pub(crate) struct Backend {
    pub engine: Arc<dyn SandboxBackend>,
    pub docker: Option<Arc<DockerBackend>>,
    pub firecracker: Option<Arc<FirecrackerBackend>>,
}

impl Backend {
    pub fn injected(engine: Arc<dyn SandboxBackend>) -> Self {
        Self {
            engine,
            docker: None,
            firecracker: None,
        }
    }
    pub async fn connect(config: &GatewayConfig) -> Result<Self, GatewayError> {
        match config.backend {
            tqa_types::SandboxBackend::Docker => {}
            tqa_types::SandboxBackend::Firecracker => return Self::firecracker(config).await,
            _ => {
                return Err(GatewayError::Configuration(
                    "runtime backend is not implemented",
                ));
            }
        }
        let options = docker_options(config)?;
        let docker = Arc::new(
            DockerBackend::connect(options)
                .await
                .map_err(super::failure("configure"))?,
        );
        Ok(Self {
            engine: docker.clone(),
            docker: Some(docker),
            firecracker: None,
        })
    }

    /// The microVM backend (ADR-0006); it refuses to start unless its host preflight passes.
    async fn firecracker(config: &GatewayConfig) -> Result<Self, GatewayError> {
        let settings = config
            .firecracker
            .as_ref()
            .ok_or(GatewayError::Configuration(
                "runtime.backend = \"firecracker\" requires a [firecracker] section",
            ))?;
        if settings.workspace_bytes_set {
            // Named keys only; the value is the operator's and harmless, but the message is fixed text.
            tracing::warn!(
                "[firecracker] workspace_bytes is deprecated: new workspace images are sized from the workspace quota \
                 (workspaces.quota_bytes) plus ext4 metadata, and existing images keep their size"
            );
        }
        let mut options = settings.backend.clone();
        options.operation_timeout = config.io_timeout();
        options.create_budget = config.sandbox.create_budget();
        // A starved engine answering one call late must not stop the gateway for good: the connect is repeated on a
        // timeout only; a refusal still stops start-up at once (GW-U-071).
        // Each attempt's engine calls, its container creates included, end within what is left of the start-up budget.
        let backend = crate::startup::retry_timeouts(config.sandbox.startup_budget(), |left| {
            let mut options = options.clone();
            options.operation_timeout = options.operation_timeout.min(left);
            options.create_timeout = options.create_timeout.min(left);
            FirecrackerBackend::connect(options)
        })
        .await
        .map_err(startup_refusal)?;
        let backend = Arc::new(backend);
        Ok(Self {
            engine: backend.clone(),
            docker: None,
            firecracker: Some(backend),
        })
    }
}

/// The Docker backend's settings: operations bounded like every other gateway I/O, teardown by the backend's own
/// longer bound (the stop's grace plus a loaded daemon), so a slow removal is not reported as a failed retire.
///
/// # Errors
/// A configuration the backend refuses.
pub(super) fn docker_options(config: &GatewayConfig) -> Result<DockerBackendConfig, GatewayError> {
    let mut options = DockerBackendConfig::new(&config.sandbox.network, &config.storage_root)
        .map_err(super::failure("configure"))?;
    options.network = config.sandbox.network_mode;
    // A configuration from before per-sandbox networks has no container name; stop here, naming the key, not later
    // as an unexplained preflight failure (docs: upgrade notes).
    if options.network == tqa_sandbox::docker::NetworkMode::Internal
        && config.sandbox.gateway_container.is_none()
    {
        return Err(GatewayError::Configuration(
            "per-sandbox networks need [sandbox] gateway_container, the gateway's container name (compose: tqa-gateway)",
        ));
    }
    options
        .set_sandbox_network(
            config.sandbox.network_pool,
            config.sandbox.gateway_container.clone(),
        )
        .map_err(|_| GatewayError::Configuration("invalid sandbox gateway_container"))?;
    options.operation_timeout = config.io_timeout();
    options.create_budget = config.sandbox.create_budget();
    Ok(options)
}

/// How a backend connect that start-up gave up on is reported: an engine that never answered in time is named in
/// plain words, with what to do (GW-U-123); any other failure keeps its own cause.
fn startup_refusal(error: tqa_sandbox::SandboxError) -> GatewayError {
    match error {
        tqa_sandbox::SandboxError::Timeout => GatewayError::Sandbox {
            stage: "configure",
            cause: "the container engine did not answer in time while the gateway started; it is likely \
                    overloaded, so start the gateway again when the host is less busy",
        },
        other => super::failure("configure")(other),
    }
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
