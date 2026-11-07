//! TK-U-030: `ext-check decision`, the conformance suite an engine's author runs (ADR-0013 point 6), against a scripted
//! engine. It asks every canned consultation at every exposure level, each carrying exactly what its level allows,
//! checks each answer with the loop's own rules and reports the loop's fallback code for each failure, the latency
//! over the answers and the first consultation's, and never the credential it sends.
use secrecy::SecretString;
use serde_json::json;
use std::time::Duration;
use tqa_ext_check::{
    decision::{EXPOSURES, Engine, cases, check_engine},
    report::{Report, Status, Target},
};
use tqa_testkit::{EngineAnswer, FakeEngineServer};
use tqa_types::{DecisionPoint, ToolCallId, decision_v1::Response};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The answer a well-behaved engine gives each canned consultation: its first route and tool, or its first call.
fn good() -> Result<Vec<EngineAnswer>, Box<dyn std::error::Error>> {
    let mut answers = Vec::new();
    for _ in EXPOSURES {
        for case in cases()? {
            answers.push(EngineAnswer::Decide(match case.point {
                DecisionPoint::BeforeStep => Response::CallModel {
                    route: case.state.routes[0].clone(),
                    tools: vec![case.state.tools[0].clone()],
                },
                _ => Response::Select {
                    call_ids: case
                        .response
                        .iter()
                        .filter_map(|response| response.tool_calls.first())
                        .map(|call| call.id.parse::<ToolCallId>())
                        .collect::<Result<_, _>>()?,
                },
            }));
        }
    }
    Ok(answers)
}

async fn run(
    engine: &FakeEngineServer,
    credential: Option<&str>,
) -> Result<Report, Box<dyn std::error::Error>> {
    let mut report = Report::new(
        Target::engine(engine.base_url(), 300, credential.is_some(), false),
        "0000000",
        "2026-09-29T00:00:00Z",
    );
    let target = Engine {
        url: engine.base_url().parse()?,
        timeout: Duration::from_millis(300),
        credential: credential.map(SecretString::from),
    };
    check_engine(&target, &mut report).await;
    Ok(report)
}
fn status_of(report: &Report, code: &str) -> Option<(Status, String)> {
    report
        .checks()
        .iter()
        .find(|check| check.code == code)
        .map(|check| (check.status, check.message.clone()))
}

// TEST: TK-U-030
#[tokio::test]
async fn tk_u_030_a_well_behaved_engine_passes_every_consultation_at_every_level() -> TestResult {
    let engine = FakeEngineServer::start(good()?).await?;
    let report = run(&engine, Some("synthetic-engine-key")).await?;
    let names: Vec<_> = cases()?.iter().map(|case| case.name).collect();
    for level in ["none", "calls", "full"] {
        for name in &names {
            let code = format!("decision.{level}.{name}");
            assert_eq!(
                status_of(&report, &code).map(|(status, _)| status),
                Some(Status::Pass),
                "{code}: {:?}",
                status_of(&report, &code)
            );
        }
    }
    assert_eq!(report.result(), "pass", "{}", report.text());
    let (status, latency) = status_of(&report, "decision.latency").ok_or("latency")?;
    assert_eq!(status, Status::Pass);
    assert!(
        latency.contains("p50") && latency.contains("p95") && latency.contains("over 18 answers"),
        "{latency}"
    );
    assert!(latency.contains("the first consultation took"), "{latency}");
    assert_eq!(
        status_of(&report, "decision.unavailability").map(|(status, _)| status),
        Some(Status::Skip)
    );
    // Each level's requests carry exactly what it allows: arguments from calls, the text and transcript at full.
    let seen = engine.requests();
    assert_eq!(seen.len(), 18);
    let per_level = seen.len() / 3;
    for (index, request) in seen.iter().enumerate() {
        let level = EXPOSURES[index / per_level];
        let body = serde_json::to_value(&request.request)?;
        let arguments = body["previous_calls"]
            .as_array()
            .into_iter()
            .chain(body["response_calls"].as_array())
            .flatten()
            .any(|call| call.get("arguments").is_some());
        let has_calls = !body["previous_calls"].as_array().is_none_or(Vec::is_empty)
            || !body["response_calls"].as_array().is_none_or(Vec::is_empty);
        assert_eq!(
            arguments,
            has_calls && level >= tqa_types::DecisionExposure::Calls,
            "{index}: {body}"
        );
        assert_eq!(
            body.get("transcript").is_some(),
            level == tqa_types::DecisionExposure::Full,
            "{index}"
        );
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer synthetic-engine-key")
        );
    }
    assert!(
        seen.iter()
            .any(|request| request.path == "/v1/after_response")
    );
    assert!(
        seen.iter().any(|request| request
            .request
            .tools
            .iter()
            .any(|tool| tool.name == "mcp.docs.search")),
        "an MCP tool name is in the step's tools"
    );
    // The credential is sent, never reported.
    let json = serde_json::to_string(&report)?;
    assert!(
        !report.text().contains("synthetic-engine-key") && !json.contains("synthetic-engine-key")
    );
    assert!(json.contains(r#""credential":true"#), "{json}");
    engine.stop().await;
    Ok(())
}

// TEST: TK-U-030
#[tokio::test]
async fn tk_u_030_each_wrong_answer_fails_its_consultation_with_the_loops_code() -> TestResult {
    let mut script = good()?;
    let raw = |status: u16, body: Vec<u8>| EngineAnswer::Raw { status, body };
    let wrong = [
        // none: before_step_first, before_step_later, before_step_one_route_one_tool, after_response_one_call
        EngineAnswer::Decide(Response::CallModel {
            route: "elsewhere".into(),
            tools: vec![],
        }),
        EngineAnswer::Decide(Response::CallModel {
            route: "local".into(),
            tools: vec!["delete_everything".into()],
        }),
        EngineAnswer::Decide(Response::Act {
            tool: "read_file".into(),
            arguments: json!({"path": 1}),
        }),
        EngineAnswer::Decide(Response::Select {
            call_ids: vec!["call_9".parse()?],
        }),
        // after_response_three_calls: a decision this point doesn't take
        EngineAnswer::Decide(Response::Compact {}),
        // after_response_text_only: a good answer
        script[5].clone(),
        // calls: an over-long stop reason, an answer over 64 KiB, a late answer, a server error
        raw(
            200,
            serde_json::to_vec(&json!({"decision": "stop", "reason": "x".repeat(201)}))?,
        ),
        raw(
            200,
            serde_json::to_vec(&json!({"decision": "compact", "pad": "x".repeat(70 * 1024)}))?,
        ),
        EngineAnswer::Delay(Duration::from_secs(1), Box::new(script[2].clone())),
        raw(500, b"{}".to_vec()),
    ];
    let count = wrong.len();
    script.splice(0..count, wrong);
    let engine = FakeEngineServer::start(script).await?;
    let report = run(&engine, None).await?;
    for (code, expected) in [
        ("decision.none.before_step_first", "route_not_allowed"),
        ("decision.none.before_step_later", "tool_not_allowed"),
        ("decision.none.before_step_one_route_one_tool", "schema"),
        ("decision.none.after_response_one_call", "invalid"),
        ("decision.none.after_response_three_calls", "invalid"),
        ("decision.calls.before_step_first", "invalid"),
        ("decision.calls.before_step_later", "invalid"),
        ("decision.calls.before_step_one_route_one_tool", "timeout"),
        ("decision.calls.after_response_one_call", "unreachable"),
    ] {
        let (status, message) = status_of(&report, code).ok_or(code)?;
        assert_eq!(status, Status::Fail, "{code}: {message}");
        assert!(
            message.contains(&format!("({expected})")),
            "{code}: {message}"
        );
    }
    assert_eq!(
        status_of(&report, "decision.none.after_response_text_only").map(|(status, _)| status),
        Some(Status::Pass)
    );
    assert_eq!(report.result(), "fail");
    assert!(report.text().contains("no answer within 300 ms"));
    // No credential given: none is reported as sent.
    assert!(serde_json::to_string(&report)?.contains(r#""credential":false"#));
    engine.stop().await;
    Ok(())
}
