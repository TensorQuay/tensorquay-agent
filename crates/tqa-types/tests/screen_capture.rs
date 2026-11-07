//! PROTO-U-036 (ADR-0039, revision 14): `screen.capture`, one stored screenshot of a `computer` call; the route
//! snapshot's `pixel_grounding`, which offers that tool's coordinate actions; and `session.config`'s
//! `computer_limits`, the tool's per-run caps.
use serde_json::{Value, json};
use tqa_types::{CapturePhase, ComputerLimits, Event, RouteSnapshot, protocol_schema};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn capture(phase: &str) -> Value {
    json!({"type": "screen.capture", "call_id": "c-1", "phase": phase, "id": "shot_01-a", "width": 1280,
           "height": 800, "bytes": 183_420, "sha256": "a".repeat(64), "sent_to_model": false, "marked": false})
}

fn schema_accepts(event: &Value) -> Result<bool, Box<dyn std::error::Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/Event");
    Ok(jsonschema::validator_for(&schema)?.is_valid(event))
}

// TEST: PROTO-U-036
#[test]
fn proto_u_036_a_capture_round_trips_in_each_phase() -> TestResult {
    for (word, phase) in [
        ("before", CapturePhase::Before),
        ("after", CapturePhase::After),
        ("screenshot", CapturePhase::Screenshot),
    ] {
        let wire = capture(word);
        let event: Event = serde_json::from_value(wire.clone())?;
        assert!(
            matches!(&event, Event::ScreenCapture { phase: read, .. } if *read == phase),
            "{word}"
        );
        assert_eq!(serde_json::to_value(&event)?, wire, "round-trips exactly");
        assert!(schema_accepts(&wire)?, "the exported schema accepts {word}");
    }
    Ok(())
}

// TEST: PROTO-U-036
#[test]
fn proto_u_036_an_odd_capture_is_refused() -> TestResult {
    let cases: Vec<(&str, Value)> = vec![
        ("phase", json!("during")),
        ("id", json!("")),
        ("id", json!("../shot")),
        ("id", json!("x".repeat(129))),
        ("width", json!(0)),
        ("height", json!(8193)),
        ("bytes", json!(-1)),
        ("bytes", json!(524_289)),
        ("sha256", json!("A".repeat(64))),
        ("sha256", json!("a".repeat(63))),
    ];
    for (field, value) in cases {
        let mut wire = capture("before");
        wire[field] = value.clone();
        assert!(
            serde_json::from_value::<Event>(wire.clone()).is_err(),
            "{field}: {value}"
        );
        assert!(
            !schema_accepts(&wire)?,
            "the schema refuses {field}: {value}"
        );
    }
    let mut later = capture("before");
    later["blur"] = json!(true);
    assert!(
        serde_json::from_value::<Event>(later).is_ok(),
        "a later field is ignored, as on every event"
    );
    for field in [
        "call_id",
        "phase",
        "id",
        "width",
        "height",
        "bytes",
        "sha256",
        "sent_to_model",
        "marked",
    ] {
        let mut wire = capture("after");
        if let Some(object) = wire.as_object_mut() {
            object.remove(field);
        }
        assert!(
            serde_json::from_value::<Event>(wire).is_err(),
            "without {field}"
        );
    }
    Ok(())
}

// TEST: PROTO-U-036
#[test]
fn proto_u_036_pixel_grounding_is_off_unless_the_route_says_so() -> TestResult {
    let mut route = json!({
        "name":"local","protocol":"openai_chat","model":"qwen3:4b",
        "relay_path":"/relay/r/local","context_window":4096,"max_output":null,
        "native_tools":true,"parallel_tools":true,"vision":true,"reasoning":null
    });
    let off: RouteSnapshot = serde_json::from_value(route.clone())?;
    assert!(!off.pixel_grounding, "absent means off");
    assert_eq!(
        serde_json::to_value(&off)?.get("pixel_grounding"),
        None,
        "off is not written"
    );
    route["pixel_grounding"] = json!(true);
    let on: RouteSnapshot = serde_json::from_value(route.clone())?;
    assert!(on.pixel_grounding);
    assert_eq!(serde_json::to_value(&on)?["pixel_grounding"], json!(true));
    route["pixel_grounding"] = json!("yes");
    assert!(serde_json::from_value::<RouteSnapshot>(route).is_err());
    Ok(())
}

// TEST: PROTO-U-036
#[test]
fn proto_u_036_the_computer_caps_are_bounded_and_default() -> TestResult {
    let caps: ComputerLimits =
        serde_json::from_value(json!({"max_screenshots_per_run": 1000, "max_actions_per_run": 1}))?;
    assert_eq!(
        (caps.max_screenshots_per_run, caps.max_actions_per_run),
        (1000, 1)
    );
    assert_eq!(
        serde_json::to_value(caps)?,
        json!({"max_screenshots_per_run": 1000, "max_actions_per_run": 1})
    );
    for odd in [
        json!({"max_screenshots_per_run": 0, "max_actions_per_run": 10}),
        json!({"max_screenshots_per_run": 1001, "max_actions_per_run": 10}),
        json!({"max_screenshots_per_run": 10, "max_actions_per_run": 5001}),
        json!({"max_screenshots_per_run": 10}),
        json!({"max_screenshots_per_run": 10, "max_actions_per_run": 10, "max_drags": 1}),
    ] {
        assert!(
            serde_json::from_value::<ComputerLimits>(odd.clone()).is_err(),
            "{odd}"
        );
    }
    let defaults = ComputerLimits::default();
    assert_eq!(
        (
            defaults.max_screenshots_per_run,
            defaults.max_actions_per_run
        ),
        (200, 1000)
    );
    Ok(())
}
