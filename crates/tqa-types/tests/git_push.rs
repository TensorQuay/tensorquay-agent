//! TOOL-U-020: a push names exactly one HTTPS remote, branch and full commit.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{GitPushTarget, WorkspaceSnapshot};

type TestResult = Result<(), Box<dyn Error>>;
fn target() -> Value {
    json!({"remote":"https://git.example/repo.git", "branch":"feature/example", "commit":"a".repeat(40)})
}
fn validator<T: schemars::JsonSchema>() -> Result<jsonschema::Validator, Box<dyn Error>> {
    let schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<T>();
    Ok(jsonschema::validator_for(&serde_json::to_value(schema)?)?)
}

#[test]
fn tool_u_020_target_wire_and_schema_reject_unsafe_or_ambiguous_inputs() -> TestResult {
    let validator = validator::<GitPushTarget>()?;
    for (key, values) in [
        (
            "remote",
            vec![
                json!("http://git.example/repo"),
                json!("ssh://git.example/repo"),
                json!("https://user@git.example/repo"),
                json!("https://git.example/repo?key=example"),
                json!(""),
            ],
        ),
        (
            "branch",
            vec![
                json!(""),
                json!("HEAD"),
                json!("refs/tags/release"),
                json!("-branch"),
                json!("a:b"),
                json!("a..b"),
                json!("a.lock"),
                json!("a\nb"),
                json!("a".repeat(257)),
            ],
        ),
        (
            "commit",
            vec![
                json!("HEAD"),
                json!("a".repeat(39)),
                json!("A".repeat(40)),
                json!("0".repeat(40)),
                json!("a".repeat(65)),
                json!(format!("{}\n", "a".repeat(40))),
                json!(format!("{}\r\n", "b".repeat(64))),
            ],
        ),
    ] {
        for replacement in values {
            let mut invalid = target();
            invalid[key] = replacement;
            assert!(
                serde_json::from_value::<GitPushTarget>(invalid.clone()).is_err(),
                "{invalid}"
            );
            assert!(!validator.is_valid(&invalid), "{invalid}");
        }
    }
    for field in ["force", "credential", "refspec", "approved"] {
        let mut invalid = target();
        invalid[field] = json!(true);
        assert!(serde_json::from_value::<GitPushTarget>(invalid.clone()).is_err());
        assert!(!validator.is_valid(&invalid));
    }
    for field in ["remote", "branch", "commit"] {
        let mut invalid = target();
        invalid.as_object_mut().ok_or("object")?.remove(field);
        assert!(serde_json::from_value::<GitPushTarget>(invalid.clone()).is_err());
        assert!(!validator.is_valid(&invalid));
    }
    Ok(())
}

#[test]
fn tool_u_020_target_bounds_are_inclusive_and_shared_by_schema() -> TestResult {
    let validator = validator::<GitPushTarget>()?;
    for commit_bytes in [40, 64] {
        for remote_bytes in [4095, 4096, 4097] {
            let prefix = "https://git.example/";
            let value = json!({"remote":format!("{prefix}{}", "a".repeat(remote_bytes-prefix.len())),
                "branch":"a".repeat(256), "commit":"b".repeat(commit_bytes)});
            assert_eq!(
                serde_json::from_value::<GitPushTarget>(value.clone()).is_ok(),
                remote_bytes <= 4096
            );
            assert_eq!(validator.is_valid(&value), remote_bytes <= 4096);
        }
    }
    let parsed: GitPushTarget = serde_json::from_value(target())?;
    parsed.validate()?;
    assert_eq!(serde_json::to_value(parsed)?, target());
    Ok(())
}

#[test]
fn tool_u_020_a_workspace_carries_no_push_settings() -> TestResult {
    // ADR-0035: the stored key and the git host decide; the workspace has no protected branches of its own.
    let decoded: WorkspaceSnapshot = serde_json::from_value(
        json!({"display":"/workspace", "clone":{"url":"https://git.example/repo"}}),
    )?;
    assert_eq!(decoded.display, "/workspace");
    let configured = json!({"display":"/workspace", "push":{"protected_branches":["release/*"]}});
    assert!(serde_json::from_value::<WorkspaceSnapshot>(configured).is_err());
    Ok(())
}
