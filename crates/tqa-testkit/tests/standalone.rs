//! WEB-E2E-004, LLM-C-002/019 and TK-U-035/036/037/038/039/040/042/046/047/048: the demo's real standalone endpoint
//! contract.
#[path = "standalone/cut_and_fail.rs"]
mod cut_and_fail;
#[path = "standalone/engines.rs"]
mod engines;
#[path = "standalone/hold.rs"]
mod hold;
#[path = "standalone/long_scenario.rs"]
mod long_scenario;
#[path = "standalone/profiles.rs"]
mod profiles;
#[path = "standalone/prompt_select.rs"]
mod prompt_select;
#[path = "standalone/provider.rs"]
mod provider;
#[path = "standalone/route_check.rs"]
mod route_check;
#[path = "standalone/selection.rs"]
mod selection;
#[path = "standalone/seq_of.rs"]
mod seq_of;
#[path = "standalone/server_traits.rs"]
mod server_traits;
#[path = "standalone/support.rs"]
mod support;
use serde_json::{Value, json};
use std::{io::Write, sync::Arc, time::Duration};
use support::*;
use tokio::process::Command;
use tqa_tools::{
    ToolRegistry,
    builtin::{EditFile, Glob, Grep, ReadFile, Shell, WriteFile},
};

#[tokio::test]
async fn llm_c_019_binary_requires_a_nonempty_bearer() -> TestResult {
    let server = Binary::start(DEMO).await?;
    let cases = [
        (reqwest::Method::GET, "/v1/models"),
        (reqwest::Method::POST, "/v1/chat/completions"),
    ]
    .into_iter()
    .flat_map(|(method, path)| {
        [None, Some("Bearer "), Some("Basic ignored")]
            .map(move |token| (method.clone(), path, token))
    });
    for (method, path, token) in cases {
        let mut request = server
            .client
            .request(method, format!("{}{path}", server.base))
            .json(&request(0));
        if let Some(token) = token {
            request = request.header("Authorization", token);
        }
        assert_eq!(request.send().await?.status(), 401);
    }
    let response = server
        .client
        .get(format!("{}/v1/models", server.base))
        .bearer_auth(TOKEN)
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await?["data"][0]["id"], "fixture");
    let logs = server.stop().await?;
    assert_eq!(logs.lines().count(), 7);
    assert!(!logs.contains(TOKEN));
    assert!(!logs.contains("private-body-marker"));
    for line in logs.lines() {
        let record: Value = serde_json::from_str(line)?;
        assert_eq!(record.as_object().ok_or("not a log object")?.len(), 4);
        for key in ["method", "path", "turn", "status"] {
            assert!(record.get(key).is_some());
        }
    }
    Ok(())
}

#[tokio::test]
async fn llm_c_002_binary_streams_fragmented_tools_and_opt_in_usage() -> TestResult {
    let server = Binary::start(DEMO).await?;
    let mut body = request(0);
    body["stream_options"] = json!({"include_usage":true});
    let response = server
        .client
        .post(format!("{}/v1/chat/completions", server.base))
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await?;
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let records = chunks(&response.text().await?)?;
    assert_eq!(records.len(), 8);
    assert_eq!(
        records[0]["choices"][0]["delta"]["reasoning_content"],
        "A short README will make the project easy to inspect."
    );
    assert_eq!(
        records[1]["choices"][0]["delta"]["content"],
        "I'll create the project."
    );
    let calls: Vec<_> = records
        .iter()
        .filter_map(|chunk| chunk["choices"][0]["delta"]["tool_calls"].as_array())
        .collect();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0][0]["function"]["name"], "write_file");
    let arguments: String = calls
        .iter()
        .filter_map(|calls| calls[0]["function"]["arguments"].as_str())
        .collect();
    assert_eq!(
        serde_json::from_str::<Value>(&arguments)?["path"],
        "hello/README.md"
    );
    assert_eq!(records[6]["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(records[7]["choices"], json!([]));
    assert_eq!(records[7]["usage"]["prompt_tokens"], 20);
    assert!(
        chunks(&server.complete(request(0)).await?)?
            .iter()
            .all(|record| record.get("usage").is_none())
    );
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn web_e2e_004_binary_turn_selection_is_stateless_and_repeats_the_last_response() -> TestResult
{
    let server = Binary::start(DEMO).await?;
    let first = server.complete(request(0)).await?;
    let second = server.complete(request(1)).await?;
    assert!(second.contains("shell"));
    assert_eq!(server.complete(request(0)).await?, first);
    assert!(
        server
            .complete(request(2))
            .await?
            .contains("Created hello/README.md")
    );
    // The demo's second request changes the project (DEV-PLAN Phase 1, "and then change it").
    let change = server.complete(request(3)).await?;
    assert!(change.contains("edit_file") && change.contains("I'll add a usage section."));
    assert!(server.complete(request(4)).await?.contains("shell"));
    let final_answer = server.complete(request(5)).await?;
    assert!(final_answer.contains("Added a Usage section to hello/README.md."));
    assert_eq!(server.complete(request(30)).await?, final_answer);
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn llm_c_002_binary_rejects_other_routes_and_bounded_invalid_requests() -> TestResult {
    let server = Binary::start(DEMO).await?;
    for (method, path) in [
        (reqwest::Method::GET, "/other"),
        (reqwest::Method::GET, "/v1/chat/completions"),
        (reqwest::Method::POST, "/v1/models"),
    ] {
        assert_eq!(
            server
                .client
                .request(method, format!("{}{path}", server.base))
                .bearer_auth(TOKEN)
                .send()
                .await?
                .status(),
            404
        );
    }
    for body in [
        json!({}),
        json!({"stream":false,"messages":[]}),
        json!({"stream":true,"messages":"bad"}),
    ] {
        assert_eq!(
            server
                .client
                .post(format!("{}/v1/chat/completions", server.base))
                .bearer_auth(TOKEN)
                .json(&body)
                .send()
                .await?
                .status(),
            400
        );
    }
    assert_eq!(
        server
            .client
            .post(format!("{}/v1/chat/completions", server.base))
            .bearer_auth(TOKEN)
            .body("x".repeat(2 * 1024 * 1024 + 1))
            .send()
            .await?
            .status(),
        413
    );
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn web_e2e_004_sigterm_cancels_an_active_delayed_stream() -> TestResult {
    let mut scenario: Value = serde_json::from_str(DEMO)?;
    scenario["responses"][0]["delay_ms"] = json!(1000);
    let server = Binary::start(&scenario.to_string()).await?;
    let mut response = server
        .client
        .post(format!("{}/v1/chat/completions", server.base))
        .bearer_auth(TOKEN)
        .json(&request(0))
        .send()
        .await?;
    assert!(response.chunk().await?.is_some());
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn llm_e_016_binary_accepts_the_request_byte_limit_and_rejects_one_more() -> TestResult {
    let server = Binary::start(DEMO).await?;
    let mut body = request(2);
    body["padding"] = json!("");
    let bytes = 2 * 1024 * 1024 - serde_json::to_vec(&body)?.len();
    for extra in [0, 1] {
        body["padding"] = json!("x".repeat(bytes + extra));
        let encoded = serde_json::to_vec(&body)?;
        assert_eq!(encoded.len(), 2 * 1024 * 1024 + extra);
        let response = server
            .client
            .post(format!("{}/v1/chat/completions", server.base))
            .bearer_auth(TOKEN)
            .body(encoded)
            .send()
            .await?;
        assert_eq!(
            response.status().as_u16(),
            if extra == 0 { 200 } else { 413 }
        );
        response.bytes().await?;
    }
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn web_e2e_004_invalid_scenarios_are_refused_with_a_reason() -> TestResult {
    let demo: Value = serde_json::from_str(DEMO)?;
    let mut empty = demo.clone();
    empty["responses"] = json!([]);
    let mut fragment = demo.clone();
    fragment["responses"][0]["events"][2]["split_into"] = json!(0);
    let mut delay = demo.clone();
    delay["responses"][0]["delay_ms"] = json!(1001);
    let mut unknown = demo;
    unknown["unknown"] = json!(true);
    for (scenario, reason) in [
        ("{".into(), "scenario"),
        (empty.to_string(), "responses"),
        (fragment.to_string(), "fragment"),
        (delay.to_string(), "delay"),
        (unknown.to_string(), "unknown"),
    ] {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(scenario.as_bytes())?;
        let result = tokio::time::timeout(
            tqa_testkit::deadline::scaled(Duration::from_secs(5)),
            Command::new(env!("CARGO_BIN_EXE_tqa-fake-model"))
                .arg("--scenario")
                .arg(file.path())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        assert!(!result.status.success(), "accepted invalid scenario");
        assert!(String::from_utf8(result.stderr)?.contains(reason));
    }
    Ok(())
}

#[test]
fn web_e2e_004_demo_tool_arguments_match_the_real_registry() -> TestResult {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(WriteFile::new()))?;
    registry.register(Arc::new(Shell::new()))?;
    registry.register(Arc::new(EditFile::new()))?;
    registry.register(Arc::new(ReadFile::new()))?;
    registry.register(Arc::new(Grep::new()))?;
    registry.register(Arc::new(Glob::new()))?;
    assert_eq!(validated_calls(&registry, &serde_json::from_str(DEMO)?)?, 4);
    let scenario: Value = serde_json::from_str(&long_demo()?)?;
    assert!(validated_calls(&registry, &scenario)? >= 10);
    Ok(())
}

fn validated_calls(
    registry: &ToolRegistry,
    scenario: &Value,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut count = 0;
    for response in scenario["responses"]
        .as_array()
        .ok_or("missing responses")?
    {
        for event in response["events"].as_array().ok_or("missing events")? {
            if event["type"] == "tool_call" {
                registry.prepare(
                    event["name"].as_str().ok_or("missing name")?,
                    &event["arguments"],
                )?;
                count += 1;
            }
        }
    }
    Ok(count)
}
