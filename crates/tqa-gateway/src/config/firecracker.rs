//! The `[firecracker]` section (ADR-0006): required exactly when `[runtime] backend = "firecracker"`, refused otherwise.
//! It is turned into the backend's own validated configuration at load, so a bad value fails at startup.

use std::{
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tqa_sandbox::firecracker::FirecrackerBackendConfig;
use tqa_types::SandboxBackend;
use url::Url;

use super::{SandboxConfig, sandbox};
use crate::GatewayError;

const MIN_WORKSPACE_BYTES: u64 = 64 << 20;
const MAX_WORKSPACE_BYTES: u64 = 2 << 40;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FirecrackerInput {
    /// The VM host image (images/vmm), pinned like `runtime.image`.
    vmm_image: String,
    /// Default `storage.root/kernel`.
    kernel_dir: Option<PathBuf>,
    /// Default `storage.root/images`.
    rootfs_dir: Option<PathBuf>,
    /// The gateway's pinned address on `[sandbox] network`.
    gateway_address: Ipv4Addr,
    /// Default: the port of `[sandbox] gateway_url` and, when egress is configured, the policy proxy's port.
    gateway_ports: Option<Vec<u16>>,
    /// Deprecated (SBX-F-025): a new workspace image is sized from the workspace quota plus ext4 metadata. Still
    /// accepted and validated; setting it logs a warning at start-up.
    #[serde(default)]
    workspace_bytes: Option<u64>,
    /// The VMM's uid as the gateway sees it; default 10000, the VMM's container uid on the same rootless engine.
    vmm_file_uid: Option<u32>,
    /// The VM host's memory above the guest's, for the VMM and the host page cache of the guest's disk writes; default
    /// 512 MiB, 64 MiB through 8 GiB.
    vmm_overhead_bytes: Option<u64>,
}

/// Validated Firecracker settings.
#[derive(Clone, Debug)]
pub(crate) struct FirecrackerSettings {
    /// The backend configuration; the gateway sets only the operation timeout when connecting.
    pub(crate) backend: FirecrackerBackendConfig,
    /// Whether the file sets the deprecated `workspace_bytes`, which start-up warns about.
    pub(crate) workspace_bytes_set: bool,
}

/// The sandbox and egress facts the section's defaults derive from.
pub(super) struct Context<'a> {
    pub(super) backend: SandboxBackend,
    pub(super) storage_root: &'a Path,
    pub(super) sandbox: &'a SandboxConfig,
    pub(super) egress_listen: Option<SocketAddr>,
}

pub(super) fn firecracker(
    input: Option<FirecrackerInput>,
    context: &Context<'_>,
) -> Result<Option<Box<FirecrackerSettings>>, GatewayError> {
    let invalid = GatewayError::Configuration;
    let input = match (context.backend, input) {
        (SandboxBackend::Firecracker, Some(input)) => input,
        (SandboxBackend::Firecracker, None) => {
            return Err(invalid(
                "runtime.backend = \"firecracker\" requires a [firecracker] section",
            ));
        }
        (_, Some(_)) => {
            return Err(invalid(
                "[firecracker] applies only to runtime.backend = \"firecracker\"",
            ));
        }
        (_, None) => return Ok(None),
    };
    let gateway_url = Url::parse(&context.sandbox.gateway_url)
        .map_err(|_| invalid("invalid sandbox gateway URL"))?;
    if gateway_url.domain().is_none() {
        return Err(invalid(
            "the Firecracker backend needs a gateway host name in [sandbox] gateway_url",
        ));
    }
    let ports = match input.gateway_ports {
        Some(ports) => ports,
        None => default_ports(&gateway_url, context.egress_listen)?,
    };
    let mut backend = FirecrackerBackendConfig::new(
        context.storage_root.to_path_buf(),
        sandbox::image(&input.vmm_image)?,
        context.sandbox.network.clone(),
        ports,
    )
    .map_err(|_| invalid("invalid [firecracker] settings"))?;
    if let Some(directory) = input.kernel_dir {
        backend.kernel_dir = directory;
    }
    if let Some(directory) = input.rootfs_dir {
        backend.rootfs_dir = directory;
    }
    backend.gateway_address = input.gateway_address;
    if let Some(uid) = input.vmm_file_uid {
        backend.vmm_file_uid = uid;
    }
    if let Some(bytes) = input.vmm_overhead_bytes {
        backend.vmm_overhead =
            tqa_sandbox::ByteSize::new(bytes).map_err(|_| firecracker_field("vmm_overhead"))?;
    }
    backend.validate().map_err(|error| match error {
        tqa_sandbox::SandboxError::InvalidSpec { field, .. } => firecracker_field(field),
        _ => invalid("invalid [firecracker] settings"),
    })?;
    // Deprecated and unused (the workspace quota sizes a new image), but a value outside its old range is still an
    // error rather than silently ignored.
    if input
        .workspace_bytes
        .is_some_and(|bytes| !(MIN_WORKSPACE_BYTES..=MAX_WORKSPACE_BYTES).contains(&bytes))
    {
        return Err(invalid(
            "[firecracker] workspace_bytes must be 64 MiB through 2 TiB",
        ));
    }
    Ok(Some(Box::new(FirecrackerSettings {
        backend,
        workspace_bytes_set: input.workspace_bytes.is_some(),
    })))
}

fn default_ports(
    gateway_url: &Url,
    egress_listen: Option<SocketAddr>,
) -> Result<Vec<u16>, GatewayError> {
    let link = gateway_url
        .port_or_known_default()
        .ok_or(GatewayError::Configuration(
            "the sandbox gateway URL has no port",
        ))?;
    let mut ports = vec![link];
    if let Some(proxy) = egress_listen.map(|listen| listen.port())
        && proxy != link
    {
        ports.push(proxy);
    }
    Ok(ports)
}

/// A static message per backend field, so a configuration error names what to fix.
fn firecracker_field(field: &str) -> GatewayError {
    GatewayError::Configuration(match field {
        "storage_root" | "kernel_dir" | "rootfs_dir" => {
            "[firecracker] directories must be absolute paths without traversal"
        }
        "network" => "invalid sandbox network",
        "vmm_overhead" => "[firecracker] vmm_overhead_bytes must be 64 MiB through 8 GiB",
        "gateway_address" => {
            "[firecracker] gateway_address must be the gateway's unicast address on the sandbox network"
        }
        "gateway_ports" => {
            "[firecracker] gateway_ports must be 1 through 8 distinct non-zero ports"
        }
        "vmm_file_uid" => "[firecracker] vmm_file_uid must not be 0",
        _ => "invalid [firecracker] settings",
    })
}

#[cfg(test)]
#[path = "firecracker_tests.rs"]
mod tests;
