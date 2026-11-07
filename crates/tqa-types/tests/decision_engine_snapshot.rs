//! PROTO-U-024: the revision-9 decision engine in `session.config` (ADR-0013 point 7). It carries the engine's name,
//! relay path, deadline and exposure and nothing else; absent means the model decides. Every bound is refused on parse
//! and, where JSON Schema can say it, by the exported schema alike, and each refusal names `$.decision_engine`.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    ClientFrame, DecisionEngineSnapshot, DecisionExposure, ErrorCode, protocol_schema,
    valid_decision_engine_name,
};

type TestResult = Result<(), Box<dyn Error>>;

fn config(engine: Option<Value>) -> Value {
    let mut frame = json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":0,
        "route":{"name":"local","protocol":"openai_chat","model":"m","relay_path":"/relay/r/local",
            "context_window":8192,"max_output":1024,"native_tools":true,"parallel_tools":true,"vision":false,
            "reasoning":null},
        "network":{"allowed":[],"more":0},
        "limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":65_536,"max_output_tokens":131_072},
        "workspace":{"display":"/workspace"},"protocol":1
    });
    if let Some(value) = engine {
        frame["decision_engine"] = value;
    }
    frame
}
fn engine(edit: impl FnOnce(&mut Value)) -> Value {
    let mut value = json!({"name":"router","relay_path":"/relay/d/router","timeout_ms":2000,"exposure":"calls"});
    edit(&mut value);
    config(Some(value))
}
fn schema_accepts(value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}
fn parse(value: &Value) -> Result<ClientFrame, tqa_types::FrameError> {
    ClientFrame::parse(&serde_json::to_vec(value).unwrap_or_default())
}

// TEST: PROTO-U-024
#[test]
fn proto_u_024_the_engine_round_trips_and_absent_means_none() -> TestResult {
    let named = engine(|_| {});
    let ClientFrame::SessionConfig {
        decision_engine, ..
    } = parse(&named)?
    else {
        return Err("not a session.config".into());
    };
    assert_eq!(
        decision_engine,
        Some(DecisionEngineSnapshot {
            name: "router".into(),
            relay_path: "/relay/d/router".into(),
            timeout_ms: 2000,
            exposure: DecisionExposure::Calls,
            points: None,
        })
    );
    assert_eq!(serde_json::to_value(parse(&named)?)?, named);
    assert!(schema_accepts(&named)?);
    let plain = config(None);
    let parsed = parse(&plain)?;
    assert!(matches!(
        &parsed,
        ClientFrame::SessionConfig {
            decision_engine: None,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(parsed)?,
        plain,
        "no field is written without an engine"
    );
    Ok(())
}

// TEST: PROTO-U-024
#[test]
fn proto_u_024_every_bound_is_refused_on_parse_and_by_the_schema() -> TestResult {
    let long = "r".repeat(65);
    let cases: Vec<(Value, bool)> = vec![
        (engine(|value| value["name"] = json!("Router")), true),
        (engine(|value| value["name"] = json!("-router")), true),
        (
            engine(|value| {
                value["name"] = json!(long);
                value["relay_path"] = json!(format!("/relay/d/{long}"));
            }),
            true,
        ),
        // A relay path that isn't the engine's own; the schema can't tie it to the name.
        (
            engine(|value| value["relay_path"] = json!("/relay/d/other")),
            false,
        ),
        (
            engine(|value| value["relay_path"] = json!("/relay/r/router")),
            true,
        ),
        (engine(|value| value["points"] = json!([])), true),
        (
            engine(|value| value["points"] = json!(["before_step", "before_step"])),
            true,
        ),
        (engine(|value| value["points"] = json!(["during"])), true),
        (engine(|value| value["timeout_ms"] = json!(0)), true),
        (engine(|value| value["timeout_ms"] = json!(10_001)), true),
        (
            engine(|value| value["exposure"] = json!("everything")),
            true,
        ),
        (
            engine(|value| value["url"] = json!("https://engine.example")),
            true,
        ),
        (
            engine(|value| value["key_ref"] = json!("secrets/engine")),
            true,
        ),
        (config(Some(Value::Null)), true),
    ];
    for (frame, schema_says) in cases {
        let error = parse(&frame).err().ok_or(format!("accepted: {frame}"))?;
        assert_eq!(error.code, ErrorCode::InvalidFrame, "{frame}");
        assert_eq!(error.path, "$.decision_engine", "{frame}");
        if schema_says {
            assert!(!schema_accepts(&frame)?, "the schema accepts {frame}");
        }
    }
    Ok(())
}

// TEST: PROTO-U-024
#[test]
fn proto_u_024_the_name_rule() {
    for name in ["router", "r", "a-b_c9", &"r".repeat(64)] {
        assert!(valid_decision_engine_name(name), "{name}");
    }
    for name in ["", "Router", "9router", "-r", "r.s", "r s", &"r".repeat(65)] {
        assert!(!valid_decision_engine_name(name), "{name}");
    }
}

// TEST: PROTO-U-024
#[test]
fn proto_u_024_points_name_what_the_engine_answers_and_absent_means_both() -> TestResult {
    use tqa_types::DecisionPoint;
    let one = engine(|value| value["points"] = json!(["before_step"]));
    let ClientFrame::SessionConfig {
        decision_engine: Some(snapshot),
        ..
    } = parse(&one)?
    else {
        return Err("no engine".into());
    };
    assert!(snapshot.answers(DecisionPoint::BeforeStep));
    assert!(!snapshot.answers(DecisionPoint::AfterResponse));
    assert_eq!(serde_json::to_value(parse(&one)?)?, one);
    assert!(schema_accepts(&one)?);
    let both = engine(|value| value["points"] = json!(["after_response", "before_step"]));
    assert!(parse(&both).is_ok() && schema_accepts(&both)?);
    let ClientFrame::SessionConfig {
        decision_engine: Some(absent),
        ..
    } = parse(&engine(|_| {}))?
    else {
        return Err("no engine".into());
    };
    assert!(
        absent.answers(DecisionPoint::BeforeStep) && absent.answers(DecisionPoint::AfterResponse)
    );
    assert!(
        !serde_json::to_string(&absent)?.contains("points"),
        "an engine answering both writes no points, so decision_v1 engines' snapshots are unchanged"
    );
    Ok(())
}
