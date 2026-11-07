//! PROTO-U-004 and PROTO-U-006 sequence and protocol negotiation contracts.

use std::error::Error;

use tqa_types::{ErrorCode, Seq, SeqChecker, SeqError, check_protocol};

#[test]
fn proto_u_004_sequence_gap_does_not_advance_checker() -> Result<(), Box<dyn Error>> {
    let mut checker = SeqChecker::new(Seq::ZERO);
    for value in [1, 2, 3] {
        checker.accept(Seq::from(value))?;
    }
    assert_eq!(
        checker.accept(Seq::from(5)),
        Err(SeqError::Gap {
            expected: Seq::from(4),
            got: Seq::from(5),
        })
    );
    checker.accept(Seq::from(4))?;
    Ok(())
}

#[test]
fn proto_u_004_sequence_exhaustion_never_wraps() -> Result<(), Box<dyn Error>> {
    let mut checker = SeqChecker::new(Seq::from(u64::MAX - 1));
    checker.accept(Seq::from(u64::MAX))?;
    assert_eq!(checker.accept(Seq::ZERO), Err(SeqError::Exhausted));
    assert_eq!(
        checker.accept(Seq::from(u64::MAX)),
        Err(SeqError::Exhausted)
    );
    assert_eq!(Seq::from(u64::MAX).next(), None);
    assert_eq!(Seq::ZERO.next(), Some(Seq::from(1)));
    assert!(Seq::from(1) < Seq::from(2));
    assert_eq!(Seq::ZERO.to_string(), "0");
    assert_eq!(SeqError::Exhausted.code(), ErrorCode::Internal);
    assert_eq!(
        SeqError::Gap {
            expected: Seq::from(4),
            got: Seq::from(5)
        }
        .code(),
        ErrorCode::SeqGap
    );
    Ok(())
}

#[test]
fn proto_u_006_protocol_negotiation_lists_supported_versions() -> Result<(), Box<dyn Error>> {
    check_protocol(1)?;
    let error = check_protocol(2)
        .err()
        .ok_or("accepted unsupported protocol")?;
    assert_eq!(error.requested, 2);
    assert_eq!(error.supported, vec![1]);
    assert_eq!(error.code(), ErrorCode::UnsupportedProtocol);
    Ok(())
}

#[test]
fn proto_c_001_error_codes_have_stable_strings_and_retry_defaults() -> Result<(), Box<dyn Error>> {
    let cases = [
        (ErrorCode::UnknownFrame, "unknown_frame", false),
        (ErrorCode::InvalidFrame, "invalid_frame", false),
        (ErrorCode::SeqGap, "seq_gap", true),
        (
            ErrorCode::UnsupportedProtocol,
            "unsupported_protocol",
            false,
        ),
        (ErrorCode::Busy, "busy", true),
        (ErrorCode::PermissionBinding, "permission_binding", false),
        (ErrorCode::EgressDenied, "egress_denied", false),
        (ErrorCode::Restarting, "restarting", true),
        (ErrorCode::StoreUnavailable, "store_unavailable", true),
        (ErrorCode::ProviderError, "provider_error", true),
        (ErrorCode::Budget, "budget", false),
        (ErrorCode::RepeatedFailure, "repeated_failure", false),
        (ErrorCode::NoProgress, "no_progress", false),
        (ErrorCode::FallbackParse, "fallback_parse", false),
        (ErrorCode::Io, "io", false),
        (ErrorCode::Internal, "internal", false),
        (ErrorCode::SandboxFailed, "sandbox_failed", true),
        (ErrorCode::RuntimeStartFailed, "runtime_start_failed", true),
        (ErrorCode::ActionFailed, "action_failed", true),
        (ErrorCode::WorkspaceQuota, "workspace_quota", false),
        (ErrorCode::StateQuota, "state_quota", false),
        (ErrorCode::DecisionStop, "decision_stop", false),
        (
            ErrorCode::DecisionEngineUnavailable,
            "decision_engine_unavailable",
            true,
        ),
        (
            ErrorCode::DecisionEngineBudgetSpent,
            "decision_engine_budget_spent",
            false,
        ),
        (ErrorCode::ApprovalsRemoved, "approvals_removed", false),
        (ErrorCode::ReplyCut, "reply_cut", false),
    ];
    for (code, wire, retryable) in cases {
        assert_eq!(code.as_str(), wire);
        assert_eq!(code.to_string(), wire);
        assert_eq!(code.retryable(), retryable);
        assert_eq!(serde_json::to_value(code)?, wire);
        assert_eq!(
            serde_json::from_value::<ErrorCode>(serde_json::json!(wire))?,
            code
        );
    }
    assert!(serde_json::from_str::<ErrorCode>("\"future_code\"").is_err());
    Ok(())
}

#[test]
fn proto_c_001_action_failure_codes_are_additive_and_schema_valid() -> Result<(), Box<dyn Error>> {
    let mut schema = tqa_types::protocol_schema();
    schema["$ref"] = serde_json::json!("#/definitions/ServerFrame");
    let validator = jsonschema::validator_for(&schema)?;
    for wire in ["runtime_start_failed", "action_failed"] {
        let frame = serde_json::json!({"type":"error", "seq":null, "code":wire,
            "message":"Action failed; retry after recovery", "retryable":true});
        let parsed = tqa_types::ServerFrame::parse(&serde_json::to_vec(&frame)?)?;
        assert_eq!(serde_json::to_value(&parsed)?, frame);
        assert!(validator.is_valid(&frame));
        let code: ErrorCode = serde_json::from_value(serde_json::json!(wire))?;
        assert_eq!(code.as_str(), wire);
        assert!(code.retryable());
    }
    Ok(())
}
