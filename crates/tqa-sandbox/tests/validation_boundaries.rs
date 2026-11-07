//! SBX-D-001: bounded parsers and policy validators reject malformed edge inputs.

use secrecy::SecretString;
use std::{
    collections::BTreeMap,
    error::Error,
    path::PathBuf,
    time::{Duration, SystemTime},
};
use tqa_sandbox::{
    BackendKind, ByteSize, CpuLimit, EgressMode, HostRule, ImageRef, SandboxError, WorkspaceSource,
    docker::{DockerBackendConfig, container_config},
};

#[path = "support/spec.rs"]
mod support;
type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn sbx_d_001_limits_images_and_error_codes_have_exact_boundaries() -> TestResult {
    assert_eq!(ByteSize::new(1)?.bytes(), 1);
    assert_eq!(
        ByteSize::new(i64::MAX.unsigned_abs())?.bytes(),
        i64::MAX.unsigned_abs()
    );
    assert_eq!(CpuLimit::new(1)?.millicores(), 1);
    assert_eq!(CpuLimit::new(1_000_000)?.millicores(), 1_000_000);
    let digest = format!("sha256:{}", "f".repeat(64));
    for name in [
        String::new(),
        "x".repeat(256),
        "repo@tag".into(),
        "repo name".into(),
    ] {
        assert!(ImageRef::new(name, &digest).is_err());
    }
    for digest in [
        format!("sha256:{}", "F".repeat(64)),
        format!("sha256:{}", "g".repeat(64)),
        format!("sha256:{}", "a".repeat(65)),
    ] {
        assert!(ImageRef::new("repo", digest).is_err());
    }
    assert_eq!(ImageRef::new("repo:tag", &digest)?.name(), Some("repo:tag"));
    let errors = [
        (SandboxError::Preflight("missing"), "preflight", false),
        (SandboxError::ImageNotFound, "image_not_found", false),
        (
            SandboxError::ImageDigestMismatch,
            "image_digest_mismatch",
            false,
        ),
        (
            SandboxError::InvalidSpec {
                field: "env",
                reason: "invalid".into(),
            },
            "invalid_spec",
            false,
        ),
        (SandboxError::Backend("redacted".into()), "backend", true),
        (SandboxError::NotFound, "not_found", false),
        (SandboxError::Timeout, "timeout", true),
        (SandboxError::Cancelled, "cancelled", false),
    ];
    for (error, code, retryable) in errors {
        assert_eq!(error.code(), format!("sandbox.{code}"));
        assert_eq!(error.retryable(), retryable);
        assert!(!format!("{error:?}").is_empty());
    }
    Ok(())
}

#[test]
fn sbx_d_001_workspace_and_clock_boundaries_are_fail_closed() -> TestResult {
    let mut spec = support::spec()?;
    for name in [
        String::new(),
        "v".repeat(129),
        "_volume".into(),
        "volume/name".into(),
    ] {
        spec.workspace.source = WorkspaceSource::NamedVolume(name);
        assert!(spec.validate(BackendKind::Docker).is_err());
    }
    for path in ["/", "/workspace/../escape", "relative"] {
        spec.workspace.source = WorkspaceSource::HostDirectory(PathBuf::from(path));
        assert!(spec.validate(BackendKind::Docker).is_err());
    }
    spec.workspace.source = WorkspaceSource::HostDirectory(PathBuf::from("/workspace-data"));
    let body = container_config(&spec, "tqa-unit", SystemTime::UNIX_EPOCH, token_file())?;
    assert_eq!(
        serde_json::to_value(body)?["HostConfig"]["Mounts"][0]["Type"],
        "bind"
    );
    assert!(
        container_config(
            &spec,
            "tqa-unit",
            SystemTime::UNIX_EPOCH - Duration::from_secs(1),
            token_file()
        )
        .is_err()
    );
    spec.workspace.source = WorkspaceSource::BlockImage(PathBuf::from("/workspace-data.ext4"));
    spec.validate(BackendKind::Firecracker)?;
    assert!(container_config(&spec, "tqa-unit", SystemTime::UNIX_EPOCH, token_file()).is_err());
    spec.disk = Some(ByteSize::new(1)?);
    spec.workspace.source = WorkspaceSource::NamedVolume("volume".into());
    assert!(spec.validate(BackendKind::Docker).is_err());
    Ok(())
}

#[test]
fn sbx_d_001_spec_variable_and_lifetime_bounds_are_enforced() -> TestResult {
    let valid = support::spec()?;
    for token in [String::new(), "x".repeat(8193), "invalid\0token".into()] {
        let mut spec = valid.clone();
        spec.runtime_token = SecretString::from(token);
        assert!(spec.validate(BackendKind::Docker).is_err());
    }
    for (key, value) in [
        (String::new(), "x".into()),
        ("x".repeat(129), "x".into()),
        ("1NAME".into(), "x".into()),
        ("HAS-DASH".into(), "x".into()),
        ("NAME".into(), "x".repeat(8193)),
        ("NAME".into(), "bad\0value".into()),
    ] {
        let mut spec = valid.clone();
        spec.env = BTreeMap::from([(key, value)]);
        assert!(spec.validate(BackendKind::Docker).is_err());
    }
    let mut spec = valid.clone();
    spec.env = (0..65)
        .map(|index| (format!("NAME_{index}"), String::new()))
        .collect();
    assert!(spec.validate(BackendKind::Docker).is_err());
    spec = valid;
    spec.pids = 65_537;
    assert!(spec.validate(BackendKind::Docker).is_err());
    Ok(())
}

#[test]
fn sbx_d_001_hosts_urls_and_network_names_are_bounded() -> TestResult {
    assert_eq!(HostRule::new("EXAMPLE.COM")?.host(), "example.com");
    for host in [
        String::new(),
        "x".repeat(254),
        "*.example.com".into(),
        "é.example".into(),
        "bad host".into(),
        "a/b".into(),
    ] {
        assert!(HostRule::new(host).is_err());
    }
    let mut spec = support::spec()?;
    spec.egress.allow = vec![HostRule::new("example.com")?; 257];
    assert!(spec.egress.validate().is_err());
    spec.egress.allow.clear();
    for endpoint in [
        "file:///workspace",
        "http://gateway.invalid/?key=x",
        "http://gateway.invalid/#fragment",
        "http://user@gateway.invalid/",
    ] {
        spec.gateway_url = endpoint.parse()?;
        assert!(spec.validate(BackendKind::Docker).is_err());
    }
    spec.gateway_url = format!("http://gateway.invalid/{}", "x".repeat(2048)).parse()?;
    assert!(spec.validate(BackendKind::Docker).is_err());
    spec = support::spec()?;
    for network in [
        String::new(),
        "n".repeat(64),
        "host".into(),
        "bridge".into(),
        "none".into(),
        "default".into(),
        "has space".into(),
    ] {
        assert!(container_config(&spec, &network, SystemTime::UNIX_EPOCH, token_file()).is_err());
        assert!(DockerBackendConfig::new(network, "/sandbox-storage").is_err());
    }
    spec.egress.mode = EgressMode::Proxy;
    assert!(spec.egress.validate().is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn sbx_c_016_non_utf8_workspace_is_rejected() -> TestResult {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let mut spec = support::spec()?;
    spec.workspace.source = WorkspaceSource::HostDirectory(PathBuf::from(OsString::from_vec(
        b"/workspace-\xff".to_vec(),
    )));
    assert!(spec.validate(BackendKind::Docker).is_err());
    assert!(container_config(&spec, "tqa-unit", SystemTime::UNIX_EPOCH, token_file()).is_err());
    Ok(())
}

fn token_file() -> &'static std::path::Path {
    std::path::Path::new("/sandbox-storage/sandboxes/sandbox_unit/token")
}
