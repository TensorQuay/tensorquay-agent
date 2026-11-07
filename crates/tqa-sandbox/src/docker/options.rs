use std::{
    path::{Component, PathBuf},
    time::Duration,
};

use super::{config::validate_network_name, pool::SubnetPool};
use crate::SandboxError;
use serde::{Deserialize, Serialize};

/// Network isolation is independent of whether a sandbox is allowed to use a proxy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    /// Internal bridge with no direct external routing; the production default.
    #[default]
    Internal,
    /// Development-only bridge, requiring `TQA_ALLOW_DEV_HOST_GATEWAY=1` at construction.
    DevHostGateway,
}

/// Validated backend bounds. Deserialize strictly, then validate when connecting.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DockerBackendConfig {
    /// Unique gateway instance network name, 1 through 63 ASCII characters.
    pub network_prefix: String,
    /// Production isolation by default; the development exception requires an explicit opt-in.
    #[serde(default)]
    pub network: NetworkMode,
    /// Existing storage directory; private credential files live under its sandboxes child.
    pub storage_root: PathBuf,
    /// Per-operation timeout, default 30 seconds; also bounds mutation settling.
    pub operation_timeout: Duration,
    /// Bound on each call that tears a sandbox down (the stop with its 10-second grace, the forced removal), default 30
    /// seconds. Kept apart from `operation_timeout` so an embedder that shortens operations does not cut removals short
    /// on a loaded daemon, where a removal that times out is reported as failed although it completes.
    #[serde(default = "default_destroy_timeout")]
    pub destroy_timeout: Duration,
    /// Bound on each call that sets a sandbox up (creating and starting its container, creating its network and
    /// attaching the gateway), default 60 seconds. Kept apart from `operation_timeout`: a loaded daemon can take longer
    /// than an ordinary operation to create a network namespace, mounts and a veth, and a create cut short fails the
    /// run although the daemon was only slow.
    #[serde(default = "default_create_timeout")]
    pub create_timeout: Duration,
    /// What one start's container create and start may take in all, its retries included, default 120 seconds
    /// (`[sandbox] create_budget_seconds`, SBX-U-014): a busy daemon's timed-out create is retried within it, each
    /// attempt bounded by `create_timeout` cut to what is left.
    #[serde(default = "default_create_budget")]
    pub create_budget: Duration,
    /// Image pull timeout, default 120 seconds.
    pub pull_timeout: Duration,
    /// Registration polling interval, default 100 milliseconds.
    pub readiness_interval: Duration,
    /// Maximum containers returned by reconciliation, default 4096.
    pub max_containers: usize,
    /// Test-only replacement for the image command. The gateway must leave this unset.
    pub test_command_override: Option<Vec<String>>,
    /// Where each sandbox's own network gets its subnet (ADR-0021 amendment).
    #[serde(default)]
    pub network_pool: SubnetPool,
    /// The gateway's container, by name, attached to every sandbox's network as `gateway`; required for `Internal`.
    #[serde(default)]
    pub gateway_container: Option<String>,
}

/// The default bound on each teardown call: the stop's 10-second grace with room for a loaded daemon.
pub const DEFAULT_DESTROY_TIMEOUT: Duration = Duration::from_secs(30);

/// The default bound on each call that sets a sandbox up.
pub const DEFAULT_CREATE_TIMEOUT: Duration = Duration::from_mins(1);

const fn default_create_timeout() -> Duration {
    DEFAULT_CREATE_TIMEOUT
}

const fn default_create_budget() -> Duration {
    crate::DEFAULT_CREATE_BUDGET
}

const fn default_destroy_timeout() -> Duration {
    DEFAULT_DESTROY_TIMEOUT
}

impl DockerBackendConfig {
    /// Creates a configuration with documented bounds and no command override.
    ///
    /// # Errors
    /// Returns `InvalidSpec` for a reserved or invalid instance network name.
    pub fn new(
        network_prefix: impl Into<String>,
        storage_root: impl Into<PathBuf>,
    ) -> Result<Self, SandboxError> {
        Self::with_defaults(network_prefix.into(), storage_root.into())
    }

    fn with_defaults(network_prefix: String, storage_root: PathBuf) -> Result<Self, SandboxError> {
        let config = Self {
            network_prefix,
            network: NetworkMode::Internal,
            storage_root,
            operation_timeout: Duration::from_secs(30),
            destroy_timeout: DEFAULT_DESTROY_TIMEOUT,
            create_timeout: DEFAULT_CREATE_TIMEOUT,
            create_budget: crate::DEFAULT_CREATE_BUDGET,
            pull_timeout: Duration::from_mins(2),
            readiness_interval: Duration::from_millis(100),
            max_containers: 4096,
            test_command_override: None,
            network_pool: SubnetPool::default(),
            gateway_container: None,
        };
        config.validate()?;
        Ok(config)
    }

    /// Sets where sandbox networks get their subnets and which container is the gateway.
    ///
    /// # Errors
    /// `InvalidSpec` for a malformed container name.
    pub fn set_sandbox_network(
        &mut self,
        network_pool: SubnetPool,
        gateway_container: Option<String>,
    ) -> Result<(), SandboxError> {
        if let Some(name) = &gateway_container {
            validate_container_name(name)?;
        }
        self.network_pool = network_pool;
        self.gateway_container = gateway_container;
        Ok(())
    }

    pub(super) fn check_network_opt_in(&self, value: Option<&str>) -> Result<(), SandboxError> {
        if self.network == NetworkMode::DevHostGateway && value != Some("1") {
            return Err(SandboxError::Preflight(
                "development host gateway requires TQA_ALLOW_DEV_HOST_GATEWAY=1",
            ));
        }
        Ok(())
    }

    pub(super) fn validate(&self) -> Result<(), SandboxError> {
        validate_network_name(&self.network_prefix)?;
        if !self.storage_root.is_absolute()
            || self.storage_root.parent().is_none()
            || self.storage_root.to_str().is_none()
            || self
                .storage_root
                .components()
                .any(|part| part == Component::ParentDir)
        {
            return Err(SandboxError::invalid(
                "storage_root",
                "expected an absolute non-root storage directory without traversal",
            ));
        }
        for duration in [
            self.operation_timeout,
            self.destroy_timeout,
            self.create_timeout,
            self.create_budget,
            self.pull_timeout,
            self.readiness_interval,
        ] {
            if duration.is_zero() || duration > Duration::from_mins(10) {
                return Err(SandboxError::invalid(
                    "docker",
                    "timeouts must be positive and at most ten minutes",
                ));
            }
        }
        if !(1..=65_536).contains(&self.max_containers) {
            return Err(SandboxError::invalid(
                "docker",
                "container listing bound must be 1 through 65536",
            ));
        }
        if let Some(command) = &self.test_command_override
            && (command.is_empty()
                || command.len() > 32
                || command
                    .iter()
                    .any(|part| part.len() > 8192 || part.contains('\0'))
                || command[0].is_empty())
        {
            return Err(SandboxError::invalid(
                "docker",
                "invalid test command override",
            ));
        }
        if let Some(name) = &self.gateway_container {
            validate_container_name(name)?;
        }
        Ok(())
    }
}

/// A Docker container name as the engine accepts it, bounded: a letter or digit, then letters, digits, `_`, `.` or `-`.
pub(super) fn validate_container_name(name: &str) -> Result<(), SandboxError> {
    let bytes = name.as_bytes();
    if !(2..=128).contains(&bytes.len())
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(byte))
    {
        return Err(SandboxError::invalid(
            "gateway_container",
            "expected a container name of 2 through 128 letters, digits, '_', '.' or '-', starting with a letter or digit",
        ));
    }
    Ok(())
}
