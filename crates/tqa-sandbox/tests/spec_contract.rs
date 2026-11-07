//! SBX-C-005 and SBX-D-001: invalid specifications fail before any host side effect.

use std::{error::Error, path::PathBuf};

use tqa_sandbox::{BackendKind, ByteSize, CpuLimit, ImageRef, SandboxError, WorkspaceSource};

#[test]
fn sbx_c_010_image_pins_round_trip_without_erasing_their_kind() -> Result<(), Box<dyn Error>> {
    let digest = format!("sha256:{}", "a".repeat(64));
    for (text, local) in [
        (format!("example/runtime@{digest}"), false),
        (digest.clone(), true),
    ] {
        let image: ImageRef = text.parse()?;
        assert_eq!(image.to_string(), text);
        assert_eq!(image.is_local(), local);
        assert_eq!(image.digest(), digest);
        assert_eq!(
            image.name(),
            if local { None } else { Some("example/runtime") }
        );
        assert_eq!(image.to_string().parse::<ImageRef>()?, image);
    }
    for invalid in [
        "runtime:base".to_owned(),
        String::new(),
        "sha256:abc".to_owned(),
        format!("sha256:{}", "A".repeat(64)),
        format!("sha256:{}", "g".repeat(64)),
        format!("sha256:{}", "a".repeat(65)),
    ] {
        assert!(invalid.parse::<ImageRef>().is_err(), "{invalid}");
    }
    Ok(())
}

#[path = "support/spec.rs"]
mod support;

#[test]
fn sbx_c_005_secret_looking_keys_are_rejected_without_echoing_values() -> Result<(), Box<dyn Error>>
{
    for key in [
        "API_KEY",
        "token",
        "ClientSecret",
        "PASSWORD",
        "cloud_credential",
        "monkey",
    ] {
        let mut spec = support::spec()?;
        spec.env
            .insert(key.into(), "sensitive-fixture-value".into());
        let error = spec
            .validate(BackendKind::Docker)
            .err()
            .ok_or("invalid env accepted")?;
        assert!(matches!(
            error,
            SandboxError::InvalidSpec { field: "env", .. }
        ));
        assert!(!format!("{error:?} {error}").contains("sensitive-fixture-value"));
    }
    Ok(())
}

#[test]
fn sbx_d_001_invalid_limits_and_missing_digest_are_typed() {
    assert!(matches!(
        ByteSize::new(0),
        Err(SandboxError::InvalidSpec {
            field: "memory",
            ..
        })
    ));
    assert!(CpuLimit::new(0).is_err());
    for digest in ["", "latest", "sha256:abc", "sha512:abc"] {
        assert!(matches!(
            ImageRef::new("example/runtime", digest),
            Err(SandboxError::InvalidSpec { field: "image", .. })
        ));
    }
    assert!(ByteSize::new(u64::MAX).is_err());
    assert!(CpuLimit::new(u32::MAX).is_err());
}

#[test]
fn sbx_c_015_docker_refuses_block_images_and_malformed_volume_names() -> Result<(), Box<dyn Error>>
{
    let mut spec = support::spec()?;
    for source in [
        WorkspaceSource::BlockImage(PathBuf::from("workspace.ext4")),
        WorkspaceSource::NamedVolume("../another".into()),
        WorkspaceSource::HostDirectory(PathBuf::from("relative")),
    ] {
        spec.workspace.source = source;
        assert!(matches!(
            spec.validate(BackendKind::Docker),
            Err(SandboxError::InvalidSpec {
                field: "workspace",
                ..
            })
        ));
    }
    Ok(())
}

#[test]
fn sbx_d_001_empty_or_unbounded_spec_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    let spec = support::spec()?;
    spec.validate(BackendKind::Docker)?;
    let mut invalid = spec.clone();
    invalid.pids = 0;
    assert!(invalid.validate(BackendKind::Docker).is_err());
    invalid = spec.clone();
    invalid
        .env
        .insert("TQA_GATEWAY_URL".into(), "http://elsewhere.invalid".into());
    assert!(invalid.validate(BackendKind::Docker).is_err());
    invalid = spec.clone();
    invalid.env.insert("NO_PROXY".into(), "*".into());
    assert!(invalid.validate(BackendKind::Docker).is_err());
    invalid = spec;
    invalid.gateway_url = "http://user:password@gateway.invalid/".parse()?;
    assert!(invalid.validate(BackendKind::Docker).is_err());
    Ok(())
}

#[test]
fn sbx_c_005_spec_and_backend_errors_redact_secrets() -> Result<(), Box<dyn Error>> {
    let spec = support::spec()?;
    assert!(!format!("{spec:?}").contains("sandbox-fixture-value"));
    let error = SandboxError::Backend("request body sandbox-fixture-value".into());
    assert!(!format!("{error:?} {error}").contains("sandbox-fixture-value"));
    assert!(error.retryable());
    assert!(SandboxError::Timeout.retryable());
    assert!(!SandboxError::Cancelled.retryable());
    assert!(!SandboxError::NotFound.retryable());
    assert_eq!(SandboxError::Cancelled.code(), "sandbox.cancelled");
    Ok(())
}
