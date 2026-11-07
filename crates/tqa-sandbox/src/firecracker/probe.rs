//! The engine-backed [`HostProbe`]: each fact is established by a short-lived container from the VM host image, so
//! the preflight observes exactly what the VMs will see.

use async_trait::async_trait;
use bollard::models::{ContainerCreateBody, DeviceMapping, HostConfig};

use super::{
    VMM_CONTAINER_UID,
    engine::{Engine, NetworkState, helper_labels},
    preflight::HostProbe,
};
use crate::{ImageRef, SandboxError};

pub(crate) struct EngineProbe<'a> {
    pub(crate) engine: &'a dyn Engine,
    pub(crate) image: &'a ImageRef,
    pub(crate) network: &'a str,
    /// Each probe helper's bound: the create bound, as for any container run (SBX-F-032); a probe is not part of a
    /// start's budget.
    pub(crate) bound: std::time::Duration,
}

impl EngineProbe<'_> {
    fn body(
        &self,
        user: String,
        device: &str,
        capabilities: &[&str],
        cmd: &[&str],
    ) -> ContainerCreateBody {
        ContainerCreateBody {
            image: Some(self.image.to_string()),
            // A probe is a helper too: if the gateway stops mid-probe, the next start or a stack's sweep removes it.
            labels: Some(helper_labels(self.network)),
            user: Some(user),
            entrypoint: Some(Vec::new()),
            cmd: Some(cmd.iter().map(|&part| part.to_owned()).collect()),
            host_config: Some(HostConfig {
                network_mode: Some("none".into()),
                readonly_rootfs: Some(true),
                cap_drop: Some(vec!["ALL".into()]),
                cap_add: Some(capabilities.iter().map(|&cap| cap.to_owned()).collect()),
                security_opt: Some(vec!["no-new-privileges".into()]),
                devices: Some(vec![DeviceMapping {
                    path_on_host: Some(device.into()),
                    path_in_container: Some(device.into()),
                    cgroup_permissions: Some("rwm".into()),
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

#[async_trait]
impl HostProbe for EngineProbe<'_> {
    async fn vmm_image_present(&self) -> Result<bool, SandboxError> {
        self.engine.image_present(&self.image.to_string()).await
    }

    async fn sandbox_network(&self) -> Result<NetworkState, SandboxError> {
        self.engine.network(self.network).await
    }

    async fn kvm_opens_for_vmm(&self) -> Result<bool, SandboxError> {
        // Opening read-write is the real test: a device passed into a rootless container looks accessible without it.
        let user = format!("{VMM_CONTAINER_UID}:{VMM_CONTAINER_UID}");
        let body = self.body(
            user,
            "/dev/kvm",
            &[],
            &["/bin/sh", "-c", "exec 3<>/dev/kvm"],
        );
        Ok(self.engine.run(body, self.bound).await? == 0)
    }

    async fn tun_available(&self) -> Result<bool, SandboxError> {
        let cmd = [
            "/usr/sbin/ip",
            "tuntap",
            "add",
            "dev",
            "tqa-probe",
            "mode",
            "tap",
        ];
        let body = self.body("0:0".into(), "/dev/net/tun", &["NET_ADMIN"], &cmd);
        Ok(self.engine.run(body, self.bound).await? == 0)
    }
}
