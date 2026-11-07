//! TK-U-048: `"decision_engines"` makes the standalone server answer as `decision/v1` engines (ADR-0013 point 6), so a
//! stack can test a task under an engine that answers (D11's positive path). Each named engine is served at
//! `/engines/<name>` (its base URL; the runtime posts to `<base>/v1/before_step` and `<base>/v1/after_response`), with
//! no key needed:
//! - `{"stop": "<reason>"}` stops the run at its first step;
//! - `{"pass": true}` lets the run go on as without an engine: the request's first route with all its tools before a
//!   step, and every call of the response kept after it.
//!
//! An engine the scenario doesn't name, or a scenario without the key, is not found; unusable entries are refused at
//! start.
use super::support::*;
use serde_json::{Value, json};

fn scenario(engines: &Value) -> String {
    json!({"model": "fixture", "responses": [
        {"events": [{"type": "text", "text": "Scripted."}], "usage": {"prompt_tokens": 10, "completion_tokens": 2}}
    ], "decision_engines": engines})
    .to_string()
}

fn request(point: &str) -> Value {
    json!({
        "point": point,
        "run_id": "r-1",
        "step": 1,
        "attempt": 1,
        "routes": [{"name": "local", "context_window": null, "max_output": null, "native_tools": true},
                   {"name": "spare", "context_window": null, "max_output": null, "native_tools": true}],
        "tools": [{"name": "read_file", "description": "Read", "parameters": {"type": "object"}},
                  {"name": "write_file", "description": "Write", "parameters": {"type": "object"}}],
        "budget": {"steps_used": 0, "max_steps": 30, "output_tokens_used": 0, "max_output_tokens": 1000},
        "previous_calls": [],
        "response_calls": [{"id": "c-1", "name": "read_file"}, {"id": "c-2", "name": "write_file"}]
    })
}

/// A consultation as the runtime's relay sends it: no key needed here.
async fn consult(
    server: &Binary,
    engine: &str,
    point: &str,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let response = server
        .client
        .post(format!("{}/engines/{engine}/v1/{point}", server.base))
        .json(&request(point))
        .send()
        .await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    Ok((
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    ))
}

#[tokio::test]
async fn tk_u_048_a_stopping_and_a_passing_engine_answer_as_engines_do() -> TestResult {
    let server = Binary::start(&scenario(&json!({
        "stopper": {"stop": "The test engine stops here."},
        "passer": {"pass": true}
    })))
    .await?;
    assert_eq!(
        consult(&server, "stopper", "before_step").await?,
        (
            200,
            json!({"decision": "stop", "reason": "The test engine stops here."})
        )
    );
    assert_eq!(
        consult(&server, "passer", "before_step").await?,
        (
            200,
            json!({"decision": "call_model", "route": "local", "tools": ["read_file", "write_file"]})
        )
    );
    assert_eq!(
        consult(&server, "passer", "after_response").await?,
        (
            200,
            json!({"decision": "select", "call_ids": ["c-1", "c-2"]})
        )
    );
    let (status, _) = consult(&server, "nobody", "before_step").await?;
    assert_eq!(status, 404, "an engine the scenario doesn't name");
    Ok(())
}

#[tokio::test]
async fn tk_u_048_without_engines_none_is_served() -> TestResult {
    let server = Binary::start(
        &json!({"model": "fixture", "responses": [
            {"events": [{"type": "text", "text": "Scripted."}], "usage": {"prompt_tokens": 10, "completion_tokens": 2}}
        ]})
        .to_string(),
    )
    .await?;
    let (status, _) = consult(&server, "stopper", "before_step").await?;
    assert!(matches!(status, 401 | 404), "{status}");
    Ok(())
}

#[tokio::test]
async fn tk_u_048_unusable_engines_are_refused() -> TestResult {
    for engines in [
        json!({"stopper": {"stop": ""}}),
        json!({"stopper": {"stop": "x".repeat(201)}}),
        json!({"Bad Name": {"pass": true}}),
        json!({"both": {"pass": true, "stop": "no"}}),
        json!({"neither": {}}),
        json!({"passer": {"pass": false}}),
        json!([]),
    ] {
        assert!(
            Binary::start(&scenario(&engines)).await.is_err(),
            "{engines}"
        );
    }
    Ok(())
}
