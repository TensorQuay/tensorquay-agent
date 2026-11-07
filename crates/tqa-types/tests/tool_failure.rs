//! PROTO-U-031 (C1): a failed tool call's `tool.end` says why as data, a stable code and for a refused destination
//! where, so a client words the failure itself. The field is optional, and the Rust parser and the schema agree on it.
use serde_json::{Value, json};
use tqa_types::{ErrorTarget, Event, ServerFrame, ToolFailure, protocol_schema};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn frame(failure: Option<Value>) -> Value {
    let mut event =
        json!({"type": "tool.end", "call_id": "a", "status": "error", "exit_code": null});
    if let Some(failure) = failure {
        event["failure"] = failure;
    }
    json!({"type": "event", "seq": 1, "run_id": null, "step": null, "ts": "2026-09-30T00:00:00Z", "event": event})
}

// TEST: PROTO-U-031
#[test]
fn proto_u_031_tool_end_carries_an_optional_bounded_failure() -> TestResult {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ServerFrame");
    let validator = jsonschema::validator_for(&schema)?;
    for (failure, valid) in [
        (None, true),
        (
            Some(json!({"code": "egress_denied", "target": {"host": "example.com", "port": 443}})),
            true,
        ),
        (Some(json!({"code": "timeout"})), true),
        (Some(json!({"code": "Egress-Denied"})), false),
        (Some(json!({"code": ""})), false),
        (Some(json!({"code": "a".repeat(65)})), false),
        (
            Some(json!({"code": "egress_denied", "target": {"host": "", "port": 443}})),
            false,
        ),
    ] {
        let wire = frame(failure);
        assert_eq!(
            ServerFrame::parse(&serde_json::to_vec(&wire)?).is_ok(),
            valid,
            "{wire}"
        );
        assert_eq!(validator.is_valid(&wire), valid, "schema: {wire}");
    }
    // Written only when there is one: an older client's view of a successful call is unchanged.
    let failure = ToolFailure::new("egress_denied", Some(ErrorTarget::new("example.com", 443)?))?;
    let parsed = ServerFrame::parse(&serde_json::to_vec(&frame(Some(
        json!({"code": "egress_denied", "target": {"host": "example.com", "port": 443}}),
    )))?)?;
    let ServerFrame::Event {
        event: Event::ToolEnd { failure: read, .. },
        ..
    } = parsed
    else {
        return Err("not a tool.end".into());
    };
    assert_eq!(read, Some(failure));
    let plain = ServerFrame::parse(&serde_json::to_vec(&frame(None))?)?;
    assert!(!serde_json::to_string(&plain)?.contains("failure"));
    assert!(ToolFailure::new("Egress", None).is_err());
    Ok(())
}
