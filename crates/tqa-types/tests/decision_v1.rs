//! PROTO-U-022: the `decision/v1` request and response (ADR-0013 point 5). A response is exactly one decision, parsed
//! strictly: an unknown field or value is refused, and so is an answer over 64 KiB. The request carries the exposure's
//! optional parts only when present. The exported schema matches the types and the committed file.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::decision_v1::{self, Request, Response};
use tqa_types::{DecisionPoint, decision_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn schema_accepts(definition: &str, value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = decision_schema();
    schema["$ref"] = json!(format!("#/definitions/{definition}"));
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}

#[test]
fn proto_u_022_every_decision_kind_parses_strictly_and_round_trips() -> TestResult {
    for answer in [
        json!({"decision":"call_model","route":"local","tools":["read_file"]}),
        json!({"decision":"act","tool":"read_file","arguments":{"path":"a"}}),
        json!({"decision":"compact"}),
        json!({"decision":"stop","reason":"out of scope"}),
        json!({"decision":"select","call_ids":["a","b"]}),
    ] {
        let parsed = decision_v1::parse_response(&serde_json::to_vec(&answer)?)?;
        assert_eq!(serde_json::to_value(&parsed)?, answer);
        assert!(schema_accepts("Response", &answer)?, "{answer}");
    }
    assert!(matches!(
        decision_v1::parse_response(br#"{"decision":"compact"}"#)?,
        Response::Compact {}
    ));
    Ok(())
}

#[test]
fn proto_u_022_unknown_fields_values_and_bounds_are_refused() -> TestResult {
    for invalid in [
        json!({"decision":"compact","why":"because"}),
        json!({"decision":"wander"}),
        json!({"route":"local","tools":[]}),
        json!({"decision":"stop","reason":"r".repeat(201)}),
        json!({"decision":"stop","reason":""}),
        json!({"decision":"call_model","route":"local","tools":["Read File"]}),
        json!({"decision":"act","tool":"read_file","arguments":"a"}),
        json!({"decision":"select","call_ids":(0..65).map(|index| format!("c{index}")).collect::<Vec<_>>()}),
    ] {
        assert!(
            decision_v1::parse_response(&serde_json::to_vec(&invalid)?).is_err(),
            "{invalid}"
        );
        assert!(!schema_accepts("Response", &invalid)?, "{invalid}");
    }
    // Over the answer bound: refused before parsing.
    let big = json!({"decision":"act","tool":"read_file","arguments":{"x":"y".repeat(64 * 1024)}});
    let error = decision_v1::parse_response(&serde_json::to_vec(&big)?)
        .err()
        .ok_or("an oversized answer was accepted")?;
    assert!(error.to_string().contains("64 KiB"), "{error}");
    assert!(decision_v1::parse_response(b"not json").is_err());
    Ok(())
}

#[test]
fn proto_u_022_a_request_carries_the_exposures_parts_only_when_present() -> TestResult {
    let minimal = json!({
        "point": "before_step",
        "run_id": "r-1",
        "step": 2,
        "attempt": 1,
        "routes": [{"name":"local","context_window":32768,"max_output":8192,"native_tools":true}],
        "tools": [{"name":"read_file","description":"Read a file","parameters":{"type":"object"}}],
        "budget": {"steps_used":1,"max_steps":30,"output_tokens_used":120,"max_output_tokens":200_000},
        "previous_calls": [{"name":"read_file","status":"ok"}]
    });
    let parsed: Request = serde_json::from_value(minimal.clone())?;
    assert_eq!(parsed.point, DecisionPoint::BeforeStep);
    assert!(parsed.response_calls.is_empty() && parsed.transcript.is_none());
    assert_eq!(
        serde_json::to_value(&parsed)?,
        minimal,
        "no optional part appears when absent"
    );
    assert!(schema_accepts("Request", &minimal)?);
    let mut full = minimal.clone();
    full["point"] = json!("after_response");
    full["previous_calls"][0]["arguments"] = json!({"path":"a"});
    full["response_calls"] = json!([{"id":"a","name":"read_file","arguments":{"path":"b"}}]);
    full["response_text"] = json!("reading b");
    full["transcript"] = json!([{"role":"user","text":"task"}]);
    let parsed: Request = serde_json::from_value(full.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, full);
    assert!(schema_accepts("Request", &full)?);
    // A call's name is a tool name, in the types and the schema, so it can't carry text under exposure `none`.
    for (field, call) in [
        (
            "previous_calls",
            json!({"name":"the secret is in notes.txt"}),
        ),
        (
            "response_calls",
            json!({"id":"a","name":"task: rotate the keys"}),
        ),
    ] {
        let mut named = full.clone();
        named[field] = json!([call]);
        assert!(
            serde_json::from_value::<Request>(named.clone()).is_err(),
            "{field}"
        );
        assert!(!schema_accepts("Request", &named)?, "{field}");
    }
    let mut unknown = minimal;
    unknown["extra"] = json!(1);
    assert!(serde_json::from_value::<Request>(unknown.clone()).is_err());
    assert!(!schema_accepts("Request", &unknown)?);
    Ok(())
}

#[test]
fn proto_u_022_the_committed_schema_matches_the_types() -> TestResult {
    let committed: Value = serde_json::from_str(include_str!("../schema/decision-v1.json"))?;
    assert_eq!(committed, decision_schema());
    Ok(())
}
