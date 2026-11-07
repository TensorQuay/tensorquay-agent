use crate::{EgressMode, SandboxError};
use bollard::models::{SystemInfo, SystemVersion};

/// Daemon platform, distinguished from the client operating system.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DockerPlatform {
    /// A native Linux Engine installation.
    LinuxEngine,
    /// Linux containers inside the Docker Desktop virtual machine.
    DockerDesktop,
}

/// Daemon default; every sandbox separately requests the built-in profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeccompStatus {
    /// The daemon advertises its built-in default profile.
    Builtin,
    /// The daemon has filtering support but defaults to unconfined.
    Unconfined,
    /// A custom profile is advertised without exposing its host path.
    Custom,
}

/// Capability observations for diagnostics; upstream proxy policy belongs to the gateway.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preflight {
    /// Maximum Docker API version advertised by the daemon.
    pub api_version: String,
    /// Server kernel family; only Linux is supported.
    pub server_os: String,
    /// Native Engine or Desktop virtual machine.
    pub platform: DockerPlatform,
    /// Whether the daemon advertises the bridge driver needed for internal networks.
    pub internal_networks: bool,
    /// Whether daemon user namespace remapping is enabled, not merely kernel-supported.
    pub user_namespaces: bool,
    /// Advertised daemon default seccomp configuration.
    pub default_seccomp: SeccompStatus,
    /// Modes whose direct routing isolation is enforced by this backend.
    pub enforceable_egress: Vec<EgressMode>,
}

pub(super) fn assess(version: SystemVersion, info: SystemInfo) -> Result<Preflight, SandboxError> {
    let server_os = version
        .os
        .filter(|os| os == "linux")
        .ok_or(SandboxError::Preflight("Linux containers are required"))?;
    let api_version = version
        .api_version
        .filter(|value| value.len() <= 32)
        .ok_or(SandboxError::Preflight("Docker API version is missing"))?;
    let internal_networks = info
        .plugins
        .and_then(|plugins| plugins.network)
        .is_some_and(|networks| networks.iter().any(|name| name == "bridge"));
    if !internal_networks {
        return Err(SandboxError::Preflight(
            "the bridge network driver is required",
        ));
    }
    let security = info.security_options.unwrap_or_default();
    let seccomp = security
        .iter()
        .find(|option| option.starts_with("name=seccomp"))
        .ok_or(SandboxError::Preflight("seccomp support is required"))?;
    let default_seccomp = if seccomp.contains("profile=unconfined") {
        SeccompStatus::Unconfined
    } else if seccomp.contains("profile=builtin") {
        SeccompStatus::Builtin
    } else {
        SeccompStatus::Custom
    };
    let desktop = info
        .operating_system
        .is_some_and(|os| os.contains("Docker Desktop"));
    Ok(Preflight {
        api_version,
        server_os,
        platform: if desktop {
            DockerPlatform::DockerDesktop
        } else {
            DockerPlatform::LinuxEngine
        },
        internal_networks,
        user_namespaces: security.iter().any(|option| option == "name=userns"),
        default_seccomp,
        enforceable_egress: vec![EgressMode::Internal, EgressMode::Proxy],
    })
}
