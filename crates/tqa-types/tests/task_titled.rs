//! PROTO-U-028: `task.titled` is the gateway's notice to clients that the model named a task (its short name after
//! the first completed run). It is bounded as a conversation title is: 1 to 200 characters on one line, not blank, no
//! control characters.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ErrorCode, ServerFrame, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn titled(title: &Value) -> Value {
    json!({"type": "task.titled", "title": title})
}

fn schema_accepts(wire: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ServerFrame");
    Ok(jsonschema::options().build(&schema)?.is_valid(wire))
}

#[test]
fn proto_u_028_a_title_round_trips_and_matches_the_schema() -> TestResult {
    for title in [
        "Sales CSV cleanup",
        "x",
        &"é".repeat(200),
        "Fix the build — twice",
    ] {
        let wire = titled(&json!(title));
        let frame = ServerFrame::parse(&serde_json::to_vec(&wire)?)?;
        assert_eq!(
            frame,
            ServerFrame::TaskTitled {
                title: title.to_owned()
            }
        );
        assert_eq!(serde_json::to_value(&frame)?, wire);
        assert!(schema_accepts(&wire)?, "{title}");
    }
    // Later additive fields are ignored, as for every server frame.
    let wire = json!({"type": "task.titled", "title": "Sales CSV cleanup", "source": "model"});
    assert!(ServerFrame::parse(&serde_json::to_vec(&wire)?).is_ok());
    Ok(())
}

#[test]
fn proto_u_028_a_title_out_of_bounds_is_an_invalid_frame() -> TestResult {
    for title in [
        json!(""),
        json!("   "),
        json!("é".repeat(201)),
        json!("two\nlines"),
        json!("a\ttab"),
        json!("bell\u{7}"),
        json!("next line\u{85}"),
        json!(7),
        Value::Null,
    ] {
        let wire = titled(&title);
        let error = ServerFrame::parse(&serde_json::to_vec(&wire)?)
            .err()
            .ok_or_else(|| format!("{title} accepted"))?;
        assert_eq!(error.code, ErrorCode::InvalidFrame, "{title}: {error:?}");
    }
    let error = ServerFrame::parse(br#"{"type":"task.titled"}"#)
        .err()
        .ok_or("a frame without a title was accepted")?;
    assert_eq!(error.code, ErrorCode::InvalidFrame);
    // The schema states the length bounds; the parser also refuses blank and control characters.
    assert!(!schema_accepts(&titled(&json!("")))?);
    assert!(!schema_accepts(&titled(&json!("é".repeat(201))))?);
    Ok(())
}
