//! The per-VM launch configuration (SBX-F-013): the Firecracker VM description and the container request for the VM
//! host. The container starts as its namespaced root only so the launcher can create the tap and load the rules; it
//! then executes Firecracker as [`VMM_CONTAINER_UID`] with no capabilities. Neither document carries the token.

use std::{collections::HashMap, path::Path, time::SystemTime};

use bollard::models::{
    ContainerCreateBody, DeviceMapping, HostConfig, Mount, MountType, RestartPolicy,
    RestartPolicyNameEnum,
};
use serde_json::{Value, json};

use super::{FirecrackerBackendConfig, VMM_CONTAINER_UID, device, exit_record, rules};
use crate::{BackendKind, SandboxError, SandboxSpec};

/// Where the per-VM files appear inside the VM host container.
pub(crate) const VM_DIR: &str = "/vm";
/// The launcher (images/vmm) that prepares the network and executes Firecracker.
pub(crate) const LAUNCHER: &str = "/usr/local/lib/tqa/launch";
const MIB: u64 = 1 << 20;
/// The smallest guest memory, in bytes: the guest keeps `tqa-vm-init`'s 64 MiB reserve outside the sandbox and the
/// sandbox needs at least 128 MiB, while `MemTotal` is somewhat below the configured memory. The gateway's admin API
/// refuses the same values (SBX-C-003; deploy/firecracker/README.md, "Memory").
pub(crate) const MIN_VM_MEMORY: u64 = 256 * MIB;
const MAX_VCPUS: u32 = 32;
const VMM_PIDS: i64 = 64;

/// Host paths of one VM's files.
pub(crate) struct Files<'a> {
    pub(crate) kernel: &'a Path,
    pub(crate) rootfs: &'a Path,
    pub(crate) workspace: &'a Path,
    pub(crate) state: &'a Path,
    /// Holds the configuration device, `vm.json` and `rules.nft`.
    pub(crate) sandbox_dir: &'a Path,
}

/// The Firecracker configuration: fixed paths inside the container, the fixed command line, the spec's size.
///
/// # Errors
/// Returns `InvalidSpec` for more than 32 vCPUs, or memory below [`MIN_VM_MEMORY`] or not a whole number of MiB.
pub(crate) fn vm_config(spec: &SandboxSpec) -> Result<Value, SandboxError> {
    let vcpus = spec.cpu.millicores().div_ceil(1000);
    if vcpus > MAX_VCPUS {
        return Err(SandboxError::invalid(
            "cpu",
            "a microVM has at most 32 vCPUs",
        ));
    }
    if spec.memory.bytes() < MIN_VM_MEMORY {
        return Err(SandboxError::invalid(
            "memory",
            "a microVM needs at least 256 MiB",
        ));
    }
    if !spec.memory.bytes().is_multiple_of(MIB) {
        return Err(SandboxError::invalid(
            "memory",
            "a microVM's memory is a whole number of MiB",
        ));
    }
    let memory = spec.memory.bytes() / MIB;
    let drive = |id: &str, root: bool, read_only: bool| json!({ "drive_id": id, "path_on_host": format!("{VM_DIR}/{id}.img"), "is_root_device": root, "is_read_only": read_only });
    // The task's data: the guest's flushes reach the image on the host (an fsync), so a task's fsync survives a host
    // crash, as on the Docker backend's bind mount. Firecracker's default, `Unsafe`, offers the guest no flush at all,
    // which risks the guest filesystem, not just the last writes (SBX-F-039).
    let durable = |id: &str| {
        let mut entry = drive(id, false, false);
        entry["cache_type"] = json!("Writeback");
        entry
    };
    Ok(json!({
        "boot-source": {
            "kernel_image_path": format!("{VM_DIR}/vmlinux"),
            "boot_args": format!(
                "console=ttyS0 reboot=k panic=1 pci=off quiet init=/usr/sbin/tqa-vm-init {}",
                rules::kernel_ip_argument()
            ),
        },
        // The exit record only needs to outlive the VM, read while the host is up; the read-only drives take no writes.
        "drives": [drive("rootfs", true, true), durable("workspace"), durable("state"), drive("config", false, true), drive("exit", false, false)],
        "machine-config": { "vcpu_count": vcpus, "mem_size_mib": memory },
        "network-interfaces": [{ "iface_id": "eth0", "host_dev_name": rules::TAP }],
    }))
}

fn bind(source: &Path, target: &str, read_only: bool) -> Result<Mount, SandboxError> {
    let source = source
        .to_str()
        .ok_or_else(|| SandboxError::invalid("mount", "path is not UTF-8"))?;
    Ok(Mount {
        typ: Some(MountType::BIND),
        source: Some(source.into()),
        target: Some(format!("{VM_DIR}/{target}")),
        read_only: Some(read_only),
        ..Default::default()
    })
}

fn mounts(files: &Files<'_>) -> Result<Vec<Mount>, SandboxError> {
    let directory = files.sandbox_dir;
    [
        (files.kernel, "vmlinux", true),
        (files.rootfs, "rootfs.img", true),
        (files.workspace, "workspace.img", false),
        (files.state, "state.img", false),
        (&directory.join(device::FILE), "config.img", true),
        (&directory.join(exit_record::FILE), "exit.img", false),
        (&directory.join("vm.json"), "vm.json", true),
        (&directory.join("rules.nft"), "rules.nft", true),
    ]
    .into_iter()
    .map(|(source, target, read_only)| bind(source, target, read_only))
    .collect()
}

/// The VM host container request.
///
/// # Errors
/// Returns `InvalidSpec` for an invalid spec, an unrepresentable limit or a non-UTF-8 path.
pub(crate) fn container(
    spec: &SandboxSpec,
    config: &FirecrackerBackendConfig,
    files: &Files<'_>,
    name: &str,
    created_at: SystemTime,
) -> Result<ContainerCreateBody, SandboxError> {
    spec.validate(BackendKind::Firecracker)?;
    let created = created_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| {
            SandboxError::invalid("created_at", "creation time precedes the Unix epoch")
        })?;
    let memory = spec
        .memory
        .signed()
        .checked_add(config.vmm_overhead.signed())
        .ok_or_else(|| {
            SandboxError::invalid("memory", "memory limit exceeds the supported range")
        })?;
    let labels = HashMap::from([
        ("tqa.conversation".into(), spec.conversation_id.to_string()),
        ("tqa.workspace".into(), spec.workspace.id.to_string()),
        ("tqa.image_digest".into(), spec.image.digest().to_owned()),
        ("tqa.created_at".into(), created.as_secs().to_string()),
        ("tqa.instance".into(), config.network.clone()),
        ("tqa.token_id".into(), name.to_owned()),
        ("tqa.backend".into(), "firecracker".into()),
        ("tqa.vmm_uid".into(), VMM_CONTAINER_UID.to_string()),
    ]);
    let device = |path: &str| DeviceMapping {
        path_on_host: Some(path.into()),
        path_in_container: Some(path.into()),
        cgroup_permissions: Some("rwm".into()),
    };
    Ok(ContainerCreateBody {
        image: Some(config.vmm_image.to_string()),
        user: Some("0:0".into()),
        cmd: Some(vec![LAUNCHER.into()]),
        env: Some(Vec::new()),
        labels: Some(labels),
        network_disabled: Some(false),
        host_config: Some(HostConfig {
            readonly_rootfs: Some(true),
            cap_drop: Some(vec!["ALL".into()]),
            // Namespaced capabilities for the launcher only: the tap and the rules, then the switch to the VMM uid.
            cap_add: Some(
                ["NET_ADMIN", "SETUID", "SETGID", "SETPCAP"]
                    .map(String::from)
                    .to_vec(),
            ),
            security_opt: Some(vec!["no-new-privileges".into()]),
            devices: Some(vec![device("/dev/kvm"), device("/dev/net/tun")]),
            sysctls: Some(HashMap::from([("net.ipv4.ip_forward".into(), "1".into())])),
            memory: Some(memory),
            memory_swap: Some(memory),
            nano_cpus: Some(i64::from(spec.cpu.millicores()) * 1_000_000),
            pids_limit: Some(VMM_PIDS),
            network_mode: Some(config.network.clone()),
            tmpfs: Some(HashMap::from([(
                "/run".into(),
                "rw,nosuid,nodev,noexec,size=16m,mode=0755".into(),
            )])),
            mounts: Some(mounts(files)?),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::NO),
                maximum_retry_count: None,
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
