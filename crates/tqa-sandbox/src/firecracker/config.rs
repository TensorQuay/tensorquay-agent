use std::{
    net::Ipv4Addr,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use crate::{ByteSize, ImageRef, SandboxError};

/// Firecracker's uid inside the per-VM container (ADR-0006); never the container's root.
pub const VMM_CONTAINER_UID: u32 = 10_000;
const MAX_PORTS: usize = 8;
const MAX_TIMEOUT: Duration = Duration::from_mins(10);

/// The VM host's default memory above the guest's. The guest's disk writes pass through the host page cache, charged to
/// the VM host's cgroup, and while the guest writes fast up to about 280 MiB of it is dirty. With 64 MiB the cgroup sat
/// at its limit with no clean cache to reclaim and the VMM was OOM-killed mid-write when writeback lagged (G-13-full,
/// 28 Sep 2026); 256 MiB left 17 MiB of clean cache at the limit, 512 MiB left 228 MiB.
pub const DEFAULT_VMM_OVERHEAD: u64 = 512 << 20;
const MIN_VMM_OVERHEAD: u64 = 64 << 20;
const MAX_VMM_OVERHEAD: u64 = 8 << 30;

/// Validated backend settings. Construct with [`FirecrackerBackendConfig::new`], adjust fields, then `validate`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirecrackerBackendConfig {
    /// Existing storage directory; per-sandbox files live under its `sandboxes` child.
    pub storage_root: PathBuf,
    /// The VM host image (`images/vmm`), pinned by digest.
    pub vmm_image: ImageRef,
    /// Directory holding `vmlinux` and its `manifest.json`; default `storage_root/kernel`.
    pub kernel_dir: PathBuf,
    /// Directory holding one `<image id>/` rootfs per template; default `storage_root/images`.
    pub rootfs_dir: PathBuf,
    /// The gateway instance's internal network, shared with Docker mode.
    pub network: String,
    /// The gateway's address on the internal network; the only address a guest can reach. Deployments pin it, so a
    /// gateway restart cannot strand running VMs behind stale rules. Must be set; the default is refused.
    pub gateway_address: Ipv4Addr,
    /// Gateway TCP ports a guest may reach (runtime link, relay, MCP proxy, policy proxy); 1 through 8, distinct.
    pub gateway_ports: Vec<u16>,
    /// The VMM uid as this process sees it: [`VMM_CONTAINER_UID`] when the backend runs in a container on the same
    /// rootless engine, the mapped subordinate uid when it runs on the host. Granted access to the per-VM files.
    pub vmm_file_uid: u32,
    /// The VM host's memory on top of the guest's, default [`DEFAULT_VMM_OVERHEAD`], 64 MiB through 8 GiB. It holds the
    /// VMM process and the host page cache the guest's disk writes go through, which the same cgroup is charged for.
    pub vmm_overhead: ByteSize,
    /// Size of a new state image, default 256 MiB.
    pub state_image: ByteSize,
    /// Per-operation timeout, default 30 seconds.
    pub operation_timeout: Duration,
    /// Bound on each container create and helper run (the VM host, image helpers, start-up probes), default 60
    /// seconds as on Docker: a loaded engine can take far longer to create a container than to answer an ordinary
    /// call (SBX-F-032).
    pub create_timeout: Duration,
    /// What one start's retried engine calls (its images' helpers and its VM host) may take in all, default 120
    /// seconds (`[sandbox] create_budget_seconds`, SBX-F-031).
    pub create_budget: Duration,
    /// Registration polling interval, default 100 milliseconds.
    pub readiness_interval: Duration,
}

impl FirecrackerBackendConfig {
    /// Settings with the documented defaults and artefact directories under `storage_root`.
    ///
    /// # Errors
    /// Returns `InvalidSpec` if the defaults cannot be represented.
    pub fn new(
        storage_root: PathBuf,
        vmm_image: ImageRef,
        network: String,
        gateway_ports: Vec<u16>,
    ) -> Result<Self, SandboxError> {
        Ok(Self {
            kernel_dir: storage_root.join("kernel"),
            rootfs_dir: storage_root.join("images"),
            storage_root,
            vmm_image,
            network,
            gateway_address: Ipv4Addr::UNSPECIFIED,
            gateway_ports,
            vmm_file_uid: VMM_CONTAINER_UID,
            vmm_overhead: ByteSize::new(DEFAULT_VMM_OVERHEAD)?,
            state_image: ByteSize::new(256 << 20)?,
            operation_timeout: Duration::from_secs(30),
            create_timeout: crate::docker::DEFAULT_CREATE_TIMEOUT,
            create_budget: crate::DEFAULT_CREATE_BUDGET,
            readiness_interval: Duration::from_millis(100),
        })
    }

    /// Checks every field without I/O.
    ///
    /// # Errors
    /// Returns `InvalidSpec` naming the first invalid field.
    pub fn validate(&self) -> Result<(), SandboxError> {
        if !(MIN_VMM_OVERHEAD..=MAX_VMM_OVERHEAD).contains(&self.vmm_overhead.bytes()) {
            return Err(SandboxError::invalid(
                "vmm_overhead",
                "expected 64 MiB through 8 GiB",
            ));
        }
        for (field, path) in [
            ("storage_root", &self.storage_root),
            ("kernel_dir", &self.kernel_dir),
            ("rootfs_dir", &self.rootfs_dir),
        ] {
            if !plain_absolute(path) {
                return Err(SandboxError::invalid(
                    field,
                    "expected an absolute path without traversal",
                ));
            }
        }
        if !valid_network(&self.network) {
            return Err(SandboxError::invalid(
                "network",
                "expected a dedicated network name",
            ));
        }
        let mut ports = self.gateway_ports.clone();
        ports.sort_unstable();
        ports.dedup();
        if ports.is_empty()
            || ports.len() > MAX_PORTS
            || ports.len() != self.gateway_ports.len()
            || ports[0] == 0
        {
            return Err(SandboxError::invalid(
                "gateway_ports",
                "expected 1 through 8 distinct non-zero ports",
            ));
        }
        let gateway = self.gateway_address;
        if gateway.is_unspecified()
            || gateway.is_loopback()
            || gateway.is_multicast()
            || gateway.is_broadcast()
        {
            return Err(SandboxError::invalid(
                "gateway_address",
                "expected the gateway's unicast address on the internal network",
            ));
        }
        if self.vmm_file_uid == 0 {
            return Err(SandboxError::invalid(
                "vmm_file_uid",
                "the VMM must not run as root",
            ));
        }
        if self.operation_timeout.is_zero() || self.operation_timeout > MAX_TIMEOUT {
            return Err(SandboxError::invalid(
                "operation_timeout",
                "expected at most 10 minutes",
            ));
        }
        if self.create_timeout.is_zero() || self.create_timeout > MAX_TIMEOUT {
            return Err(SandboxError::invalid(
                "create_timeout",
                "expected at most 10 minutes",
            ));
        }
        if self.create_budget.is_zero() || self.create_budget > MAX_TIMEOUT {
            return Err(SandboxError::invalid(
                "create_budget",
                "expected a positive budget of at most 10 minutes",
            ));
        }
        if self.readiness_interval.is_zero() || self.readiness_interval > self.operation_timeout {
            return Err(SandboxError::invalid(
                "readiness_interval",
                "expected a positive interval no longer than the operation timeout",
            ));
        }
        Ok(())
    }
}

fn plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.parent().is_some()
        && path.to_str().is_some_and(|text| text.len() <= 4096)
        && !path.components().any(|part| part == Component::ParentDir)
}

/// The same rule as the Docker backend's network names.
fn valid_network(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && !["host", "none", "bridge", "default"].contains(&name)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::{error::Error, time::Duration};

    use super::{FirecrackerBackendConfig, VMM_CONTAINER_UID};
    use crate::{SandboxError, firecracker::test_support::config};

    type Mutation = fn(&mut FirecrackerBackendConfig);

    #[test]
    fn sbx_f_013_configuration_defaults_and_bounds() -> Result<(), Box<dyn Error>> {
        let valid = config()?;
        valid.validate()?;
        assert_eq!(valid.kernel_dir, std::path::Path::new("/srv/tqa/kernel"));
        assert_eq!(valid.vmm_file_uid, VMM_CONTAINER_UID);
        let cases: [(&str, Mutation); 14] = [
            ("storage_root", |c| {
                c.storage_root = "relative".into();
            }),
            ("kernel_dir", |c| {
                c.kernel_dir = "/srv/../etc".into();
            }),
            ("network", |c| {
                c.network = "host".into();
            }),
            ("gateway_ports", |c| {
                c.gateway_ports = vec![];
            }),
            ("gateway_ports", |c| {
                c.gateway_ports = vec![80, 80];
            }),
            ("gateway_ports", |c| {
                c.gateway_ports = vec![0];
            }),
            ("vmm_file_uid", |c| {
                c.vmm_file_uid = 0;
            }),
            ("gateway_address", |c| {
                c.gateway_address = std::net::Ipv4Addr::UNSPECIFIED;
            }),
            ("gateway_address", |c| {
                c.gateway_address = std::net::Ipv4Addr::LOCALHOST;
            }),
            ("readiness_interval", |c| {
                c.readiness_interval = Duration::ZERO;
            }),
            ("operation_timeout", |c| {
                c.operation_timeout = Duration::ZERO;
            }),
            ("create_timeout", |c| {
                c.create_timeout = Duration::ZERO;
            }),
            ("create_budget", |c| {
                c.create_budget = Duration::ZERO;
            }),
            ("create_budget", |c| {
                c.create_budget = Duration::from_mins(11);
            }),
        ];
        for (field, mutate) in cases {
            let mut invalid = config()?;
            mutate(&mut invalid);
            assert!(
                matches!(invalid.validate(), Err(SandboxError::InvalidSpec { field: f, .. }) if f == field),
                "{field}"
            );
        }
        Ok(())
    }
}
