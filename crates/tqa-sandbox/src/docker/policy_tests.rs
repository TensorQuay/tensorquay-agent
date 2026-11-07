use super::{
    DockerBackendConfig, DockerPlatform, NetworkMode, SeccompStatus, image_policy::validate_image,
    network_policy::validate_network, preflight::assess, records,
};
use crate::{EgressMode, SandboxError};
use bollard::models::{ContainerSummary, ImageInspect, NetworkInspect, SystemInfo, SystemVersion};
use serde_json::json;
use std::{
    error::Error,
    time::{Duration, SystemTime},
};

#[path = "../../tests/support/spec.rs"]
pub(super) mod support;
type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn sbx_d_001_storage_roots_reject_relative_root_and_traversal_paths() {
    for root in ["/", "relative", "/data/../storage"] {
        assert!(DockerBackendConfig::new("tqa-unit", root).is_err());
    }
}

#[test]
fn sbx_d_001_backend_configuration_rejects_unbounded_and_unknown_fields() -> TestResult {
    let config = DockerBackendConfig::new("tqa-unit", "/sandbox-storage")?;
    config.validate()?;
    for duration in [Duration::ZERO, Duration::from_mins(11)] {
        let mut invalid = config.clone();
        invalid.operation_timeout = duration;
        assert!(invalid.validate().is_err());
        let mut invalid = config.clone();
        invalid.destroy_timeout = duration;
        assert!(
            invalid.validate().is_err(),
            "the teardown bound is bounded too"
        );
        let mut invalid = config.clone();
        invalid.create_timeout = duration;
        assert!(
            invalid.validate().is_err(),
            "the setup bound is bounded too"
        );
        let mut invalid = config.clone();
        invalid.create_budget = duration;
        assert!(
            invalid.validate().is_err(),
            "the create budget is bounded too (SBX-U-014)"
        );
    }
    // Teardown keeps its own bound: the stop's 10-second grace fits it, whatever operation_timeout an embedder sets.
    assert_eq!(
        config.destroy_timeout,
        crate::docker::DEFAULT_DESTROY_TIMEOUT
    );
    assert!(config.destroy_timeout > Duration::from_secs(10));
    // Setup keeps its own, longer bound (SBX-U-009).
    assert_eq!(config.create_timeout, crate::docker::DEFAULT_CREATE_TIMEOUT);
    assert!(config.create_timeout > config.operation_timeout);
    // One start's creates share a budget longer than one create's bound, so a timed-out create has room for a retry.
    assert_eq!(config.create_budget, crate::DEFAULT_CREATE_BUDGET);
    assert!(config.create_budget > config.create_timeout);
    for size in [0, 65_537] {
        let mut invalid = config.clone();
        invalid.max_containers = size;
        assert!(invalid.validate().is_err());
    }
    for command in [
        vec![],
        vec![String::new()],
        vec!["x".repeat(8193)],
        vec!["a\0b".into()],
        vec!["x".into(); 33],
    ] {
        let mut invalid = config.clone();
        invalid.test_command_override = Some(command);
        assert!(invalid.validate().is_err());
    }
    let mut valid = config.clone();
    valid.test_command_override = Some(vec!["true".into()]);
    valid.validate()?;
    let mut value = serde_json::to_value(config)?;
    value["unknown"] = json!(true);
    assert!(serde_json::from_value::<DockerBackendConfig>(value).is_err());
    Ok(())
}

#[test]
fn sbx_d_001_preflight_distinguishes_capabilities_and_fails_closed() -> TestResult {
    let version: SystemVersion =
        serde_json::from_value(json!({"Os":"linux", "ApiVersion":"1.47"}))?;
    let info: SystemInfo = serde_json::from_value(
        json!({"Plugins":{"Network":["bridge"]}, "SecurityOptions":["name=seccomp,profile=builtin", "name=userns"], "OperatingSystem":"Linux"}),
    )?;
    let preflight = assess(version.clone(), info.clone())?;
    assert_eq!(preflight.platform, DockerPlatform::LinuxEngine);
    assert_eq!(preflight.default_seccomp, SeccompStatus::Builtin);
    assert!(preflight.user_namespaces && preflight.internal_networks);
    assert_eq!(
        preflight.enforceable_egress,
        vec![EgressMode::Internal, EgressMode::Proxy]
    );
    for (profile, expected) in [
        ("unconfined", SeccompStatus::Unconfined),
        ("custom", SeccompStatus::Custom),
    ] {
        let mut desktop = info.clone();
        desktop.operating_system = Some("Docker Desktop".into());
        desktop.security_options = Some(vec![format!("name=seccomp,profile={profile}")]);
        let actual = assess(version.clone(), desktop)?;
        assert_eq!(actual.platform, DockerPlatform::DockerDesktop);
        assert_eq!(actual.default_seccomp, expected);
        assert!(!actual.user_namespaces);
    }
    let mut invalid = version.clone();
    invalid.os = Some("windows".into());
    assert!(matches!(
        assess(invalid, info.clone()),
        Err(SandboxError::Preflight(_))
    ));
    let mut invalid = version.clone();
    invalid.api_version = None;
    assert!(assess(invalid, info.clone()).is_err());
    let mut invalid = info.clone();
    invalid.plugins = None;
    assert!(assess(version.clone(), invalid).is_err());
    let mut invalid = info;
    invalid.security_options = None;
    assert!(assess(version, invalid).is_err());
    Ok(())
}

// TEST: SBX-C-014
#[test]
fn sbx_d_004_image_policy_rejects_digest_mismatch_implicit_mounts_and_environment() -> TestResult {
    let spec = support::spec()?;
    let value = json!({"RepoDigests":[spec.image.to_string()], "Config":{"Env":["LANG=C"]}});
    validate_image(&serde_json::from_value(value.clone())?, &spec)?;
    let mut invalid: ImageInspect = serde_json::from_value(value.clone())?;
    invalid.repo_digests = None;
    assert_eq!(
        validate_image(&invalid, &spec),
        Err(SandboxError::ImageDigestMismatch)
    );
    for config in [
        json!({"Volumes":{"/unexpected":{}}}),
        json!({"Env":["EXTRA=value"]}),
        json!({"Env":["MALFORMED"]}),
    ] {
        let mut invalid = value.clone();
        invalid["Config"] = config;
        assert!(matches!(
            validate_image(&serde_json::from_value(invalid)?, &spec),
            Err(SandboxError::InvalidSpec { field: "image", .. })
        ));
    }
    let mut no_config = value;
    no_config["Config"] = json!(null);
    validate_image(&serde_json::from_value(no_config)?, &spec)?;
    Ok(())
}

/// The refusal names the image's unset environment keys, never a value or a key the spec sets, within bounds.
// TEST: SBX-C-014
#[test]
fn sbx_u_002_unset_image_environment_is_refused_by_key_name_only() -> TestResult {
    let spec = support::spec()?;
    let image = |env: Vec<String>| -> Result<ImageInspect, Box<dyn Error>> {
        let value = json!({"RepoDigests":[spec.image.to_string()], "Config":{"Env":env}});
        Ok(serde_json::from_value(value)?)
    };
    let reason = |image: &ImageInspect| match validate_image(image, &spec) {
        Err(SandboxError::InvalidSpec {
            field: "image",
            reason,
        }) => Ok(reason.into_owned()),
        other => Err(format!("expected an image refusal, got {other:?}")),
    };
    let env = ["LANG=C", "HOME=/image-home", "PATH=/image-bin", "MALFORMED"];
    let named = reason(&image(env.map(String::from).to_vec())?)?;
    assert!(
        named.ends_with(": HOME, PATH, an entry without '='"),
        "{named}"
    );
    for absent in ["LANG", "/image-home", "/image-bin", "MALFORMED"] {
        assert!(!named.contains(absent), "{named}");
    }
    let many: Vec<String> = (0..20).map(|index| format!("KEY_{index:02}=v")).collect();
    let listed = reason(&image(many)?)?;
    assert!(listed.ends_with("KEY_07, and 12 more"), "{listed}");
    let long = reason(&image(vec![
        format!("{}=v", "K".repeat(300)),
        "BAD\u{7}KEY=v".into(),
    ])?)?;
    assert!(long.contains(&format!("{}...", "K".repeat(64))), "{long}");
    assert!(
        !long.contains(&"K".repeat(65)) && long.contains("BAD?KEY"),
        "{long}"
    );
    Ok(())
}

#[test]
fn sbx_c_010_local_image_policy_checks_id_and_preserves_mount_and_environment_guards() -> TestResult
{
    let mut spec = support::spec()?;
    let digest = format!("sha256:{}", "a".repeat(64));
    spec.image = crate::ImageRef::local_id(&digest)?;
    let valid = json!({"Id":digest, "Config":{"Env":["LANG=C"]}});
    validate_image(&serde_json::from_value(valid.clone())?, &spec)?;
    for id in [json!(null), json!(format!("sha256:{}", "b".repeat(64)))] {
        let mut invalid = valid.clone();
        invalid["Id"] = id;
        invalid["RepoDigests"] = json!([format!("example/runtime@{digest}")]);
        assert_eq!(
            validate_image(&serde_json::from_value(invalid)?, &spec),
            Err(SandboxError::ImageDigestMismatch)
        );
    }
    for config in [
        json!({"Volumes":{"/unexpected":{}}}),
        json!({"Env":["EXTRA=value"]}),
    ] {
        let mut invalid = valid.clone();
        invalid["Config"] = config;
        assert!(matches!(
            validate_image(&serde_json::from_value(invalid)?, &spec),
            Err(SandboxError::InvalidSpec { field: "image", .. })
        ));
    }
    Ok(())
}

#[test]
fn sbx_d_001_existing_network_must_match_ownership_and_policy() -> TestResult {
    let network: NetworkInspect = serde_json::from_value(
        json!({"Driver":"bridge", "Internal":true, "EnableIPv6":false, "Labels":{"tqa.instance":"tqa-unit"}}),
    )?;
    validate_network(&network, "tqa-unit", NetworkMode::Internal)?;
    assert!(validate_network(&network, "other", NetworkMode::Internal).is_err());
    assert!(validate_network(&network, "tqa-unit", NetworkMode::DevHostGateway).is_err());
    for field in ["Driver", "Internal", "EnableIPv6", "Labels"] {
        let mut value = serde_json::to_value(&network)?;
        value[field] = json!(null);
        assert!(
            validate_network(
                &serde_json::from_value(value)?,
                "tqa-unit",
                NetworkMode::Internal
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn sbx_c_008_container_records_reject_missing_or_malformed_identity() -> TestResult {
    let value = json!({"Id":"sandbox-id", "Labels":{"tqa.conversation":"conversation_record", "tqa.created_at":"123"}});
    let handle = records::handle(serde_json::from_value(value.clone())?)?;
    assert_eq!(
        handle.created_at,
        SystemTime::UNIX_EPOCH + Duration::from_secs(123)
    );
    assert_eq!(handle.id, "sandbox-id");
    for pointer in [
        "/Id",
        "/Labels",
        "/Labels/tqa.conversation",
        "/Labels/tqa.created_at",
    ] {
        let mut invalid = value.clone();
        *invalid.pointer_mut(pointer).ok_or("missing test pointer")? = json!(null);
        let decoded = serde_json::from_value::<ContainerSummary>(invalid);
        assert!(decoded.is_err() || decoded.is_ok_and(|record| records::handle(record).is_err()));
    }
    for timestamp in ["not-a-number", "18446744073709551615"] {
        let mut invalid = value.clone();
        invalid["Labels"]["tqa.created_at"] = json!(timestamp);
        assert!(records::handle(serde_json::from_value(invalid)?).is_err());
    }
    let mut invalid = value;
    invalid["Labels"]["tqa.conversation"] = json!("bad/id");
    assert!(records::handle(serde_json::from_value(invalid)?).is_err());
    Ok(())
}

#[test]
fn sbx_c_003_stats_require_resource_samples_and_convert_cpu_units() -> TestResult {
    let value = json!({"cpu_stats":{"cpu_usage":{"total_usage":12_345_678}}, "memory_stats":{"usage":1234}, "pids_stats":{"current":2}});
    let actual = records::stats(
        serde_json::from_value(value.clone())?,
        Duration::from_secs(5),
    )?;
    assert_eq!(
        (
            actual.cpu_millis,
            actual.memory_bytes,
            actual.pids,
            actual.uptime
        ),
        (12, 1234, 2, Duration::from_secs(5))
    );
    for field in ["cpu_stats", "memory_stats", "pids_stats"] {
        let mut invalid = value.clone();
        invalid[field] = json!(null);
        assert!(records::stats(serde_json::from_value(invalid)?, Duration::ZERO).is_err());
    }
    Ok(())
}

#[test]
fn sec_018_development_network_requires_an_exact_explicit_opt_in() -> TestResult {
    let mut config = DockerBackendConfig::new("tqa-unit", "/sandbox-storage")?;
    assert_eq!(config.network, NetworkMode::Internal);
    config.check_network_opt_in(None)?;
    config.network = NetworkMode::DevHostGateway;
    for value in [None, Some(""), Some("0"), Some("true")] {
        assert!(config.check_network_opt_in(value).is_err());
    }
    config.check_network_opt_in(Some("1"))?;
    Ok(())
}

#[test]
fn sec_018_network_requests_are_internal_unless_development_was_selected() {
    for mode in [NetworkMode::Internal, NetworkMode::DevHostGateway] {
        let request = super::network_policy::request("tqa-unit", mode);
        assert_eq!(request.internal, Some(mode == NetworkMode::Internal));
        assert_eq!(request.enable_ipv6, Some(false));
    }
}
