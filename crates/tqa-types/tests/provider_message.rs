//! PROTO-U-019: the optional `provider_message` on run.failed and provider.retry, the provider's own words, sanitised.
//! Absent stays absent; 1 to 300 characters (code points, as Rust and the web's validator both count);
//! no control characters; the parser and the exported schema agree.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    ErrorCode, Event, MAX_PROVIDER_MESSAGE_CHARS, ServerFrame, protocol_schema,
    valid_provider_message,
};

type TestResult = Result<(), Box<dyn Error>>;

fn failed(message: Option<Value>) -> Value {
    let mut event = json!({"type":"run.failed","run_id":"r-1","reason":"provider_error 400","code":"provider_error"});
    if let Some(message) = message {
        event["provider_message"] = message;
    }
    event
}
fn retry(message: Option<Value>) -> Value {
    let mut event =
        json!({"type":"provider.retry","attempt":1,"delay_ms":250,"reason":"provider_error 500"});
    if let Some(message) = message {
        event["provider_message"] = message;
    }
    event
}
fn frame(event: &Value) -> Value {
    json!({"type":"event","seq":1,"run_id":"r-1","step":null,"ts":"2000-01-01T00:00:00Z","event":event})
}
fn parsed(event: &Value) -> Result<ServerFrame, tqa_types::FrameError> {
    ServerFrame::parse(&serde_json::to_vec(&frame(event)).unwrap_or_default())
}
fn schema_accepts(event: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/Event");
    Ok(jsonschema::validator_for(&schema)?.is_valid(event))
}

#[test]
fn proto_u_019_absent_stays_absent_and_a_message_round_trips_exactly() -> TestResult {
    assert_eq!(MAX_PROVIDER_MESSAGE_CHARS, 300);
    for event in [
        failed(None),
        retry(None),
        failed(Some(json!("The model `gpt-x` does not exist."))),
        retry(Some(json!("upstream overloaded; try again"))),
    ] {
        let decoded: Event = serde_json::from_value(event.clone())?;
        assert_eq!(serde_json::to_value(&decoded)?, event, "exact round trip");
        parsed(&event)?;
        assert!(schema_accepts(&event)?, "{event}");
    }
    let Event::RunFailed {
        provider_message,
        code,
        reason,
        ..
    } = serde_json::from_value(failed(Some(json!("x"))))?
    else {
        return Err("not run.failed".into());
    };
    assert_eq!(
        (provider_message.as_deref(), code, reason.as_str()),
        (Some("x"), ErrorCode::ProviderError, "provider_error 400")
    );
    Ok(())
}

#[test]
fn proto_u_019_the_bound_is_300_characters_counted_as_code_points() -> TestResult {
    // Exactly 300 characters whose last is multi-byte: 299 ASCII and one 'é' (2 bytes), 301 bytes in all.
    let at_bound = format!("{}é", "a".repeat(299));
    assert_eq!(at_bound.chars().count(), 300);
    let past = format!("{}é", "a".repeat(300));
    for make in [failed as fn(Option<Value>) -> Value, retry] {
        let ok = make(Some(json!(at_bound)));
        parsed(&ok)?;
        assert!(
            schema_accepts(&ok)?,
            "300 characters are accepted by the schema"
        );
        let long = make(Some(json!(past)));
        assert!(parsed(&long).is_err(), "301 characters are refused");
        assert!(!schema_accepts(&long)?, "and by the schema");
    }
    assert!(valid_provider_message(&at_bound) && !valid_provider_message(&past));
    Ok(())
}

#[test]
fn proto_u_019_empty_null_and_control_characters_are_refused_by_both() -> TestResult {
    for bad in [
        json!(""),
        json!(null),
        json!(7),
        json!("a\u{0}b"),
        json!("line\nbreak"),
        json!("tab\there"),
        json!("del\u{7f}"),
        json!("c1\u{85}"),
        json!("c1\u{9f}"),
    ] {
        for make in [failed as fn(Option<Value>) -> Value, retry] {
            let event = make(Some(bad.clone()));
            let error = parsed(&event)
                .err()
                .ok_or_else(|| format!("accepted {bad}"))?;
            assert_eq!(error.code, ErrorCode::InvalidFrame, "{bad}");
            assert!(!schema_accepts(&event)?, "the schema accepts {bad}");
        }
    }
    // Just outside the control ranges: allowed.
    for good in [
        "\u{a0}no-break space",
        "\u{200f}right-to-left mark",
        "emoji 🚀",
    ] {
        parsed(&failed(Some(json!(good))))?;
        assert!(schema_accepts(&failed(Some(json!(good))))?, "{good}");
    }
    Ok(())
}

#[test]
fn proto_u_019_the_schema_states_the_plain_character_class() -> TestResult {
    let schema = protocol_schema();
    let variants = schema["definitions"]["Event"]["oneOf"]
        .as_array()
        .ok_or("Event variants")?;
    for kind in ["run.failed", "provider.retry"] {
        let variant = variants
            .iter()
            .find(|shape| shape["properties"]["type"]["const"] == kind)
            .ok_or("variant")?;
        let field = &variant["properties"]["provider_message"];
        assert_eq!(field["type"], "string", "{kind}");
        assert_eq!(field["minLength"], 1, "{kind}");
        assert_eq!(field["maxLength"], 300, "{kind}");
        assert_eq!(
            field["pattern"], "^[^\\u0000-\\u001f\\u007f-\\u009f]+$",
            "{kind}"
        );
        assert!(
            !variant["required"]
                .as_array()
                .ok_or("required")?
                .iter()
                .any(|name| name == "provider_message"),
            "{kind}: optional"
        );
    }
    Ok(())
}
