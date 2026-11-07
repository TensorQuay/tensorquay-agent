//! PROTO-C-001: optional workspace clone configuration remains additive.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ErrorCode, Event, WorkspaceClone, WorkspaceSnapshot, protocol_schema};

fn validator() -> Result<jsonschema::Validator, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/WorkspaceSnapshot");
    Ok(jsonschema::validator_for(&schema)?)
}

#[test]
fn proto_c_001_workspace_clone_round_trips_url_and_branch_or_tag() -> Result<(), Box<dyn Error>> {
    let validator = validator()?;
    for reference in [None, Some("feature/example"), Some("v1.0.0")] {
        let mut value = json!({
            "display":"/workspace", "clone":{"url":"https://git.example/repository.git"}
        });
        if let Some(reference) = reference {
            value["clone"]["ref"] = json!(reference);
        }
        let parsed: WorkspaceSnapshot = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(parsed)?, value);
        assert!(validator.is_valid(&value));
    }
    Ok(())
}

#[test]
fn proto_c_001_workspace_clone_absence_preserves_legacy_config() -> Result<(), Box<dyn Error>> {
    let value = json!({"display":"/workspace"});
    let parsed: WorkspaceSnapshot = serde_json::from_value(value.clone())?;
    assert_eq!(serde_json::to_value(parsed)?, value);
    assert!(validator()?.is_valid(&value));
    Ok(())
}

#[test]
fn proto_c_001_workspace_clone_refuses_non_https_transports() {
    for url in [
        "http://git.example/repository.git",
        "ssh://git.example/repository.git",
        "file:///repository",
        "git.example:repository.git",
    ] {
        let value: Value = json!({"display":"/workspace", "clone":{"url":url}});
        assert!(
            serde_json::from_value::<WorkspaceSnapshot>(value).is_err(),
            "{url}"
        );
    }
}

#[test]
fn proto_u_003_workspace_clone_validates_public_https_authorities() -> Result<(), Box<dyn Error>> {
    for url in [
        "https://git.example/repo.git",
        "https://git.example:65535/repo",
        "https://[2001:db8::1]:443/repo",
        "https://git.example/a%20b.git",
    ] {
        let value = WorkspaceClone {
            url: url.into(),
            r#ref: None,
        };
        value.validate()?;
        let decoded: WorkspaceClone = serde_json::from_value(serde_json::to_value(&value)?)?;
        assert_eq!(decoded, value);
    }
    for url in [
        "",
        "https://git.example",
        "https://git.example/",
        "https:///repo",
        "https://user@git.example/repo",
        "https://user:pass@git.example/repo",
        "https://git.example/repo?token=synthetic",
        "https://git.example/repo#main",
        "https://git.example:0/repo",
        "https://git.example:65536/repo",
        "https://git.example:+1/repo",
        "https://[git.example]/repo",
        "https://[2001:db8::no]/repo",
        "https://2001:db8::1:443/repo",
        "https://git..example/repo",
        "https://-git.example/repo",
        "https://git.example\\outside/repo",
        "https://git.example/line\nbreak",
        "https://git.example/zero\0byte",
        "https://git.example/a b",
        "https://git.example/非ascii",
        "https://git.example/%zz",
        "https://git.example/<repo>",
    ] {
        let value = WorkspaceClone {
            url: url.into(),
            r#ref: None,
        };
        let error = value
            .validate()
            .err()
            .ok_or("accepted an invalid clone URL")?;
        assert_eq!(error.code, ErrorCode::InvalidFrame);
        assert_eq!(error.path, "workspace.clone.url");
        assert!(
            serde_json::from_value::<WorkspaceClone>(serde_json::to_value(value)?).is_err(),
            "{url:?}"
        );
    }
    Ok(())
}

#[test]
fn proto_u_003_workspace_clone_url_length_is_inclusive_in_wire_and_schema()
-> Result<(), Box<dyn Error>> {
    let validator = validator()?;
    for length in [4095, 4096, 4097] {
        let prefix = "https://git.example/";
        let url = format!("{prefix}{}", "a".repeat(length - prefix.len()));
        let value = WorkspaceClone { url, r#ref: None };
        let valid = length <= 4096;
        assert_eq!(value.validate().is_ok(), valid);
        let wire = json!({"display":"/workspace", "clone":value});
        assert_eq!(
            serde_json::from_value::<WorkspaceSnapshot>(wire.clone()).is_ok(),
            valid
        );
        assert_eq!(validator.is_valid(&wire), valid);
    }
    Ok(())
}

#[test]
fn proto_u_003_workspace_clone_ref_accepts_names_and_rejects_revision_expressions()
-> Result<(), Box<dyn Error>> {
    for (reference, valid) in [
        ("a", true),
        ("feature/example", true),
        ("v1.0.0", true),
        ("main.LOCK", true),
        ("", false),
        ("-branch", false),
        (".", false),
        (".hidden", false),
        ("a/.hidden", false),
        ("a..b", false),
        ("a.lock/b", false),
        ("a.lock", false),
        ("a//b", false),
        ("/a", false),
        ("a/", false),
        ("a.", false),
        ("a b", false),
        ("a\nb", false),
        ("a\\b", false),
        ("a~1", false),
        ("a^1", false),
        ("a:b", false),
        ("a?", false),
        ("a*", false),
        ("a[", false),
        ("a@{1}", false),
        ("@", false),
        ("非ascii", false),
    ] {
        check_reference(reference.into(), valid)?;
    }
    check_reference("a".repeat(256), true)?;
    check_reference("a".repeat(257), false)?;
    Ok(())
}

fn check_reference(reference: String, valid: bool) -> Result<(), Box<dyn Error>> {
    let value = WorkspaceClone {
        url: "https://git.example/repo".into(),
        r#ref: Some(reference),
    };
    assert_eq!(value.validate().is_ok(), valid, "{value:?}");
    let wire = json!({"display":"/workspace", "clone":value});
    assert_eq!(
        serde_json::from_value::<WorkspaceSnapshot>(wire.clone()).is_ok(),
        valid,
        "{wire}"
    );
    assert_eq!(validator()?.is_valid(&wire), valid, "{wire}");
    Ok(())
}

#[test]
fn proto_u_003_workspace_clone_fields_are_exact() -> Result<(), Box<dyn Error>> {
    for clone in [
        json!({"url":"https://git.example/repo", "unexpected":true}),
        json!({"ref":"main"}),
        json!({"url":123}),
    ] {
        let wire = json!({"display":"/workspace", "clone":clone});
        assert!(serde_json::from_value::<WorkspaceSnapshot>(wire.clone()).is_err());
        assert!(!validator()?.is_valid(&wire));
    }
    Ok(())
}

#[test]
fn proto_c_001_workspace_clone_status_round_trips_and_ignores_future_fields()
-> Result<(), Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/Event");
    let validator = jsonschema::validator_for(&schema)?;
    for status in ["started", "completed", "skipped", "failed"] {
        let mut value =
            json!({"type":"workspace.clone", "status":status, "message":"Repository clone status"});
        if status == "failed" {
            value["code"] = json!("egress_denied");
        }
        let mut future = value.clone();
        future["future"] = json!(true);
        let decoded: Event = serde_json::from_value(future)?;
        assert_eq!(serde_json::to_value(decoded)?, value);
        assert!(validator.is_valid(&value));
    }
    for message in [String::new(), "x".repeat(257), "bad\nstatus".into()] {
        let wire = json!({"type":"workspace.clone", "status":"failed", "message":message});
        assert!(serde_json::from_value::<Event>(wire.clone()).is_err());
        assert!(!validator.is_valid(&wire));
    }
    let wire = json!({"type":"workspace.clone", "status":"completed", "message":"x".repeat(256)});
    assert!(serde_json::from_value::<Event>(wire.clone()).is_ok());
    assert!(validator.is_valid(&wire));
    Ok(())
}
