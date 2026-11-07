//! PROTO-C-001 / PROTO-U-003: queue positions are bounded, transient server metadata.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ServerFrame, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn ready(state: &str) -> Value {
    json!({"type":"session.ready","conversation_id":"conversation","resumed":false,
        "sandbox":{"backend":"docker","state":state}})
}

fn queued(queue: Value) -> Value {
    let mut wire = ready("queued");
    wire["queue"] = queue;
    wire
}

fn invalid_positions() -> Vec<Value> {
    let mut values = vec![json!({}), json!(false), json!([]), json!("next")];
    for ahead in [
        json!(-1),
        json!(10_001),
        json!(u64::MAX),
        json!(0.5),
        json!("0"),
        json!(false),
        Value::Null,
    ] {
        values.push(json!({"ahead":ahead}));
    }
    values
}

#[test]
fn proto_c_001_queued_ready_round_trips_position_boundaries_without_a_sequence() -> TestResult {
    for backend in ["docker", "firecracker"] {
        for ahead in [0, 1, 10_000] {
            let mut wire = queued(json!({"ahead":ahead}));
            wire["sandbox"]["backend"] = json!(backend);
            let frame = ServerFrame::parse(&serde_json::to_vec(&wire)?)?;
            let encoded = serde_json::to_value(frame)?;
            assert_eq!(encoded, wire);
            assert!(encoded.get("seq").is_none());
        }
    }
    Ok(())
}

#[test]
fn proto_u_003_queue_position_rejects_invalid_counts_and_shapes() -> TestResult {
    for queue in invalid_positions() {
        let wire = queued(queue);
        assert!(
            ServerFrame::parse(&serde_json::to_vec(&wire)?).is_err(),
            "{wire}"
        );
        assert!(
            serde_json::from_value::<ServerFrame>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    Ok(())
}

#[test]
fn proto_c_001_ready_without_queue_preserves_legacy_frames_and_omits_null() -> TestResult {
    for state in [
        "creating",
        "ready",
        "busy",
        "idle",
        "parked",
        "destroyed",
        "failed",
        "queued",
    ] {
        let old = ready(state);
        let frame = ServerFrame::parse(&serde_json::to_vec(&old)?)?;
        assert_eq!(serde_json::to_value(frame)?, old);
        let mut nullable = old.clone();
        nullable["queue"] = Value::Null;
        let frame = ServerFrame::parse(&serde_json::to_vec(&nullable)?)?;
        assert_eq!(serde_json::to_value(frame)?, old);
    }
    Ok(())
}

#[test]
fn proto_u_003_queue_schema_enforces_integer_bounds_and_keeps_legacy_frames() -> TestResult {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ServerFrame");
    let validator = jsonschema::validator_for(&schema)?;
    for ahead in [0, 1, 10_000] {
        assert!(validator.is_valid(&queued(json!({"ahead":ahead}))));
    }
    for invalid in invalid_positions() {
        let wire = queued(invalid);
        assert!(!validator.is_valid(&wire), "schema accepted {wire}");
    }
    assert!(validator.is_valid(&ready("ready")));
    assert!(validator.is_valid(&ready("queued")));
    Ok(())
}

#[test]
fn proto_c_001_queue_ignores_future_server_metadata() -> TestResult {
    let expected = queued(json!({"ahead":2}));
    let mut wire = expected.clone();
    wire["future_frame_field"] = json!(true);
    wire["queue"]["future_queue_field"] = json!(true);
    let parsed = ServerFrame::parse(&serde_json::to_vec(&wire)?)?;
    assert_eq!(serde_json::to_value(parsed)?, expected);
    Ok(())
}

#[test]
fn proto_u_003_queue_constructor_keeps_zero_and_the_exact_upper_bound() -> TestResult {
    use tqa_types::{ErrorCode, MAX_QUEUE_AHEAD, QueuePosition};
    assert_eq!(MAX_QUEUE_AHEAD, 10_000);
    for ahead in [0, 1, MAX_QUEUE_AHEAD] {
        let queue = QueuePosition::new(ahead)?;
        assert_eq!(queue.ahead(), ahead);
        assert_eq!(serde_json::to_value(queue)?, json!({"ahead":ahead}));
    }
    let error = QueuePosition::new(MAX_QUEUE_AHEAD + 1)
        .err()
        .ok_or("accepted an out-of-bounds queue position")?;
    assert_eq!(error.code, ErrorCode::InvalidFrame);
    assert_eq!(error.path, "queue.ahead");
    Ok(())
}

#[test]
fn proto_u_003_queue_reason_says_what_the_run_waits_for_and_tolerates_future_reasons() -> TestResult
{
    use tqa_types::{QueuePosition, QueueReason};
    for (reason, wire) in [
        (QueueReason::Slot, "slot"),
        (QueueReason::Workspace, "workspace"),
        (QueueReason::OwnSandbox, "own_sandbox"),
        (QueueReason::DesktopSessions, "desktop_sessions"),
    ] {
        let queue = QueuePosition::new(0)?.with_reason(reason);
        assert_eq!(
            serde_json::to_value(queue)?,
            json!({"ahead":0,"reason":wire})
        );
        let parsed: QueuePosition = serde_json::from_value(json!({"ahead":0,"reason":wire}))?;
        assert_eq!(parsed.reason(), Some(reason));
    }
    // Absent: no reason (older gateways); unknown: ignored like any future metadata.
    let plain: QueuePosition = serde_json::from_value(json!({"ahead":1}))?;
    assert_eq!(
        (plain.reason(), serde_json::to_value(plain)?),
        (None, json!({"ahead":1}))
    );
    let future: QueuePosition = serde_json::from_value(json!({"ahead":1,"reason":"maintenance"}))?;
    assert_eq!(future.reason(), None);
    Ok(())
}
