//! TK-U-021: the fake `decision/v1` engine (ADR-0013 point 6). It answers each consultation from its script in order:
//! a decision, a malformed body, an error status, a delayed answer, or none at all; it records each request as parsed,
//! with its authorization header, and refuses another path or a request that isn't `decision/v1`.
use serde_json::{Value, json};
use std::{error::Error, time::Duration};
use tqa_testkit::{EngineAnswer, FakeEngineServer};
use tqa_types::decision_v1::Response;

type TestResult = Result<(), Box<dyn Error>>;

fn request(point: &str) -> Value {
    json!({
        "point": point,
        "run_id": "r-1",
        "step": 1,
        "attempt": 1,
        "routes": [{"name":"local","context_window":null,"max_output":null,"native_tools":true}],
        "tools": [{"name":"read_file","description":"Read","parameters":{"type":"object"}}],
        "budget": {"steps_used":0,"max_steps":30,"output_tokens_used":0,"max_output_tokens":1000},
        "previous_calls": []
    })
}
async fn post(
    server: &FakeEngineServer,
    path: &str,
    body: &Value,
) -> Result<(u16, String), Box<dyn Error>> {
    let response = reqwest::Client::new()
        .post(format!("{}{path}", server.base_url()))
        .bearer_auth("engine-key")
        .json(body)
        .send()
        .await?;
    Ok((response.status().as_u16(), response.text().await?))
}

#[tokio::test]
async fn tk_u_021_the_script_answers_in_order() -> TestResult {
    let server = FakeEngineServer::start(vec![
        EngineAnswer::Decide(Response::CallModel {
            route: "local".into(),
            tools: vec!["read_file".into()],
        }),
        EngineAnswer::Raw {
            status: 200,
            body: b"not json".to_vec(),
        },
        EngineAnswer::Raw {
            status: 503,
            body: Vec::new(),
        },
        EngineAnswer::Delay(
            Duration::from_millis(50),
            Box::new(EngineAnswer::Decide(Response::Compact {})),
        ),
    ])
    .await?;
    let (status, body) = post(&server, "/v1/before_step", &request("before_step")).await?;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&body)?,
        json!({"decision":"call_model","route":"local","tools":["read_file"]})
    );
    assert_eq!(
        post(&server, "/v1/after_response", &request("after_response")).await?,
        (200, "not json".into())
    );
    assert_eq!(
        post(&server, "/v1/before_step", &request("before_step"))
            .await?
            .0,
        503
    );
    let started = std::time::Instant::now();
    let (status, body) = post(&server, "/v1/before_step", &request("before_step")).await?;
    assert!(started.elapsed() >= Duration::from_millis(50));
    assert_eq!((status, body.as_str()), (200, r#"{"decision":"compact"}"#));
    // Past the script: the engine is unavailable.
    assert_eq!(
        post(&server, "/v1/before_step", &request("before_step"))
            .await?
            .0,
        503
    );
    let seen = server.requests();
    assert_eq!(seen.len(), 5);
    assert_eq!(seen[0].path, "/v1/before_step");
    assert_eq!(seen[1].path, "/v1/after_response");
    assert_eq!(seen[0].authorization.as_deref(), Some("Bearer engine-key"));
    assert_eq!(seen[0].request.step, 1);
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn tk_u_021_a_hang_never_answers() -> TestResult {
    let server = FakeEngineServer::start(vec![EngineAnswer::Hang]).await?;
    let outcome = reqwest::Client::new()
        .post(format!("{}/v1/before_step", server.base_url()))
        .timeout(Duration::from_millis(200))
        .json(&request("before_step"))
        .send()
        .await;
    assert!(outcome.is_err_and(|error| error.is_timeout()));
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn tk_u_021_another_path_or_a_malformed_request_is_refused() -> TestResult {
    let server = FakeEngineServer::start(vec![EngineAnswer::Decide(Response::Compact {})]).await?;
    assert_eq!(
        post(&server, "/v1/elsewhere", &request("before_step"))
            .await?
            .0,
        404
    );
    let mut unknown = request("before_step");
    unknown["extra"] = json!(1);
    assert_eq!(post(&server, "/v1/before_step", &unknown).await?.0, 400);
    assert!(
        server.requests().is_empty(),
        "nothing recorded, and the script isn't spent"
    );
    assert_eq!(
        post(&server, "/v1/before_step", &request("before_step"))
            .await?
            .0,
        200
    );
    server.stop().await;
    Ok(())
}
