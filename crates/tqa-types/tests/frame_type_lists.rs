//! PROTO-U-003: a malformed frame or event of a KNOWN type is `invalid_frame`, never `unknown_frame`. The known types are
//! read from the exported schema, so a new frame or event can never be missing from the parser's type lists.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ClientFrame, ErrorCode, ServerFrame, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

/// The `type` constants of a tagged union in the exported schema.
fn kinds(schema: &Value, definition: &str) -> Vec<String> {
    let branches = schema["definitions"][definition]["oneOf"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let kinds: Vec<String> = branches
        .iter()
        .filter_map(|branch| {
            let tag = &branch["properties"]["type"];
            tag["const"].as_str().or_else(|| tag["enum"][0].as_str())
        })
        .map(str::to_owned)
        .collect();
    assert_eq!(
        kinds.len(),
        branches.len(),
        "every {definition} branch declares its type"
    );
    kinds
}

#[test]
fn proto_u_003_every_known_client_frame_type_is_classified_invalid_when_malformed() -> TestResult {
    let schema = protocol_schema();
    let kinds = kinds(&schema, "ClientFrame");
    assert!(kinds.len() >= 7, "{kinds:?}");
    for kind in kinds {
        let malformed = json!({"type": kind, "no_such_field": true});
        let error = ClientFrame::parse(&serde_json::to_vec(&malformed)?)
            .err()
            .ok_or_else(|| format!("{kind} accepted"))?;
        assert_eq!(error.code, ErrorCode::InvalidFrame, "{kind}: {error:?}");
    }
    Ok(())
}

#[test]
fn proto_u_003_every_known_server_frame_type_is_classified_invalid_when_malformed() -> TestResult {
    let schema = protocol_schema();
    let kinds = kinds(&schema, "ServerFrame");
    assert!(kinds.len() >= 6, "{kinds:?}");
    for kind in kinds {
        // A string where every frame expects numbers, objects or known strings is invalid for each of them.
        let malformed =
            json!({"type": kind, "seq": "not a sequence", "request_id": [], "event": 1, "code": 1});
        let error = ServerFrame::parse(&serde_json::to_vec(&malformed)?)
            .err()
            .ok_or_else(|| format!("{kind} accepted"))?;
        assert_eq!(error.code, ErrorCode::InvalidFrame, "{kind}: {error:?}");
    }
    Ok(())
}

#[test]
fn proto_u_003_every_known_event_type_is_classified_invalid_when_malformed() -> TestResult {
    let schema = protocol_schema();
    let kinds = kinds(&schema, "Event");
    assert!(kinds.len() >= 30, "{kinds:?}");
    // The retired markers (ADR-0035) read any old payload, so nothing about their fields is malformed.
    let retired = ["permission.resolved", "permission.expired", "run.parked"];
    for kind in &kinds {
        let malformed = json!({"type":"event","seq":1,"run_id":null,"step":null,"ts":"2026-09-26T00:00:00Z",
            "event":{"type": kind, "no_such_field": [1]}});
        let parsed = ServerFrame::parse(&serde_json::to_vec(&malformed)?);
        if retired.contains(&kind.as_str()) {
            assert!(parsed.is_ok(), "{kind} reads an old payload: {parsed:?}");
            continue;
        }
        let error = parsed.err().ok_or_else(|| format!("{kind} accepted"))?;
        assert_eq!(error.code, ErrorCode::InvalidFrame, "{kind}: {error:?}");
    }
    // The contrast: a type nobody knows is unknown, with the event's path.
    let unknown = json!({"type":"event","seq":1,"run_id":null,"step":null,"ts":"2026-09-26T00:00:00Z",
        "event":{"type":"no.such_event"}});
    let error = ServerFrame::parse(&serde_json::to_vec(&unknown)?)
        .err()
        .ok_or("an unknown event was accepted")?;
    assert_eq!(
        (error.code, error.path.as_str()),
        (ErrorCode::UnknownFrame, "$.event.type")
    );
    Ok(())
}
