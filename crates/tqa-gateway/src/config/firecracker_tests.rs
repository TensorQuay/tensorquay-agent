use std::error::Error;

use crate::{GatewayConfig, GatewayError};

fn base(runtime: &str, extra: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='{runtime}'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{extra}"
    )
}

fn section(fields: &str) -> String {
    format!(
        "[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n{fields}",
        "b".repeat(64)
    )
}

fn refused(toml: &str) -> Option<&'static str> {
    match GatewayConfig::from_toml(toml) {
        Err(GatewayError::Configuration(reason)) => Some(reason),
        _ => None,
    }
}

#[test]
fn gw_i_055_firecracker_settings_default_from_the_sandbox_configuration()
-> Result<(), Box<dyn Error>> {
    let config = GatewayConfig::from_toml(&base("firecracker", &section("")))?;
    let settings = config.firecracker.ok_or("no firecracker settings")?;
    assert_eq!(
        settings.backend.kernel_dir,
        std::path::Path::new("/srv/tqa/kernel")
    );
    assert_eq!(
        settings.backend.rootfs_dir,
        std::path::Path::new("/srv/tqa/images")
    );
    assert_eq!(settings.backend.network, "tqa-sandboxes");
    // The runtime link and the policy proxy, which always runs (ADR-0028).
    assert_eq!(settings.backend.gateway_ports, [8080, 3128]);
    assert_eq!(settings.backend.vmm_file_uid, 10_000);
    // The VM host's memory above the guest's: room for the host page cache the guest's disk writes fill (G-13-full on
    // candidate 6: at 64 MiB the VMM was OOM-killed during a workspace fill).
    assert_eq!(settings.backend.vmm_overhead.bytes(), 512 << 20);
    // Deprecated (SBX-F-025): unset is the default, and only a set value is warned about.
    assert!(!settings.workspace_bytes_set);
    let custom = GatewayConfig::from_toml(&base(
        "firecracker",
        &section(
            "gateway_ports=[8080, 3128]\nworkspace_bytes=1073741824\nvmm_file_uid=175535\nkernel_dir='/opt/kernel'\n\
             vmm_overhead_bytes=1073741824",
        ),
    ))?;
    let settings = custom.firecracker.ok_or("no firecracker settings")?;
    assert!(
        settings.workspace_bytes_set,
        "a set workspace_bytes is warned about"
    );
    assert_eq!(settings.backend.gateway_ports, [8080, 3128]);
    assert_eq!(
        settings.backend.kernel_dir,
        std::path::Path::new("/opt/kernel")
    );
    assert_eq!(settings.backend.vmm_file_uid, 175_535);
    assert_eq!(settings.backend.vmm_overhead.bytes(), 1 << 30);
    Ok(())
}

#[test]
fn gw_i_055_firecracker_section_matches_the_backend_and_fails_closed() {
    assert_eq!(
        refused(&base("firecracker", "")),
        Some("runtime.backend = \"firecracker\" requires a [firecracker] section")
    );
    assert_eq!(
        refused(&base("docker", &section(""))),
        Some("[firecracker] applies only to runtime.backend = \"firecracker\"")
    );
    for (fields, reason) in [
        (
            "workspace_bytes=1024",
            "[firecracker] workspace_bytes must be 64 MiB through 2 TiB",
        ),
        (
            "gateway_ports=[]",
            "[firecracker] gateway_ports must be 1 through 8 distinct non-zero ports",
        ),
        ("vmm_file_uid=0", "[firecracker] vmm_file_uid must not be 0"),
        (
            "vmm_overhead_bytes=1048576",
            "[firecracker] vmm_overhead_bytes must be 64 MiB through 8 GiB",
        ),
        (
            "vmm_overhead_bytes=17179869184",
            "[firecracker] vmm_overhead_bytes must be 64 MiB through 8 GiB",
        ),
        (
            "kernel_dir='relative'",
            "[firecracker] directories must be absolute paths without traversal",
        ),
    ] {
        assert_eq!(
            refused(&base("firecracker", &section(fields))),
            Some(reason),
            "{fields}"
        );
    }
    // An unknown field is the TOML parse's refusal, naming it (GW-U-144).
    let unknown = GatewayConfig::from_toml(&base("firecracker", &section("unknown=1")))
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        unknown.starts_with(
            "invalid gateway configuration: firecracker.unknown: unknown field, expected "
        ),
        "{unknown}"
    );
    let loopback = base("firecracker", &section("")).replace("192.0.2.10", "127.0.0.1");
    assert!(refused(&loopback).is_some());
    let mutable = base("firecracker", &section("")).replace(
        &format!("vmm_image='sha256:{}'", "b".repeat(64)),
        "vmm_image='tqa-vmm:latest'",
    );
    assert!(refused(&mutable).is_some(), "a tag is not a pin");
    let literal = base("firecracker", &section(""))
        .replace("http://tqa-gateway:8080", "http://192.0.2.10:8080");
    assert_eq!(
        refused(&literal),
        Some("the Firecracker backend needs a gateway host name in [sandbox] gateway_url")
    );
}
