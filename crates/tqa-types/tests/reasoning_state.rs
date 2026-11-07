//! PROTO-U-009 / SEC-021: the opaque reasoning-state event, its bounds, and the client form that
//! never carries the item.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    Event, MAX_REASONING_BYTES_PER_STEP, MAX_REASONING_ITEM_BYTES, MAX_REASONING_ITEMS_PER_STEP,
    ServerFrame, protocol_schema,
};

type TestResult = Result<(), Box<dyn Error>>;

const SECRET: &str = "opaque-encrypted-marker";

fn state(item: Option<Value>) -> Value {
    let mut value = json!({"type":"reasoning.state","protocol":"openai_responses","route":"local",
        "model":"fake-model","bytes":0});
    if let Some(item) = item {
        let text = item.to_string();
        value["bytes"] = json!(text.len());
        value["item"] = json!(text);
    }
    value
}
fn item() -> Value {
    json!({"type":"reasoning","id":"rs_1","encrypted_content":SECRET,"summary":[]})
}
fn frame(event: &Value) -> Value {
    json!({"type":"event","seq":4,"run_id":"r-1","step":1,"ts":"2026-09-25T00:00:00Z","event":event})
}
fn schema_accepts(definition: &str, value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!(format!("#/definitions/{definition}"));
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}

#[test]
fn proto_u_009_the_event_round_trips_with_and_without_its_item() -> TestResult {
    assert_eq!(MAX_REASONING_ITEM_BYTES, 128 * 1024);
    assert_eq!(MAX_REASONING_ITEMS_PER_STEP, 16);
    assert_eq!(MAX_REASONING_BYTES_PER_STEP, 512 * 1024);
    for wire in [state(Some(item())), state(None)] {
        let event: Event = serde_json::from_value(wire.clone())?;
        assert_eq!(serde_json::to_value(&event)?, wire);
        assert!(schema_accepts("Event", &wire)?, "{wire}");
        let parsed = ServerFrame::parse(&serde_json::to_vec(&frame(&wire))?)?;
        assert_eq!(serde_json::to_value(&parsed)?, frame(&wire));
    }
    Ok(())
}

/// An item of exactly `bytes` bytes of JSON text.
fn sized_item(bytes: usize) -> String {
    let (prefix, suffix) = (r#"{"type":"reasoning","encrypted_content":""#, r#""}"#);
    format!(
        "{prefix}{}{suffix}",
        "x".repeat(bytes - prefix.len() - suffix.len())
    )
}

#[test]
fn proto_u_009_items_and_names_are_bounded() {
    let with_text = |text: String| {
        let mut value = state(None);
        value["bytes"] = json!(text.len());
        value["item"] = json!(text);
        value
    };
    assert!(
        serde_json::from_value::<Event>(with_text(sized_item(MAX_REASONING_ITEM_BYTES))).is_ok()
    );
    let mut long_route = state(Some(item()));
    long_route["route"] = json!("r".repeat(257));
    let mut empty_model = state(Some(item()));
    empty_model["model"] = json!("");
    let mut unknown_protocol = state(None);
    unknown_protocol["protocol"] = json!("smoke_signals");
    for invalid in [
        with_text(sized_item(MAX_REASONING_ITEM_BYTES + 1)),
        with_text("[1,2]".into()),
        with_text("not json".into()),
        long_route,
        empty_model,
        unknown_protocol,
    ] {
        assert!(
            serde_json::from_value::<Event>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn sec_021_the_client_form_and_debug_output_never_carry_the_item() -> TestResult {
    let parsed = ServerFrame::parse(&serde_json::to_vec(&frame(&state(Some(item()))))?)?;
    let bytes = item().to_string().len();
    let client = serde_json::to_value(parsed.clone().for_clients())?;
    assert_eq!(
        client,
        frame(
            &json!({"type":"reasoning.state","protocol":"openai_responses",
        "route":"local","model":"fake-model","bytes":bytes})
        )
    );
    assert!(!client.to_string().contains(SECRET));
    assert!(
        !format!("{parsed:?}").contains(SECRET),
        "Debug must redact the item"
    );
    assert!(format!("{parsed:?}").contains(&format!("{bytes} bytes")));
    // Other frames pass through the client form unchanged.
    let text = ServerFrame::parse(&serde_json::to_vec(&frame(
        &json!({"type":"text.delta","text":"hi"}),
    ))?)?;
    assert_eq!(
        serde_json::to_value(text.clone().for_clients())?,
        serde_json::to_value(text)?
    );
    Ok(())
}
