use serde_json::{Value, json};

/// SBX-D-001: one table is applied to pure configuration and live inspect responses.
pub fn assert_hardening(config: &Value, network: &str) {
    let cases = [
        ("/User", json!("1000:1000")),
        ("/WorkingDir", json!("/workspace")),
        ("/StopTimeout", json!(10)),
        ("/HostConfig/ReadonlyRootfs", json!(true)),
        ("/HostConfig/Privileged", json!(false)),
        ("/HostConfig/CapDrop", json!(["ALL"])),
        (
            "/HostConfig/SecurityOpt",
            json!(["no-new-privileges", "seccomp=builtin"]),
        ),
        ("/HostConfig/Memory", json!(67_108_864)),
        ("/HostConfig/MemorySwap", json!(67_108_864)),
        ("/HostConfig/PidsLimit", json!(64)),
        ("/HostConfig/CpuPeriod", json!(1_000_000)),
        ("/HostConfig/CpuQuota", json!(500_000)),
        ("/HostConfig/NetworkMode", json!(network)),
        ("/HostConfig/IpcMode", json!("private")),
        ("/HostConfig/RestartPolicy/Name", json!("no")),
        ("/HostConfig/Mounts/0/Target", json!("/workspace")),
        ("/HostConfig/Mounts/0/ReadOnly", json!(false)),
        ("/HostConfig/Mounts/1/Target", json!("/run/tqa/token")),
        ("/HostConfig/Mounts/1/ReadOnly", json!(true)),
        ("/HostConfig/Mounts/1/Type", json!("bind")),
        ("/HostConfig/Mounts/2/Target", json!("/run/tqa/state")),
        ("/HostConfig/Mounts/2/ReadOnly", json!(false)),
        ("/HostConfig/Mounts/2/Type", json!("bind")),
    ];
    for (pointer, expected) in cases {
        assert_eq!(
            config.pointer(pointer),
            Some(&expected),
            "hardening field {pointer}"
        );
    }
    for pointer in [
        "/HostConfig/CapAdd",
        "/HostConfig/Devices",
        "/HostConfig/DeviceRequests",
        "/HostConfig/PortBindings",
        "/HostConfig/VolumesFrom",
        "/Volumes",
    ] {
        assert!(
            config.pointer(pointer).is_none_or(|value| value.is_null()
                || value.as_array().is_some_and(Vec::is_empty)
                || value.as_object().is_some_and(serde_json::Map::is_empty)),
            "unexpected {pointer}"
        );
    }
    assert_ne!(config.pointer("/HostConfig/PidMode"), Some(&json!("host")));
    assert_eq!(
        config
            .pointer("/HostConfig/Mounts")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );
    let temporary = config
        .pointer("/HostConfig/Tmpfs/~1tmp")
        .and_then(Value::as_str);
    assert!(
        temporary.is_some_and(|value| value.contains("size=16777216")
            && value.contains("nosuid")
            && value.contains("nodev"))
    );
}
