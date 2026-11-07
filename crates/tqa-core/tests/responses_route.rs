//! CORE-E-038: runs on an `openai_responses` route through the real adapter, and fallbacks between
//! a Chat route and a Responses route in both directions. LLM-U-035: a run's requests on that route carry neither
//! `metadata` nor `temperature`, which some Responses backends refuse.
mod support;
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RouteProvider, RunOutcome, Runner, UserTurn};
use tqa_llm::{HttpConfig, ModelProvider, Registry, StreamLimits, provider_for};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};
use tqa_types::{Event, RouteSnapshot, WireProtocol};

/// A real adapter for `protocol` against a fake model server. Its clock is never advanced, so HTTP
/// deadlines cannot fire in the harness's fake time.
fn adapter(
    name: &str,
    model: &str,
    protocol: &str,
    server: &FakeModelServer,
) -> Result<Arc<dyn ModelProvider>, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='{name}'\nprotocol='{protocol}'\nbase_url='{}'\nmodel='{model}'\nauth={{kind='none'}}\nnative_tools=true\n",
        server.base_url()
    ))?;
    let clock = FakeClock::new();
    Ok(provider_for(
        registry.get(name).ok_or("missing route")?.clone(),
        None,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?)
}

fn wire(protocol: &str) -> WireProtocol {
    if protocol == "openai_responses" {
        WireProtocol::OpenAiResponses
    } else {
        WireProtocol::OpenAiChat
    }
}

/// The fake model server path each protocol posts to.
fn path(protocol: &str) -> &'static str {
    if protocol == "openai_responses" {
        "/v1/responses"
    } else {
        "/v1/chat/completions"
    }
}

fn two_steps() -> Vec<Script> {
    vec![
        Script::new(vec![
            Scripted::Reasoning("Look first.".into()),
            Scripted::ToolCall {
                name: "read_file".into(),
                arguments_json: r#"{"path":"a.txt"}"#.into(),
                split_into: 3,
            },
        ]),
        Script::new(vec![Scripted::Text("The file says ok.".into())]),
    ]
}

fn plain_turn() -> UserTurn {
    UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "read a.txt".into(),
        attachments: vec![],
    }
}

fn texts(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            Event::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_e_038_a_two_step_tool_run_on_a_responses_route() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let mut h = Harness::new(vec![], registry(&[tool])?)?;
    let server = FakeModelServer::start(two_steps(), Arc::new(h.sleeper.clone())).await?;
    h.deps.provider = adapter("test", "fake-model", "openai_responses", &server)?;
    h.deps.route.protocol = WireProtocol::OpenAiResponses;
    let sink = h.sink.clone();
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(plain_turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let events = sink.events();
    let call_id = events
        .iter()
        .find_map(|event| match event {
            Event::ToolCall {
                call_id,
                name,
                arguments,
                ..
            } if name == "read_file" => {
                assert_eq!(arguments, &serde_json::json!({"path":"a.txt"}));
                Some(call_id.to_string())
            }
            _ => None,
        })
        .ok_or("no tool call")?;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ReasoningDelta { text } if text == "Look first."))
    );
    assert_eq!(texts(&events), "The file says ok.");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.path == "/v1/responses")
    );
    let input = requests[1].body["input"].as_array().ok_or("no input")?;
    let call = input
        .iter()
        .find(|item| item["type"] == "function_call")
        .ok_or("the call was not sent back")?;
    let result = input
        .iter()
        .find(|item| item["type"] == "function_call_output")
        .ok_or("the result was not sent back")?;
    assert_eq!(call["call_id"], call_id.as_str());
    assert_eq!(result["call_id"], call_id.as_str());
    assert_eq!(requests[1].body["store"], false);
    assert!(requests[1].body.get("previous_response_id").is_none());
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn core_e_038_chat_and_responses_routes_fall_back_to_each_other() -> TestResult {
    for (primary, backup) in [
        ("openai_chat", "openai_responses"),
        ("openai_responses", "openai_chat"),
    ] {
        let tool = fake("read_file", true, vec![output("ok", 0)])?;
        let mut h = Harness::new(vec![], registry(&[tool])?)?;
        let busy = FakeModelServer::start(
            vec![Script::new(vec![Scripted::Status {
                code: 429,
                body: "busy".into(),
                retry_after: Some("90".into()),
            }])],
            Arc::new(h.sleeper.clone()),
        )
        .await?;
        let spare = FakeModelServer::start(two_steps(), Arc::new(h.sleeper.clone())).await?;
        h.deps.provider = adapter("test", "fake-model", primary, &busy)?;
        h.deps.route.protocol = wire(primary);
        h.deps.fallback_routes = vec![RouteProvider {
            route: RouteSnapshot {
                name: "backup".into(),
                relay_path: "/relay/r/backup".into(),
                model: "backup-model".into(),
                protocol: wire(backup),
                ..route()
            },
            provider: adapter("backup", "backup-model", backup, &spare)?,
        }];
        let sink = h.sink.clone();
        let runner = Runner::new(h.deps)?;
        let outcome = drive(
            runner.run(
                &[],
                Some(plain_turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            ),
            &h.clock,
        )
        .await??;
        assert_eq!(outcome, RunOutcome::Completed, "{primary} -> {backup}");
        let events = sink.events();
        assert!(events.iter().any(|event| matches!(event,
            Event::RouteFallback { from, to, .. } if from == "test" && to == "backup")));
        assert_eq!(texts(&events), "The file says ok.");

        assert_eq!(busy.requests().len(), 1);
        assert_eq!(busy.requests()[0].path, path(primary));
        let spare_requests = spare.requests();
        assert_eq!(spare_requests.len(), 2, "both steps ran on the fallback");
        assert!(
            spare_requests
                .iter()
                .all(|request| request.path == path(backup))
        );
        busy.shutdown().await?;
        spare.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn llm_u_035_a_run_request_on_a_responses_route_carries_neither_metadata_nor_temperature()
-> TestResult {
    // A two-step run with a native tool: both requests, as they reach the wire through the real adapter.
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let mut h = Harness::new(vec![], registry(&[tool])?)?;
    let server = FakeModelServer::start(two_steps(), Arc::new(h.sleeper.clone())).await?;
    h.deps.provider = adapter("test", "fake-model", "openai_responses", &server)?;
    h.deps.route.protocol = WireProtocol::OpenAiResponses;
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(plain_turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.path, "/v1/responses");
        for field in ["metadata", "temperature"] {
            assert!(
                request.body.get(field).is_none(),
                "{field} was sent: {}",
                request.body
            );
        }
    }
    server.shutdown().await?;
    Ok(())
}
