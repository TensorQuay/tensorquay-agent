//! Exact client input, forward-compatible server input and frame-size tests.

use std::error::Error;

use serde_json::{Value, json};
use tqa_types::{ClientFrame, ErrorCode, MAX_FRAME_BYTES, MAX_USER_MESSAGE_BYTES, ServerFrame};

fn client_examples() -> Vec<Value> {
    vec![
        json!({"type": "session.init", "conversation_id": "c", "last_seq": 0, "protocol": 1}),
        json!({"type": "user.message", "text": "hello", "attachments": []}),
        json!({"type": "run.cancel", "run_id": "r"}),
    ]
}

#[test]
fn proto_u_003_unknown_frame_is_typed() {
    for result in [
        ClientFrame::parse(br#"{"type":"nope"}"#).map(|_| ()),
        ServerFrame::parse(br#"{"type":"nope"}"#).map(|_| ()),
    ] {
        assert!(matches!(result, Err(error) if error.code == ErrorCode::UnknownFrame));
    }
}

#[test]
fn proto_u_003_unknown_nested_event_type_is_typed() -> Result<(), Box<dyn Error>> {
    let input = json!({"type": "event", "seq": 1, "run_id": null, "step": null,
        "ts": "2026-09-22T00:00:00Z", "event": {"type": "nope"}});
    let error = ServerFrame::parse(&serde_json::to_vec(&input)?)
        .err()
        .ok_or("accepted unknown event")?;
    assert_eq!(error.code, ErrorCode::UnknownFrame);
    assert_eq!(error.path, "$.event.type");
    for status in ["nope", "tool.end"] {
        let wrong_status = json!({"type": "event", "seq": 1,
            "ts": "2026-09-22T00:00:00Z", "event": {"type": "tool.end", "call_id": "a", "status": status}});
        let error = ServerFrame::parse(&serde_json::to_vec(&wrong_status)?)
            .err()
            .ok_or("accepted unknown status")?;
        assert_eq!(error.code, ErrorCode::InvalidFrame);
    }
    Ok(())
}

#[test]
fn proto_u_007_every_missing_client_field_is_invalid() -> Result<(), Box<dyn Error>> {
    for value in client_examples() {
        let object = value.as_object().ok_or("fixture must be an object")?;
        for field in object.keys() {
            let mut bad = object.clone();
            bad.remove(field);
            let error = ClientFrame::parse(&serde_json::to_vec(&bad)?)
                .err()
                .ok_or("accepted missing field")?;
            assert_eq!(error.code, ErrorCode::InvalidFrame, "{field}");
            assert!(!error.path.is_empty(), "{field}");
        }
    }
    Ok(())
}

#[test]
fn proto_u_007_wrong_client_field_types_are_invalid() -> Result<(), Box<dyn Error>> {
    for value in client_examples() {
        let object = value.as_object().ok_or("fixture must be an object")?;
        for field in object.keys() {
            let mut bad = object.clone();
            bad.insert(field.clone(), json!({"wrong": "type"}));
            let error = ClientFrame::parse(&serde_json::to_vec(&bad)?)
                .err()
                .ok_or("accepted wrong type")?;
            assert_eq!(error.code, ErrorCode::InvalidFrame, "{field}");
            assert!(!error.path.is_empty(), "{field}");
        }
    }
    Ok(())
}

#[test]
fn proto_u_007_client_extra_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    for mut value in client_examples() {
        value["stray"] = json!(true);
        let result = ClientFrame::parse(&serde_json::to_vec(&value)?);
        assert!(matches!(result, Err(error) if error.code == ErrorCode::InvalidFrame));
    }
    Ok(())
}

#[test]
fn proto_u_007_server_extra_fields_are_ignored() -> Result<(), Box<dyn Error>> {
    let original = json!({"type": "session.ready", "conversation_id": "c", "resumed": false,
        "sandbox": {"backend": "docker", "state": "ready"}});
    let mut extended = original.clone();
    extended["future"] = json!("field");
    extended["sandbox"]["future"] = json!(true);
    assert_eq!(
        ServerFrame::parse(&serde_json::to_vec(&original)?)?,
        ServerFrame::parse(&serde_json::to_vec(&extended)?)?
    );
    Ok(())
}

#[test]
fn proto_u_007_user_message_limits_are_measured_in_bytes() -> Result<(), Box<dyn Error>> {
    for text in [
        String::new(),
        "x".repeat(MAX_USER_MESSAGE_BYTES + 1),
        "界".repeat(MAX_USER_MESSAGE_BYTES / 3 + 1),
    ] {
        let value = json!({"type": "user.message", "text": text, "attachments": []});
        assert!(ClientFrame::parse(&serde_json::to_vec(&value)?).is_err());
    }
    let boundary = json!({"type": "user.message", "text": "x".repeat(MAX_USER_MESSAGE_BYTES),
        "attachments": []});
    assert!(ClientFrame::parse(&serde_json::to_vec(&boundary)?).is_ok());
    Ok(())
}

#[test]
fn proto_u_005_frame_cap_precedes_json_parsing() {
    let oversized = vec![b'!'; MAX_FRAME_BYTES + 1];
    for result in [
        ClientFrame::parse(&oversized).map(|_| ()),
        ServerFrame::parse(&oversized).map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(error) if error.code == ErrorCode::InvalidFrame
            && error.detail.contains("256") && !error.path.is_empty())
        );
    }
}

#[test]
fn proto_u_007_malformed_json_and_duplicate_fields_are_invalid() {
    for input in [
        b"not JSON".as_slice(),
        b"null",
        br#"{"type":"run.cancel","run_id":"r","run_id":"s"}"#,
        br#"{"type":"run.cancel","run_id":"r"} trailing"#,
    ] {
        assert!(
            matches!(ClientFrame::parse(input), Err(error) if error.code == ErrorCode::InvalidFrame)
        );
    }
}

#[test]
fn proto_u_007_attachment_paths_are_bounded() -> Result<(), Box<dyn Error>> {
    for attachment in ["", "a/b", "a b", "文"] {
        let input = json!({"type": "user.message", "text": "hello", "attachments": [attachment]});
        assert!(ClientFrame::parse(&serde_json::to_vec(&input)?).is_err());
    }
    Ok(())
}

#[test]
fn proto_u_007_event_timestamp_is_rfc3339() -> Result<(), Box<dyn Error>> {
    let valid = [
        "2026-09-22T12:34:56Z",
        "2024-02-29T00:00:00.123+08:00",
        "2026-09-22t12:34:56z",
        "2026-09-22T12:34:56-04:00",
        "2026-06-30T23:59:60Z",
    ];
    let invalid = [
        "",
        "today",
        "2026-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-01-00T00:00:00Z",
        "2026-01-32T00:00:00Z",
        "2026-09-22T24:00:00Z",
        "2026-09-22T00:60:00Z",
        "2026-09-22T00:00:61Z",
        "2026-09-22T00:00:00",
        "2026-09-22T00:00:00.Z",
        "2026-09-22T00:00:00+24:00",
        "2026-09-22T00:00:00+00:60",
        "2026-09-22T00:00:00+0000",
        "2026-09-22X00:00:00Z",
        "2026-09-22T00:00:00.文Z",
    ];
    for (timestamps, expected) in [(valid.as_slice(), true), (invalid.as_slice(), false)] {
        for timestamp in timestamps {
            let input = json!({"type": "event", "seq": 1, "run_id": null, "step": null,
                "ts": timestamp, "event": {"type": "text.delta", "text": "hello"}});
            assert_eq!(
                ServerFrame::parse(&serde_json::to_vec(&input)?).is_ok(),
                expected,
                "{timestamp}"
            );
        }
    }
    Ok(())
}

#[test]
fn proto_u_015_workspace_full_names_the_call_and_a_malformed_one_is_invalid_not_unknown()
-> Result<(), Box<dyn Error>> {
    let event = tqa_types::Event::WorkspaceFull {
        call_id: "call-7".parse()?,
    };
    assert_eq!(
        serde_json::to_value(&event)?,
        json!({"type": "workspace.full", "call_id": "call-7"})
    );
    let malformed = json!({"type": "event", "seq": 1, "run_id": null, "step": null,
        "ts": "2026-09-22T00:00:00Z", "event": {"type": "workspace.full"}});
    let error = ServerFrame::parse(&serde_json::to_vec(&malformed)?)
        .err()
        .ok_or("accepted a workspace.full without its call")?;
    assert_eq!(error.code, ErrorCode::InvalidFrame);
    Ok(())
}
