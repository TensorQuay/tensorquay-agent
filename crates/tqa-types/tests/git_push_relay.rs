//! TOOL-U-020 / SEC-019: the relay control API is bounded and cannot choose a destination.
use serde_json::json;
use std::error::Error;
use tqa_types::{GitPushAuthorization, GitPushOperationRequest, GitPushReceipt, GitPushRelayError};

#[test]
fn tool_u_020_relay_grants_have_only_an_opaque_id_and_bounded_start_lifetime()
-> Result<(), Box<dyn Error>> {
    let schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<GitPushAuthorization>();
    let validator = jsonschema::validator_for(&serde_json::to_value(schema)?)?;
    for lifetime in [1, 120_000] {
        let wire =
            json!({"status":"ready", "operation_id":"a".repeat(32), "expires_in_ms":lifetime});
        let grant: GitPushAuthorization = serde_json::from_value(wire.clone())?;
        assert!(validator.is_valid(&wire));
        assert_eq!(serde_json::to_value(grant)?, wire);
    }
    for wire in [
        json!({"status":"ready", "operation_id":"a".repeat(31), "expires_in_ms":1}),
        json!({"status":"ready", "operation_id":"A".repeat(32), "expires_in_ms":1}),
        json!({"status":"ready", "operation_id":"a".repeat(32), "expires_in_ms":0}),
        json!({"status":"ready", "operation_id":"a".repeat(32), "expires_in_ms":120_001}),
        json!({"status":"ready", "operation_id":"a".repeat(32), "expires_in_ms":1, "url":"https://elsewhere.example/repo"}),
        json!({"status":"already_at_commit", "credential":"forbidden"}),
        json!({"status":"success"}),
    ] {
        assert!(serde_json::from_value::<GitPushAuthorization>(wire.clone()).is_err());
        assert!(
            !validator.is_valid(&wire),
            "invalid relay grant passed its schema: {wire}"
        );
    }
    let wire = json!({"status":"already_at_commit"});
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<GitPushAuthorization>(
            wire.clone()
        )?)?,
        wire
    );
    Ok(())
}

#[test]
fn sec_019_relay_receipts_and_errors_cannot_carry_upstream_text() -> Result<(), Box<dyn Error>> {
    for status in ["ready", "uploading", "pushed", "unknown"] {
        let wire = json!({"status":status});
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<GitPushReceipt>(wire.clone())?)?,
            wire
        );
    }
    let rejected = json!({"status":"rejected", "code":"git_push_rejected"});
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<GitPushReceipt>(rejected.clone())?)?,
        rejected
    );
    for wire in [
        json!({"status":"pushed", "diagnostic":"forbidden"}),
        json!({"status":"rejected", "code":"new_unrecognized_code"}),
        json!({"status":"rejected", "code":"git_push_rejected", "message":"forbidden"}),
    ] {
        assert!(serde_json::from_value::<GitPushReceipt>(wire).is_err());
    }
    Ok(())
}

#[test]
fn sec_019_the_relay_has_no_branch_rules_or_approver_codes() {
    // ADR-0035: the git host decides; the relay neither protects branches nor asks who allowed the push.
    for code in [
        "git_push_protected_branch",
        "git_push_existing_branch",
        "push_decider_mismatch",
    ] {
        assert!(
            serde_json::from_value::<GitPushRelayError>(json!({"code":code})).is_err(),
            "{code}"
        );
    }
}

#[test]
fn sec_019_setup_carries_only_the_frozen_call_identity_and_arguments() -> Result<(), Box<dyn Error>>
{
    let wire = json!({"action_hash":"a".repeat(64), "context":{"conversation_id":"c", "run_id":"r",
        "step":1, "attempt":1, "call_id":"call"}, "arguments":{"remote":"https://git.example/repo",
        "branch":"feature/example", "commit":"b".repeat(40)}});
    let decoded: GitPushOperationRequest = serde_json::from_value(wire.clone())?;
    assert_eq!(serde_json::to_value(decoded)?, wire);
    for field in ["user_id", "secret_ref", "authorization", "approved"] {
        let mut invalid = wire.clone();
        invalid[field] = json!(true);
        assert!(serde_json::from_value::<GitPushOperationRequest>(invalid).is_err());
    }
    Ok(())
}
