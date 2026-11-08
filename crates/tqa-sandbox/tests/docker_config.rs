//! SBX-D-001: Docker hardening is a pure, deterministic transformation.

use std::{
    error::Error,
    time::{Duration, SystemTime},
};

use proptest::prelude::*;
use secrecy::ExposeSecret;
use serde_json::json;
use tqa_sandbox::{CpuLimit, EgressMode, SandboxError, docker::container_config};

#[path = "support/hardening.rs"]
mod hardening;
#[path = "support/spec.rs"]
mod support;

#[test]
fn sbx_d_001_table_checks_every_required_hardening_setting() -> Result<(), Box<dyn Error>> {
    let spec = support::spec()?;
    let created = SystemTime::UNIX_EPOCH + Duration::from_secs(123);
    let config = container_config(&spec, "sandbox-internal", created, token_file())?;
    let json = serde_json::to_value(&config)?;
    hardening::assert_hardening(&json, "sandbox-internal");
    assert_eq!(json.pointer("/Image"), Some(&json!(spec.image.to_string())));
    assert_eq!(
        json.pointer("/Labels/tqa.conversation"),
        Some(&json!("conversation_1"))
    );
    assert_eq!(
        json.pointer("/Labels/tqa.workspace"),
        Some(&json!("workspace_1"))
    );
    assert_eq!(
        json.pointer("/Labels/tqa.image_digest"),
        Some(&json!(spec.image.digest()))
    );
    assert_eq!(json.pointer("/Labels/tqa.created_at"), Some(&json!("123")));
    assert_eq!(
        json.pointer("/HostConfig/Mounts/0/Type"),
        Some(&json!("volume"))
    );
    assert_eq!(
        json.pointer("/HostConfig/Mounts/0/Source"),
        Some(&json!("workspace_1"))
    );
    Ok(())
}

#[test]
fn sbx_c_005_token_is_absent_from_body_debug_and_environment() -> Result<(), Box<dyn Error>> {
    let spec = support::spec()?;
    let config = container_config(
        &spec,
        "sandbox-internal",
        SystemTime::UNIX_EPOCH,
        token_file(),
    )?;
    assert!(!format!("{config:?}").contains(spec.runtime_token.expose_secret()));
    assert!(!serde_json::to_string(&config)?.contains(spec.runtime_token.expose_secret()));
    assert_eq!(
        config.env,
        Some(vec![
            "LANG=C".into(),
            "TQA_GATEWAY_URL=http://gateway.invalid/runtime".into(),
            "TQA_RUNTIME_TOKEN_FILE=/run/tqa/token".into(),
            "TQA_STATE_DIR=/run/tqa/state".into()
        ])
    );
    let body = serde_json::to_value(config)?;
    assert_eq!(
        body["HostConfig"]["Mounts"][1]["Source"],
        json!(token_file())
    );
    assert_eq!(body["HostConfig"]["Mounts"][1]["ReadOnly"], json!(true));
    Ok(())
}

#[test]
fn sbx_c_004_proxy_is_explicit_and_internal_has_no_proxy_environment() -> Result<(), Box<dyn Error>>
{
    let mut spec = support::spec()?;
    let internal = container_config(
        &spec,
        "sandbox-internal",
        SystemTime::UNIX_EPOCH,
        token_file(),
    )?;
    assert!(!serde_json::to_string(&internal.env)?.contains("PROXY"));
    spec.egress.mode = EgressMode::Proxy;
    assert!(matches!(
        container_config(&spec, "sandbox-proxy", SystemTime::UNIX_EPOCH, token_file()),
        Err(SandboxError::InvalidSpec {
            field: "egress",
            ..
        })
    ));
    spec.egress.proxy_url = Some("http://proxy.invalid:8080/".parse()?);
    let proxy = container_config(&spec, "sandbox-proxy", SystemTime::UNIX_EPOCH, token_file())?;
    let env = proxy.env.ok_or("missing proxy environment")?;
    // The proxy's origin, with no path: Chromium refuses a proxy with one (SBX-U-016).
    assert!(env.contains(&"HTTP_PROXY=http://proxy.invalid:8080".into()));
    assert!(env.contains(&"HTTPS_PROXY=http://proxy.invalid:8080".into()));
    assert!(env.contains(&"TQA_EGRESS_PROXY_URL=http://proxy.invalid:8080".into()));
    assert!(env.contains(&"NO_PROXY=gateway.invalid,localhost,127.0.0.1,::1".into()));
    for address in [
        "ftp://proxy.invalid",
        "http://user:password@proxy.invalid",
        "http://proxy.invalid?query",
        "http://proxy.invalid/#fragment",
    ] {
        spec.egress.proxy_url = Some(address.parse()?);
        assert!(
            container_config(&spec, "sandbox-proxy", SystemTime::UNIX_EPOCH, token_file()).is_err()
        );
    }
    spec.egress.mode = EgressMode::Internal;
    assert!(
        container_config(
            &spec,
            "sandbox-internal",
            SystemTime::UNIX_EPOCH,
            token_file()
        )
        .is_err()
    );
    Ok(())
}

proptest! {
    #[test]
    fn sbx_d_001_equal_specifications_produce_equal_configuration(millicores in 1_u32..16_000, name in "[a-z]{1,20}") {
        let mut spec = support::spec().map_err(|error| TestCaseError::fail(error.to_string()))?;
        spec.cpu = CpuLimit::new(millicores).map_err(|error| TestCaseError::fail(error.to_string()))?;
        spec.env.insert("MODE".into(), name);
        let first = container_config(&spec, "sandbox-internal", SystemTime::UNIX_EPOCH, token_file()).map_err(|error| TestCaseError::fail(error.to_string()))?;
        let second = container_config(&spec.clone(), "sandbox-internal", SystemTime::UNIX_EPOCH, token_file()).map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(first, second);
    }
}

// TEST: SBX-C-020
/// Only a desktop sandbox gets the 1 GiB `/dev/shm` and the browser's tmpfs, with exactly the mounts RT-I-046's probe
/// runs on; `/tmp` stays the 16 MiB noexec one.
#[test]
fn sbx_c_020_a_desktop_sandbox_gets_the_shared_memory_and_the_browser_tmpfs()
-> Result<(), Box<dyn Error>> {
    let mut spec = support::spec()?;
    let created = SystemTime::UNIX_EPOCH + Duration::from_secs(123);
    let plain = serde_json::to_value(container_config(
        &spec,
        "sandbox-internal",
        created,
        token_file(),
    )?)?;
    assert_eq!(plain.pointer("/HostConfig/ShmSize"), None);
    assert_eq!(
        plain.pointer("/HostConfig/Tmpfs"),
        Some(&json!({"/tmp": "rw,nosuid,nodev,noexec,size=16777216,mode=1777"}))
    );
    spec.desktop = true;
    let desktop = serde_json::to_value(container_config(
        &spec,
        "sandbox-internal",
        created,
        token_file(),
    )?)?;
    hardening::assert_hardening(&desktop, "sandbox-internal");
    assert_eq!(
        desktop.pointer("/HostConfig/ShmSize"),
        Some(&json!(1_073_741_824))
    );
    assert_eq!(
        desktop.pointer("/HostConfig/Tmpfs"),
        Some(&json!({
            "/tmp": "rw,nosuid,nodev,noexec,size=16777216,mode=1777",
            "/run/tqa-browser": "rw,nosuid,nodev,noexec,size=536870912,mode=0700,uid=1000,gid=1000",
            "/run/tqa-display": "rw,nosuid,nodev,noexec,size=8388608,mode=0700,uid=1000,gid=1000",
        }))
    );
    Ok(())
}

fn token_file() -> &'static std::path::Path {
    std::path::Path::new("/sandbox-storage/sandboxes/sandbox_unit/token")
}

// TEST: SBX-U-016
#[test]
fn sbx_u_016_the_proxy_variables_are_an_origin_with_no_path() -> Result<(), Box<dyn Error>> {
    // Chromium refuses a proxy URL with any path, "/" included (net::ERR_NO_SUPPORTED_PROXIES), while curl and pip
    // take either; the gateway's `url::Url` always writes "/" for http. So the variables carry the origin only.
    for (proxy, expected) in [
        ("http://proxy.invalid:8080/", "http://proxy.invalid:8080"),
        ("http://proxy.invalid:3128", "http://proxy.invalid:3128"),
        ("https://proxy.invalid:8443/", "https://proxy.invalid:8443"),
        ("http://[fd00::1]:8080/", "http://[fd00::1]:8080"),
    ] {
        let mut spec = support::spec()?;
        spec.egress.mode = EgressMode::Proxy;
        spec.egress.proxy_url = Some(proxy.parse()?);
        let config =
            container_config(&spec, "sandbox-proxy", SystemTime::UNIX_EPOCH, token_file())?;
        let env = config.env.ok_or("missing proxy environment")?;
        for name in ["HTTP_PROXY", "HTTPS_PROXY", "TQA_EGRESS_PROXY_URL"] {
            assert!(
                env.contains(&format!("{name}={expected}")),
                "{proxy}: {name} in {env:?}"
            );
        }
    }
    Ok(())
}

// TEST: SBX-U-018
#[test]
fn sbx_u_018_loopback_never_goes_through_the_proxy() -> Result<(), Box<dyn Error>> {
    // Real-world use before 17.0 (17 Oct 2026): NO_PROXY named only the gateway, so curl and Python sent a task's own
    // server on 127.0.0.1 to the egress proxy, which refused it. Loopback is bypassed, in both spellings; the rest
    // still goes through the proxy (HTTP_PROXY and HTTPS_PROXY unchanged).
    let mut spec = support::spec()?;
    spec.egress.mode = EgressMode::Proxy;
    spec.egress.proxy_url = Some("http://proxy.invalid:8080/".parse()?);
    let config = container_config(&spec, "sandbox-proxy", SystemTime::UNIX_EPOCH, token_file())?;
    let env = config.env.ok_or("missing proxy environment")?;
    for expected in [
        "NO_PROXY=gateway.invalid,localhost,127.0.0.1,::1",
        "no_proxy=gateway.invalid,localhost,127.0.0.1,::1",
        "HTTP_PROXY=http://proxy.invalid:8080",
        "HTTPS_PROXY=http://proxy.invalid:8080",
    ] {
        assert!(env.contains(&expected.into()), "{expected} in {env:?}");
    }
    Ok(())
}
