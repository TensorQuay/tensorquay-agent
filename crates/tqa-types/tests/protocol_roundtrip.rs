//! PROTO-C-001: every wire variant validates against the exported schema and round-trips.

#[path = "fixtures/protocol.rs"]
mod support;

use std::error::Error;

use serde::Serialize;
use serde_json::{Value, json};
use tqa_types::{ClientFrame, Event, ServerFrame};

fn validate<T: Serialize>(name: &str, value: &T) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut schema: Value = serde_json::from_str(&std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/schema/tqa-protocol-v1.json"
    ))?)?;
    schema["$ref"] = json!(format!("#/definitions/{name}"));
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)?;
    let encoded = serde_json::to_vec(value)?;
    let instance: Value = serde_json::from_slice(&encoded)?;
    assert!(
        validator.is_valid(&instance),
        "schema rejected {name}: {instance}"
    );
    Ok(encoded)
}

#[test]
fn proto_c_001_every_client_frame_round_trips() -> Result<(), Box<dyn Error>> {
    for frame in support::clients()? {
        assert_eq!(
            ClientFrame::parse(&validate("ClientFrame", &frame)?)?,
            frame
        );
    }
    Ok(())
}

#[test]
fn proto_c_001_every_server_frame_round_trips() -> Result<(), Box<dyn Error>> {
    for frame in support::servers()? {
        assert_eq!(
            ServerFrame::parse(&validate("ServerFrame", &frame)?)?,
            frame
        );
    }
    Ok(())
}

#[test]
fn proto_c_001_every_event_round_trips() -> Result<(), Box<dyn Error>> {
    for event in support::events()? {
        let decoded: Event = serde_json::from_slice(&validate("Event", &event)?)?;
        assert_eq!(decoded, event);
    }
    Ok(())
}

#[test]
fn proto_c_001_identifier_schemas_reject_invalid_strings() -> Result<(), Box<dyn Error>> {
    let document: Value = serde_json::from_str(&std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/schema/tqa-protocol-v1.json"
    ))?)?;
    for name in ["ConversationId", "RunId", "ToolCallId", "WorkspaceId"] {
        let mut schema = document.clone();
        schema["$ref"] = json!(format!("#/definitions/{name}"));
        let validator = jsonschema::validator_for(&schema)?;
        for text in [
            String::new(),
            "x".repeat(65),
            "a b".to_owned(),
            "文".to_owned(),
            "a/b".to_owned(),
            "a\n".to_owned(),
            "a\r".to_owned(),
            "a\u{2028}".to_owned(),
        ] {
            assert!(!validator.is_valid(&json!(text)), "{name}: {text}");
        }
        for text in ["a".to_owned(), "A-z_09".to_owned(), "x".repeat(64)] {
            assert!(validator.is_valid(&json!(text)), "{name}: {text}");
        }
    }
    Ok(())
}
