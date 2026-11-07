//! PROTO-C-001 and PROTO-U-007: additive context events and old-frame compatibility.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ErrorCode, Event, MAX_TOOL_RESULT_BYTES, ServerFrame};

fn frame(event: &Value) -> Value {
    json!({"type":"event","seq":1,"run_id":"run","step":1,"ts":"2026-09-22T00:00:00Z","event":event})
}
#[test]
fn proto_c_001_old_step_started_defaults_attempt_to_one() -> Result<(), Box<dyn Error>> {
    let encoded = serde_json::to_vec(&frame(&json!({"type":"step.started","step":1})))?;
    assert!(matches!(
        ServerFrame::parse(&encoded)?,
        ServerFrame::Event {
            event: Event::StepStarted {
                step: 1,
                attempt: 1
            },
            ..
        }
    ));
    Ok(())
}
#[test]
fn proto_u_007_new_events_validate_messages_and_result_bytes() -> Result<(), Box<dyn Error>> {
    for message in [
        json!({"type":"message.user","text":"","attachments":[]}),
        json!({"type":"message.user","text":"ok","attachments":["bad/id"]}),
        json!({"type":"tool.result","call_id":"call","status":"ok"}),
    ] {
        assert_eq!(
            ServerFrame::parse(&serde_json::to_vec(&frame(&message))?)
                .err()
                .ok_or("expected error")?
                .code,
            ErrorCode::InvalidFrame
        );
    }
    let valid = json!({"type":"tool.result","call_id":"call","status":"ok","content":"x".repeat(MAX_TOOL_RESULT_BYTES)});
    assert!(serde_json::from_value::<Event>(valid).is_ok());
    let invalid = json!({"type":"tool.result","call_id":"call","status":"ok","content":"é".repeat(MAX_TOOL_RESULT_BYTES/2+1)});
    assert!(serde_json::from_value::<Event>(invalid).is_err());
    Ok(())
}

#[test]
fn proto_c_001_heartbeat_is_unsequenced_and_known() -> Result<(), Box<dyn Error>> {
    let heartbeat = tqa_types::ServerFrame::Heartbeat {
        run_id: "run".parse()?,
    };
    let encoded = serde_json::to_value(&heartbeat)?;
    assert_eq!(encoded, json!({"type":"heartbeat","run_id":"run"}));
    assert_eq!(
        ServerFrame::parse(&serde_json::to_vec(&encoded)?)?,
        heartbeat
    );
    let invalid = br#"{"type":"heartbeat","run_id":"bad/id"}"#;
    assert_eq!(
        ServerFrame::parse(invalid)
            .err()
            .ok_or("expected invalid frame")?
            .code,
        ErrorCode::InvalidFrame
    );
    let invalid = serde_json::to_vec(&frame(
        &json!({"type":"model.completed","step":1,"attempt":1,"finish_reason":"unknown"}),
    ))?;
    assert_eq!(
        ServerFrame::parse(&invalid)
            .err()
            .ok_or("expected invalid event")?
            .code,
        ErrorCode::InvalidFrame
    );
    Ok(())
}
