//! PROTO-U-017: the `mcp.server` event (ADR-0011): an MCP server's state for the session, emitted around a run's start.
//! Additive and revision-free; bounded like the configuration it reports on.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{Event, McpServerState, ServerFrame, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn schema_accepts(value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/Event");
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}
fn event(name: Value, state: &str, tools: Value, reason: Option<Value>) -> Value {
    let mut event = json!({"type":"mcp.server","state":state});
    event["name"] = name;
    event["tools"] = tools;
    if let Some(reason) = reason {
        event["reason"] = reason;
    }
    event
}

#[test]
fn proto_u_017_each_state_round_trips_with_and_without_a_reason() -> TestResult {
    for (state, expected) in [
        ("starting", McpServerState::Starting),
        ("ready", McpServerState::Ready),
        ("restarted", McpServerState::Restarted),
        ("disabled", McpServerState::Disabled),
    ] {
        for reason in [None, Some(json!("handshake_timeout"))] {
            let wire = event(json!("docs"), state, json!(3), reason);
            let parsed: Event = serde_json::from_value(wire.clone())?;
            assert!(
                matches!(&parsed, Event::McpServer { state: s, .. } if *s == expected),
                "{parsed:?}"
            );
            assert_eq!(serde_json::to_value(&parsed)?, wire);
            assert!(schema_accepts(&wire)?, "{wire}");
        }
    }
    let frame = json!({"type":"event","seq":1,"run_id":null,"step":null,"ts":"2026-09-27T00:00:00Z",
        "event":event(json!("docs"), "ready", json!(0), None)});
    assert!(ServerFrame::parse(&serde_json::to_vec(&frame)?).is_ok());
    Ok(())
}

#[test]
fn proto_u_017_names_reasons_states_and_counts_are_bounded() -> TestResult {
    for invalid in [
        event(json!("Docs"), "ready", json!(1), None),
        event(json!("a__b"), "ready", json!(1), None),
        event(json!(""), "ready", json!(1), None),
        event(json!("docs"), "running", json!(1), None),
        event(json!("docs"), "ready", json!(-1), None),
        event(json!("docs"), "ready", json!(65), None),
        event(
            json!("docs"),
            "disabled",
            json!(0),
            Some(json!("r".repeat(65))),
        ),
        event(json!("docs"), "disabled", json!(0), Some(json!("Crashed!"))),
        event(json!("docs"), "disabled", json!(0), Some(json!(""))),
    ] {
        assert!(
            serde_json::from_value::<Event>(invalid.clone()).is_err(),
            "{invalid}"
        );
        assert!(!schema_accepts(&invalid)?, "{invalid}");
    }
    assert!(
        serde_json::from_value::<Event>(event(json!("docs"), "ready", json!(64), None)).is_ok()
    );
    assert!(
        serde_json::from_value::<Event>(event(
            json!("docs"),
            "disabled",
            json!(0),
            Some(json!("r".repeat(64)))
        ))
        .is_ok()
    );
    Ok(())
}
