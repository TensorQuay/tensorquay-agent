use crate::GatewayError;
use serde::Deserialize;
use std::{collections::BTreeMap, time::Duration};
use tqa_sandbox::{ByteSize, CpuLimit, ImageRef};
use url::Url;

/// One container create's own bound, in seconds: the least `create_budget_seconds` may be.
const ONE_CREATE_SECONDS: u64 = tqa_sandbox::docker::DEFAULT_CREATE_TIMEOUT.as_secs();
// The refusal names this bound in words.
const _: () = assert!(ONE_CREATE_SECONDS == 60);

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SandboxConfig {
    pub max_live: u32,
    pub idle_seconds: u32,
    pub ready_seconds: u32,
    /// What one start's engine work may take in all, its retries on a busy engine included: a person sees "Starting…"
    /// at most this long before a start fails (SBX-U-014, SBX-F-031). At least one container create's bound (60 s).
    pub create_budget_seconds: u32,
    /// How long start-up keeps trying a backend connect whose engine is too busy to answer, at least one full container
    /// create (GW-U-123); within the gateway image's health check (90 s) by default.
    pub startup_budget_seconds: u32,
    pub network: String,
    pub network_mode: tqa_sandbox::docker::NetworkMode,
    pub gateway_url: String,
    pub cpu_millicores: u32,
    pub memory_bytes: u64,
    pub pids: u32,
    pub env: BTreeMap<String, String>,
    /// How long a pinned sandbox may live; unpinned sandboxes retire on the idle window first.
    pub max_lifetime_seconds: u32,
    /// Where each Docker sandbox's own network gets its subnet (`[sandbox.network_pool]`, ADR-0021 amendment).
    pub network_pool: tqa_sandbox::docker::SubnetPool,
    /// The gateway's container name, attached to every sandbox network as `gateway`.
    pub gateway_container: Option<String>,
    /// The most a conversation's runtime state directory may hold on Docker before its run is stopped (N4, G1): the
    /// runtime's journal lives there, and tools share the runtime's user.
    pub state_bytes: u64,
    /// The installation's ceiling on every environment's size (`[sandbox.ceiling]`, file-only).
    pub ceiling: super::sizes::Ceiling,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_live: 4,
            idle_seconds: 600,
            ready_seconds: 30,
            create_budget_seconds: 120,
            startup_budget_seconds: u32::try_from(crate::startup::BACKEND_DEADLINE.as_secs())
                .unwrap_or(u32::MAX),
            network: "tqa-sandboxes".into(),
            network_mode: tqa_sandbox::docker::NetworkMode::Internal,
            gateway_url: "http://gateway:8080".into(),
            cpu_millicores: 1000,
            memory_bytes: 512 * 1024 * 1024,
            pids: 256,
            env: BTreeMap::new(),
            max_lifetime_seconds: 28_800,
            network_pool: tqa_sandbox::docker::SubnetPool::default(),
            gateway_container: None,
            state_bytes: 1 << 30,
            ceiling: super::sizes::Ceiling::default(),
        }
    }
}

impl SandboxConfig {
    /// The defaults an environment without its own size gets must fit the ceiling, and a microVM has at most 32 vCPUs.
    fn defaults_within_ceiling(
        &self,
        backend: tqa_types::SandboxBackend,
    ) -> Result<(), GatewayError> {
        let ceiling = &self.ceiling;
        if self.cpu_millicores > ceiling.cpu_millicores()
            || self.memory_bytes > ceiling.memory_bytes()
            || self.pids > ceiling.pids()
        {
            return Err(GatewayError::Configuration(
                "the sandbox defaults (cpu_millicores, memory_bytes, pids) must be within [sandbox.ceiling]",
            ));
        }
        if backend == tqa_types::SandboxBackend::Firecracker && ceiling.cpu_millicores() > 32_000 {
            return Err(GatewayError::Configuration(
                "[sandbox.ceiling] cpus is at most 32 on microVMs",
            ));
        }
        Ok(())
    }

    /// # Errors
    /// Bounds outside their ranges, and a subnet pool too small for `max_live` when Docker sandboxes each get their
    /// own network (the Docker backend in `Internal` mode; Firecracker and the development mode don't use the pool).
    pub(super) fn validate(&self, backend: tqa_types::SandboxBackend) -> Result<(), GatewayError> {
        if !(1..=4096).contains(&self.max_live)
            || !(1..=86_400).contains(&self.idle_seconds)
            || !(1..=30).contains(&self.ready_seconds)
            || !(60..=7 * 86_400).contains(&self.max_lifetime_seconds)
        {
            return Err(GatewayError::Configuration(
                "sandbox limits exceed supported bounds",
            ));
        }
        // At least one container create's own bound: a shorter budget cuts even the first create short.
        if !(ONE_CREATE_SECONDS..=600).contains(&u64::from(self.create_budget_seconds)) {
            return Err(GatewayError::Configuration(
                "sandbox.create_budget_seconds must be 60 (one container create's bound) through 600",
            ));
        }
        if !(60..=600).contains(&self.startup_budget_seconds) {
            return Err(GatewayError::Configuration(
                "sandbox.startup_budget_seconds must be 60 through 600",
            ));
        }
        if !((16 << 20)..=(1 << 40)).contains(&self.state_bytes) {
            return Err(GatewayError::Configuration(
                "sandbox.state_bytes must be 16 MiB through 1 TiB",
            ));
        }
        let mut config = tqa_sandbox::docker::DockerBackendConfig::new(
            &self.network,
            std::env::temp_dir().join("tqa-config-validation"),
        )
        .map_err(|_| GatewayError::Configuration("invalid sandbox network"))?;
        config
            .set_sandbox_network(self.network_pool, self.gateway_container.clone())
            .map_err(|_| GatewayError::Configuration("invalid sandbox gateway_container"))?;
        if backend == tqa_types::SandboxBackend::Docker
            && self.network_mode == tqa_sandbox::docker::NetworkMode::Internal
        {
            self.network_pool
                .check_capacity(self.max_live)
                .map_err(|_| {
                    GatewayError::Configuration(
                        "sandbox network_pool holds fewer than twice max_live subnets",
                    )
                })?;
        }
        CpuLimit::new(self.cpu_millicores)
            .map_err(|_| GatewayError::Configuration("invalid sandbox CPU limit"))?;
        ByteSize::new(self.memory_bytes)
            .map_err(|_| GatewayError::Configuration("invalid sandbox memory limit"))?;
        if !(1..=65_536).contains(&self.pids) || self.env.len() > 64 {
            return Err(GatewayError::Configuration(
                "invalid sandbox process or environment bounds",
            ));
        }
        self.defaults_within_ceiling(backend)?;
        let endpoint = Url::parse(&self.gateway_url)
            .map_err(|_| GatewayError::Configuration("invalid sandbox gateway URL"))?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.path() != "/"
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.as_str().len() > 2048
        {
            return Err(GatewayError::Configuration("invalid sandbox gateway URL"));
        }
        self.validate_environment()
    }

    fn validate_environment(&self) -> Result<(), GatewayError> {
        if self
            .env
            .iter()
            .all(|(key, value)| environment_entry_ok(key, value))
        {
            Ok(())
        } else {
            Err(GatewayError::Configuration(
                "invalid, reserved or secret-looking sandbox environment",
            ))
        }
    }
    pub fn ready(&self) -> Duration {
        Duration::from_secs(u64::from(self.ready_seconds))
    }

    /// How long start-up keeps trying the backend's connect.
    pub fn startup_budget(&self) -> Duration {
        Duration::from_secs(u64::from(self.startup_budget_seconds))
    }

    /// The budget one start's engine work shares, retries included.
    pub fn create_budget(&self) -> Duration {
        Duration::from_secs(u64::from(self.create_budget_seconds))
    }
}

pub(super) fn image(value: &str) -> Result<ImageRef, GatewayError> {
    if !value.contains('@') && !value.starts_with("sha256:") {
        return Err(GatewayError::Configuration(
            "runtime image requires a digest",
        ));
    }
    value
        .parse()
        .map_err(|_| GatewayError::Configuration("invalid runtime image digest"))
}

/// One environment entry a sandbox process may be given: a portable name that is neither reserved (`TQA_*`, the proxy
/// variables) nor secret-looking, and a bounded value without NUL. Shared by `[sandbox.env]` and `[[mcp.servers]] env`.
pub(super) fn environment_entry_ok(key: &str, value: &str) -> bool {
    let name = key.to_ascii_lowercase();
    let reserved = name.starts_with("tqa_")
        || ["http_proxy", "https_proxy", "no_proxy", "all_proxy"].contains(&name.as_str());
    let secret = ["key", "token", "secret", "password", "credential"]
        .iter()
        .any(|part| name.contains(part));
    !(key.is_empty()
        || key.len() > 128
        || !key.starts_with(|character: char| character.is_ascii_alphabetic() || character == '_')
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || reserved
        || secret
        || value.len() > 8192
        || value.contains('\0'))
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
mod tests;
