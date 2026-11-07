//! RT-I-031: the runtime's decision engine over the gateway relay (ADR-0013 point 6), against the fake `decision/v1`
//! engine standing in for the relay. A valid answer becomes the loop's decision; no answer in time is `timeout`; a
//! refused or unreachable relay is `unreachable`; a malformed, oversized or out-of-place answer is `invalid`. The
//! runtime sends only its own token and never an engine credential or address, and it sends what the engine's history
//! exposure allows and nothing more.
use secrecy::SecretString;
use serde_json::json;
use std::{error::Error, time::Duration};
use tqa_core::{BudgetUse, CallSummary, Decision, DecisionEngine, EngineFailure, RunState};
use tqa_llm::{Completion, ToolCall};
use tqa_runtime::testing::{RelayDecisionEngine, RelayEngineConfig};
use tqa_testkit::{EngineAnswer, FakeEngineServer};
use tqa_types::{DecisionExposure, DecisionPoint, ToolStatus, decision_v1};

type TestResult = Result<(), Box<dyn Error>>;

fn engine(
    base: &str,
    timeout: Duration,
    exposure: DecisionExposure,
) -> Result<RelayDecisionEngine, Box<dyn Error>> {
    Ok(RelayDecisionEngine::new(RelayEngineConfig {
        name: "router".into(),
        base: base.parse()?,
        token: SecretString::from("runtime-token"),
        timeout,
        exposure,
        tools: vec![(
            "read_file".into(),
            "Read a file".into(),
            json!({"type":"object"}),
        )],
        routes: vec![decision_v1::Route {
            name: "local".into(),
            context_window: Some(32768),
            max_output: Some(8192),
            native_tools: true,
        }],
        points: None,
    })?)
}
fn state() -> Result<RunState, Box<dyn Error>> {
    Ok(RunState {
        run_id: "r-1".parse()?,
        step: 2,
        attempt: 1,
        route: "local".into(),
        routes: vec!["local".into(), "cheap".into()],
        tools: vec!["read_file".into()],
        budget: BudgetUse {
            steps_used: 1,
            max_steps: 30,
            output_tokens_used: 100,
            max_output_tokens: 10_000,
        },
        previous_calls: vec![CallSummary {
            name: "read_file".into(),
            status: Some(ToolStatus::Ok),
            arguments: None,
        }],
        transcript: None,
    })
}
fn response() -> Completion {
    Completion {
        text: "reading the secret".into(),
        tool_calls: vec![ToolCall {
            id: "a".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"secret.txt"}"#.into(),
        }],
        ..Completion::default()
    }
}

#[tokio::test]
async fn rt_i_031_each_answer_becomes_the_loops_decision() -> TestResult {
    let fake = FakeEngineServer::start(vec![
        EngineAnswer::Decide(decision_v1::Response::CallModel {
            route: "cheap".into(),
            tools: vec!["read_file".into()],
        }),
        EngineAnswer::Decide(decision_v1::Response::Act {
            tool: "read_file".into(),
            arguments: json!({"path":"a"}),
        }),
        EngineAnswer::Decide(decision_v1::Response::Compact {}),
        EngineAnswer::Decide(decision_v1::Response::Stop {
            reason: "out of scope".into(),
        }),
    ])
    .await?;
    let relay = engine(
        fake.base_url(),
        Duration::from_secs(2),
        DecisionExposure::None,
    )?;
    let state = state()?;
    assert_eq!(
        relay.before_step(&state).await,
        Ok(Decision::CallModel {
            route: "cheap".into(),
            tool_subset: vec!["read_file".into()],
        })
    );
    match relay.before_step(&state).await {
        Ok(Decision::Act(call)) => {
            assert_eq!(call.name, "read_file");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&call.arguments)?,
                json!({"path":"a"})
            );
        }
        other => return Err(format!("expected an act, got {other:?}").into()),
    }
    assert_eq!(relay.before_step(&state).await, Ok(Decision::Compact));
    assert_eq!(
        relay.before_step(&state).await,
        Ok(Decision::Stop("out of scope".into()))
    );
    let seen = fake.requests();
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[0].path, "/v1/before_step");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some("Bearer runtime-token"),
        "only the runtime's own token"
    );
    let request = &seen[0].request;
    assert_eq!(request.point, DecisionPoint::BeforeStep);
    assert_eq!((request.step, request.budget.steps_used), (2, 1));
    assert_eq!(request.tools[0].description, "Read a file");
    assert_eq!(request.routes[0].context_window, Some(32768));
    assert_eq!(
        request.routes[1].name, "cheap",
        "a route without facts still goes by name"
    );
    fake.stop().await;
    Ok(())
}

#[tokio::test]
async fn rt_i_031_failures_map_to_their_codes() -> TestResult {
    let big = vec![b'x'; 70 * 1024];
    let fake = FakeEngineServer::start(vec![
        EngineAnswer::Raw {
            status: 200,
            body: b"not json".to_vec(),
        },
        EngineAnswer::Raw {
            status: 200,
            body: big,
        },
        EngineAnswer::Raw {
            status: 500,
            body: Vec::new(),
        },
        EngineAnswer::Decide(decision_v1::Response::Select {
            call_ids: vec!["a".parse()?],
        }),
        EngineAnswer::Hang,
    ])
    .await?;
    let relay = engine(
        fake.base_url(),
        Duration::from_millis(300),
        DecisionExposure::None,
    )?;
    let state = state()?;
    assert_eq!(
        relay.before_step(&state).await,
        Err(EngineFailure::Invalid),
        "malformed"
    );
    assert_eq!(
        relay.before_step(&state).await,
        Err(EngineFailure::Invalid),
        "over 64 KiB"
    );
    assert_eq!(
        relay.before_step(&state).await,
        Err(EngineFailure::Unreachable),
        "refused"
    );
    assert_eq!(
        relay.before_step(&state).await,
        Err(EngineFailure::Invalid),
        "a selection before a step"
    );
    let started = std::time::Instant::now();
    assert_eq!(relay.before_step(&state).await, Err(EngineFailure::Timeout));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "bounded: {:?}",
        started.elapsed()
    );
    fake.stop().await;
    // Nothing listening.
    let closed = engine(
        "http://127.0.0.1:9",
        Duration::from_secs(2),
        DecisionExposure::None,
    )?;
    assert_eq!(
        closed.before_step(&state).await,
        Err(EngineFailure::Unreachable)
    );
    Ok(())
}

#[tokio::test]
async fn rt_i_031_the_request_carries_only_what_the_exposure_allows() -> TestResult {
    for exposure in [
        DecisionExposure::None,
        DecisionExposure::Calls,
        DecisionExposure::Full,
    ] {
        let fake =
            FakeEngineServer::start(vec![EngineAnswer::Decide(decision_v1::Response::Select {
                call_ids: vec!["a".parse()?],
            })])
            .await?;
        let relay = engine(fake.base_url(), Duration::from_secs(2), exposure)?;
        let planned = relay
            .after_response(&state()?, &response())
            .await
            .map_err(|failure| format!("{failure:?}"))?;
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].call_id, "a");
        let seen = fake.requests();
        assert_eq!(seen[0].path, "/v1/after_response");
        let request = &seen[0].request;
        assert_eq!(request.response_calls[0].name, "read_file");
        let arguments = request.response_calls[0].arguments.clone();
        match exposure {
            DecisionExposure::None => {
                assert_eq!(arguments, None);
                assert_eq!(request.response_text, None);
            }
            DecisionExposure::Calls => {
                assert_eq!(arguments, Some(json!({"path":"secret.txt"})));
                assert_eq!(request.response_text, None);
            }
            _ => assert_eq!(request.response_text.as_deref(), Some("reading the secret")),
        }
        fake.stop().await;
    }
    Ok(())
}

#[test]
fn rt_i_031_the_timeout_is_bounded() -> TestResult {
    assert_eq!(
        engine(
            "http://127.0.0.1:9",
            Duration::from_secs(30),
            DecisionExposure::None
        )?
        .timeout(),
        Duration::from_secs(10)
    );
    assert_eq!(
        engine("http://127.0.0.1:9", Duration::ZERO, DecisionExposure::None)?.timeout(),
        Duration::from_secs(2)
    );
    Ok(())
}

#[tokio::test]
async fn rt_i_031_a_call_with_a_made_up_name_is_left_out() -> TestResult {
    // The relay refuses a name that isn't a tool name, since it could carry text; the runtime leaves such a call out
    // rather than lose the consultation.
    let fake = FakeEngineServer::start(vec![EngineAnswer::Decide(decision_v1::Response::Select {
        call_ids: vec!["a".parse()?],
    })])
    .await?;
    let relay = engine(
        fake.base_url(),
        Duration::from_secs(2),
        DecisionExposure::None,
    )?;
    let mut state = state()?;
    state.previous_calls.push(CallSummary {
        name: "run the tests please".into(),
        status: Some(ToolStatus::Error),
        arguments: None,
    });
    let mut response = response();
    response.tool_calls.push(ToolCall {
        id: "b".into(),
        name: "read the secret".into(),
        arguments: "{}".into(),
    });
    let planned = relay
        .after_response(&state, &response)
        .await
        .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!(planned.len(), 1);
    let seen = fake.requests();
    assert_eq!(
        seen[0]
            .request
            .previous_calls
            .iter()
            .map(|call| call.name.as_str())
            .collect::<Vec<_>>(),
        ["read_file"]
    );
    assert_eq!(
        seen[0]
            .request
            .response_calls
            .iter()
            .map(|call| call.name.as_str())
            .collect::<Vec<_>>(),
        ["read_file"]
    );
    fake.stop().await;
    Ok(())
}

#[tokio::test]
async fn rt_i_031_the_gateways_unusable_answer_is_invalid_a_spent_budget_its_own_and_any_other_refusal_unreachable()
-> TestResult {
    let fake = FakeEngineServer::start(vec![
        // The gateway's typed-question translation found the engine's answer unusable: invalid, not unreachable.
        EngineAnswer::Raw {
            status: 502,
            body: br#"{"error":{"code":"decision_engine_invalid","message":"x","retryable":true}}"#.to_vec(),
        },
        // A spent budget is its own failure, so the loop records budget_spent and stops asking for the run.
        EngineAnswer::Raw {
            status: 409,
            body: br#"{"error":{"code":"decision_engine_budget_spent","message":"x","retryable":false}}"#.to_vec(),
        },
        // Any other 502 or 409 stays unreachable.
        EngineAnswer::Raw {
            status: 502,
            body: br#"{"error":{"code":"upstream_unavailable","message":"x","retryable":true}}"#.to_vec(),
        },
        EngineAnswer::Raw {
            status: 409,
            body: br#"{"error":{"code":"conflict","message":"x","retryable":false}}"#.to_vec(),
        },
    ])
    .await?;
    let relay = engine(
        fake.base_url(),
        Duration::from_secs(2),
        DecisionExposure::None,
    )?;
    let state = state()?;
    assert_eq!(relay.before_step(&state).await, Err(EngineFailure::Invalid));
    assert_eq!(
        relay.before_step(&state).await,
        Err(EngineFailure::BudgetSpent)
    );
    assert_eq!(
        relay.before_step(&state).await,
        Err(EngineFailure::Unreachable)
    );
    assert_eq!(
        relay.before_step(&state).await,
        Err(EngineFailure::Unreachable)
    );
    fake.stop().await;
    Ok(())
}
