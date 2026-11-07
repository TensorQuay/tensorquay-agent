//! Host preflight (SBX-F-001): the backend refuses to start without the VM host image, KVM access for the VMM uid,
//! the tun device, ACL-capable storage and a guest kernel matching its manifest. Each refusal names what is missing.

use async_trait::async_trait;

use super::{
    FirecrackerBackendConfig, acl,
    engine::NetworkState,
    manifest::{self, Verified},
};
use crate::SandboxError;

/// Host facts that need the container engine; the Docker-backed implementation arrives with the backend.
#[async_trait]
pub trait HostProbe: Send + Sync {
    /// Whether the pinned VM host image is present locally.
    async fn vmm_image_present(&self) -> Result<bool, SandboxError>;
    /// Whether the sandbox network exists and is internal.
    async fn sandbox_network(&self) -> Result<NetworkState, SandboxError>;
    /// Whether `/dev/kvm` opens read-write for the VMM uid inside a short-lived VM host container. An access check is
    /// not enough: a device passed into a rootless container appears readable without being openable.
    async fn kvm_opens_for_vmm(&self) -> Result<bool, SandboxError>;
    /// Whether `/dev/net/tun` is available to the VM host container.
    async fn tun_available(&self) -> Result<bool, SandboxError>;
}

/// What a passed preflight established.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preflight {
    /// The verified guest kernel's release.
    pub kernel_release: String,
}

/// Runs every check in order and stops at the first failure; later probes (which start containers from the VM host
/// image) never run when an earlier check failed.
///
/// # Errors
/// Returns `InvalidSpec` for an invalid configuration, otherwise `Preflight` with a static, actionable reason.
pub async fn run(
    config: &FirecrackerBackendConfig,
    probe: &dyn HostProbe,
) -> Result<Preflight, SandboxError> {
    config.validate()?;
    require(
        probe.vmm_image_present().await?,
        "the VM host image is not present (build images/vmm)",
    )?;
    match probe.sandbox_network().await? {
        NetworkState::Internal => {}
        NetworkState::Missing => {
            return Err(SandboxError::Preflight(
                "the sandbox network does not exist (start the gateway on its internal network first)",
            ));
        }
        NetworkState::NotInternal => {
            return Err(SandboxError::Preflight(
                "the sandbox network is not internal, so it cannot confine a guest",
            ));
        }
    }
    require(
        probe.kvm_opens_for_vmm().await?,
        "/dev/kvm does not open for the VMM uid (run deploy/firecracker/host-setup.sh --check)",
    )?;
    require(
        probe.tun_available().await?,
        "/dev/net/tun is not available to the VM host container",
    )?;
    let (root, uid) = (config.storage_root.clone(), config.vmm_file_uid);
    let kernel_dir = config.kernel_dir.clone();
    let kernel = tokio::time::timeout(
        config.operation_timeout,
        tokio::task::spawn_blocking(move || {
            acl::probe(&root, uid)?;
            manifest::kernel(&kernel_dir, &Verified::default())
        }),
    )
    .await
    .map_err(|_| SandboxError::Timeout)?
    .map_err(|_| SandboxError::Backend("the preflight worker stopped".into()))??;
    Ok(Preflight {
        kernel_release: kernel.release,
    })
}

fn require(present: bool, reason: &'static str) -> Result<(), SandboxError> {
    if present {
        Ok(())
    } else {
        Err(SandboxError::Preflight(reason))
    }
}

#[cfg(test)]
#[path = "preflight_tests.rs"]
mod tests;
