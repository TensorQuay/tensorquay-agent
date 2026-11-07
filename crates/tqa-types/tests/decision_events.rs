//! PROTO-U-021: the `decision` and `decision.fallback` events (ADR-0013 point 4). A decision records the full applied
//! choice; a fallback has a closed reason code. Both are bounded, and the parser and the exported schema agree. They
//! reach the wire only with the protocol revision of ADR-0013 point 7, since only a non-default engine emits them.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    DecisionChoice, DecisionFallbackReason, DecisionPoint, Event, ServerFrame, protocol_schema,
};

type TestResult = Result<(), Box<dyn Error>>;

fn schema_accepts(value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/Event");
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}
fn decision(choice: &Value) -> Value {
    json!({"type":"decision","point":"before_step","engine":"router","choice":choice,"latency_ms":12})
}
fn fallback(reason: &str) -> Value {
    json!({"type":"decision.fallback","point":"after_response","engine":"router","reason":reason})
}

#[test]
fn proto_u_021_each_choice_and_reason_round_trips() -> TestResult {
    for (choice, kind) in [
        (
            json!({"kind":"call_model","route":"local","tools":["read_file","mcp.docs.search"]}),
            "call_model",
        ),
        (
            json!({"kind":"act","tool":"read_file","arguments":{"path":"a"}}),
            "act",
        ),
        (json!({"kind":"compact"}), "compact"),
        (
            json!({"kind":"stop","reason":"the budget is better spent elsewhere"}),
            "stop",
        ),
        (
            json!({"kind":"select","call_ids":["call-1","call-2"]}),
            "select",
        ),
    ] {
        let wire = decision(&choice);
        let parsed: Event = serde_json::from_value(wire.clone())?;
        assert!(
            matches!(
                &parsed,
                Event::Decision {
                    point: DecisionPoint::BeforeStep,
                    latency_ms: 12,
                    ..
                }
            ),
            "{parsed:?}"
        );
        assert_eq!(serde_json::to_value(&parsed)?, wire, "{kind}");
        assert!(schema_accepts(&wire)?, "{wire}");
    }
    for (code, reason) in [
        ("unreachable", DecisionFallbackReason::Unreachable),
        ("timeout", DecisionFallbackReason::Timeout),
        ("invalid", DecisionFallbackReason::Invalid),
        ("route_not_allowed", DecisionFallbackReason::RouteNotAllowed),
        ("tool_not_allowed", DecisionFallbackReason::ToolNotAllowed),
        ("schema", DecisionFallbackReason::Schema),
        ("repeated_compact", DecisionFallbackReason::RepeatedCompact),
        ("compact_nothing", DecisionFallbackReason::CompactNothing),
        ("act_loop", DecisionFallbackReason::ActLoop),
        ("circuit_open", DecisionFallbackReason::CircuitOpen),
        ("budget_spent", DecisionFallbackReason::BudgetSpent),
    ] {
        let wire = fallback(code);
        let parsed: Event = serde_json::from_value(wire.clone())?;
        assert!(
            matches!(&parsed, Event::DecisionFallback { reason: r, point: DecisionPoint::AfterResponse, .. } if *r == reason),
            "{parsed:?}"
        );
        assert_eq!(serde_json::to_value(&parsed)?, wire);
        assert!(schema_accepts(&wire)?, "{wire}");
    }
    let frame = json!({"type":"event","seq":1,"run_id":"r-1","step":2,"ts":"2026-09-28T00:00:00Z",
        "event":decision(&json!({"kind":"compact"}))});
    assert!(ServerFrame::parse(&serde_json::to_vec(&frame)?).is_ok());
    // The applied choice as the core builds it.
    let built = DecisionChoice::Stop {
        reason: "done".into(),
    };
    assert_eq!(
        serde_json::to_value(&built)?,
        json!({"kind":"stop","reason":"done"})
    );
    Ok(())
}

#[test]
fn proto_u_021_fields_are_bounded_and_codes_closed() -> TestResult {
    let long_reason = "r".repeat(201);
    let many_tools: Vec<String> = (0..257).map(|index| format!("tool_{index}")).collect();
    let many_calls: Vec<String> = (0..65).map(|index| format!("call-{index}")).collect();
    // Refused by the parser and by the exported schema.
    for invalid in [
        json!({"type":"decision","point":"during","engine":"router","choice":{"kind":"compact"},"latency_ms":1}),
        json!({"type":"decision","point":"before_step","engine":"","choice":{"kind":"compact"},"latency_ms":1}),
        json!({"type":"decision","point":"before_step","engine":"router","choice":{"kind":"compact"},"latency_ms":-1}),
        decision(&json!({"kind":"wander"})),
        decision(&json!({"kind":"stop","reason":long_reason})),
        decision(&json!({"kind":"stop","reason":""})),
        decision(&json!({"kind":"call_model","route":"","tools":[]})),
        decision(&json!({"kind":"call_model","route":"local","tools":["Read File"]})),
        decision(&json!({"kind":"call_model","route":"local","tools":many_tools})),
        decision(&json!({"kind":"act","tool":"Read File","arguments":{}})),
        decision(&json!({"kind":"act","tool":"read_file","arguments":"a"})),
        decision(&json!({"kind":"select","call_ids":many_calls})),
        fallback("gave_up"),
        fallback(""),
    ] {
        assert!(
            serde_json::from_value::<Event>(invalid.clone()).is_err(),
            "{invalid}"
        );
        assert!(!schema_accepts(&invalid)?, "{invalid}");
    }
    // Refused by the parser; a schema can't state these bounds.
    let big = "x".repeat(64 * 1024);
    for invalid in [
        decision(&json!({"kind":"stop","reason":"line\nbreak"})),
        decision(&json!({"kind":"act","tool":"read_file","arguments":{"content":big}})),
    ] {
        assert!(
            serde_json::from_value::<Event>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
    // At the bounds.
    assert!(
        serde_json::from_value::<Event>(decision(&json!({"kind":"stop","reason":"r".repeat(200)})))
            .is_ok()
    );
    let tools: Vec<String> = (0..256).map(|index| format!("tool_{index}")).collect();
    assert!(
        serde_json::from_value::<Event>(decision(
            &json!({"kind":"call_model","route":"local","tools":tools})
        ))
        .is_ok()
    );
    Ok(())
}
