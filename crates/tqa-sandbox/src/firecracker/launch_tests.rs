use std::{error::Error, path::Path, time::SystemTime};

use serde_json::json;

use super::{Files, container, vm_config};
use crate::{
    ByteSize, CpuLimit, SandboxError,
    firecracker::test_support::{TOKEN, config, spec},
};

fn files() -> Files<'static> {
    Files {
        kernel: Path::new("/srv/tqa/kernel/vmlinux"),
        rootfs: Path::new("/srv/tqa/images/b/rootfs.ext4"),
        workspace: Path::new("/srv/tqa/workspaces/workspace_1.ext4"),
        state: Path::new("/srv/tqa/state/conversation_1.ext4"),
        sandbox_dir: Path::new("/srv/tqa/sandboxes/tqa-conversation_1-1"),
    }
}

#[test]
fn sbx_f_013_vm_configuration_golden() -> Result<(), Box<dyn Error>> {
    let expected = json!({
        "boot-source": {
            "kernel_image_path": "/vm/vmlinux",
            "boot_args": "console=ttyS0 reboot=k panic=1 pci=off quiet init=/usr/sbin/tqa-vm-init ip=169.254.100.2::169.254.100.1:255.255.255.252::eth0:off"
        },
        "drives": [
            { "drive_id": "rootfs", "path_on_host": "/vm/rootfs.img", "is_root_device": true, "is_read_only": true },
            { "drive_id": "workspace", "path_on_host": "/vm/workspace.img", "is_root_device": false, "is_read_only": false, "cache_type": "Writeback" },
            { "drive_id": "state", "path_on_host": "/vm/state.img", "is_root_device": false, "is_read_only": false, "cache_type": "Writeback" },
            { "drive_id": "config", "path_on_host": "/vm/config.img", "is_root_device": false, "is_read_only": true },
            { "drive_id": "exit", "path_on_host": "/vm/exit.img", "is_root_device": false, "is_read_only": false }
        ],
        "machine-config": { "vcpu_count": 2, "mem_size_mib": 512 },
        "network-interfaces": [{ "iface_id": "eth0", "host_dev_name": "tap0" }]
    });
    assert_eq!(vm_config(&spec()?)?, expected);
    let mut small = spec()?;
    small.memory = ByteSize::new(64 << 20)?;
    assert!(matches!(
        vm_config(&small),
        Err(SandboxError::InvalidSpec {
            field: "memory",
            ..
        })
    ));
    let mut wide = spec()?;
    wide.cpu = CpuLimit::new(33_000)?;
    assert!(matches!(
        vm_config(&wide),
        Err(SandboxError::InvalidSpec { field: "cpu", .. })
    ));
    Ok(())
}

#[test]
fn sbx_f_013_container_request_posture() -> Result<(), Box<dyn Error>> {
    let config = config()?;
    let body = container(
        &spec()?,
        &config,
        &files(),
        "tqa-conversation_1-1",
        SystemTime::UNIX_EPOCH,
    )?;
    let value = serde_json::to_value(&body)?;
    assert_eq!(value["User"], "0:0");
    assert_eq!(value["Cmd"], json!(["/usr/local/lib/tqa/launch"]));
    assert_eq!(value["Env"], json!([]));
    assert_eq!(value["Labels"]["tqa.backend"], "firecracker");
    assert_eq!(value["Labels"]["tqa.instance"], "tqa-sandbox");
    let host = &value["HostConfig"];
    assert_eq!(host["ReadonlyRootfs"], true);
    assert_eq!(host["CapDrop"], json!(["ALL"]));
    assert_eq!(
        host["CapAdd"],
        json!(["NET_ADMIN", "SETUID", "SETGID", "SETPCAP"])
    );
    assert_eq!(host["SecurityOpt"], json!(["no-new-privileges"]));
    assert_eq!(host["NetworkMode"], "tqa-sandbox");
    assert_eq!(host["Memory"], (512 + 512) << 20);
    assert_eq!(host["MemorySwap"], host["Memory"]);
    assert_eq!(host["NanoCpus"], 1_500_000_000_i64);
    assert_eq!(host["PidsLimit"], 64);
    assert_eq!(host["Sysctls"], json!({ "net.ipv4.ip_forward": "1" }));
    let devices: Vec<_> = host["Devices"]
        .as_array()
        .ok_or("devices")?
        .iter()
        .map(|d| d["PathOnHost"].clone())
        .collect();
    assert_eq!(devices, [json!("/dev/kvm"), json!("/dev/net/tun")]);
    let mounts: Vec<_> = host["Mounts"]
        .as_array()
        .ok_or("mounts")?
        .iter()
        .map(|m| {
            (
                m["Target"].as_str().unwrap_or_default().to_owned(),
                m["ReadOnly"].as_bool().unwrap_or(false),
            )
        })
        .collect();
    let expected = [
        ("/vm/vmlinux", true),
        ("/vm/rootfs.img", true),
        ("/vm/workspace.img", false),
        ("/vm/state.img", false),
        ("/vm/config.img", true),
        ("/vm/exit.img", false),
        ("/vm/vm.json", true),
        ("/vm/rules.nft", true),
    ];
    assert_eq!(
        mounts,
        expected.map(|(target, read_only)| (target.to_owned(), read_only))
    );
    assert!(host["Binds"].is_null() && host["Privileged"] != true);
    Ok(())
}

#[test]
fn sbx_f_011_token_is_absent_from_every_launch_document() -> Result<(), Box<dyn Error>> {
    let config = config()?;
    let spec = spec()?;
    let body = container(
        &spec,
        &config,
        &files(),
        "tqa-conversation_1-1",
        SystemTime::UNIX_EPOCH,
    )?;
    for text in [
        serde_json::to_string(&body)?,
        format!("{body:?}"),
        serde_json::to_string(&vm_config(&spec)?)?,
        format!("{spec:?}"),
        format!("{config:?}"),
    ] {
        assert!(
            !text.contains(TOKEN),
            "the token leaked into a launch document"
        );
    }
    Ok(())
}

#[test]
fn sbx_f_013_guest_memory_is_whole_mib_and_at_least_the_minimum()
-> Result<(), Box<dyn std::error::Error>> {
    let sized = |bytes: u64| -> Result<Result<serde_json::Value, SandboxError>, Box<dyn std::error::Error>> {
        let mut spec = spec()?;
        spec.memory = ByteSize::new(bytes)?;
        Ok(vm_config(&spec))
    };
    let accepted = sized(256 << 20)??;
    assert_eq!(accepted["machine-config"]["mem_size_mib"], 256);
    // 128 MiB boots the VM but leaves no room for the sandbox beside the guest's memory reserve.
    for bytes in [(256 << 20) - (1 << 20), 128 << 20, (256 << 20) + 1] {
        let refused = sized(bytes)?;
        assert!(
            matches!(
                refused,
                Err(SandboxError::InvalidSpec {
                    field: "memory",
                    ..
                })
            ),
            "{bytes}: {refused:?}"
        );
    }
    Ok(())
}

// TEST: SBX-C-020
/// The microVM has no Desktop rootfs yet: a desktop sandbox is refused, never started without its mounts.
#[test]
fn sbx_c_020_the_microvm_refuses_a_desktop_sandbox_until_its_rootfs_has_one()
-> Result<(), Box<dyn Error>> {
    let mut spec = spec()?;
    spec.validate(crate::BackendKind::Firecracker)?;
    spec.desktop = true;
    assert!(matches!(
        spec.validate(crate::BackendKind::Firecracker),
        Err(SandboxError::InvalidSpec {
            field: "desktop",
            ..
        })
    ));
    Ok(())
}

// TEST: SBX-F-039
#[test]
fn sbx_f_039_the_tasks_drives_write_back_and_the_others_keep_firecrackers_default()
-> Result<(), Box<dyn Error>> {
    let config = vm_config(&spec()?)?;
    let caches: Vec<(String, Option<String>)> = config["drives"]
        .as_array()
        .ok_or("no drives")?
        .iter()
        .map(|drive| {
            (
                drive["drive_id"].as_str().unwrap_or_default().to_owned(),
                drive["cache_type"].as_str().map(str::to_owned),
            )
        })
        .collect();
    assert_eq!(
        caches,
        vec![
            ("rootfs".to_owned(), None),
            ("workspace".to_owned(), Some("Writeback".to_owned())),
            ("state".to_owned(), Some("Writeback".to_owned())),
            ("config".to_owned(), None),
            ("exit".to_owned(), None),
        ],
        "a task's fsync reaches the host disk for its workspace and state; read-only drives take no writes"
    );
    Ok(())
}
