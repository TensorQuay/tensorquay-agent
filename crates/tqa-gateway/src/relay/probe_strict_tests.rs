//! GW-U-064: on a route that sends strict tools, the probe checks that a strict tool's string argument keeps its line
//! breaks (docs/evals/gate-5.md, "Cause found"). When it doesn't but the same request without strict does, the probe
//! fails with the suggestion `strict = "off"`, and the route is left as it is. At most two more requests; none on a
//! route that sends no strict tool.

use super::{
    probe::Check,
    probe_tests::{
        NONCE, TestResult, call, good, limiter, lines, probe, route, start_model, usage,
    },
};
use std::sync::Arc;
use tqa_llm::TokioSleeper;
use tqa_testkit::{FakeModelOptions, FakeModelServer, Script, Scripted, StrictLineBreaks};

fn two_lines() -> String {
    format!("{NONCE}\n{NONCE}")
}

/// The first two steps, answered well.
fn first_steps() -> Vec<Script> {
    vec![
        Script::new(vec![call(NONCE), usage()]),
        Script::new(vec![Scripted::Text("ok".into()), usage()]),
    ]
}

/// Whether the recorded request sent `probe_lines` with `strict: true`, in either tool shape.
fn sent_strict(body: &serde_json::Value) -> Option<bool> {
    body["tools"].as_array()?.iter().find_map(|tool| {
        let definition = if tool["function"].is_object() {
            &tool["function"]
        } else {
            tool
        };
        (definition["name"] == "probe_lines").then(|| definition["strict"] == true)
    })
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_a_strict_tool_that_keeps_line_breaks_passes() -> TestResult {
    for (protocol, strict) in [
        ("openai_chat", ""),
        ("openai_responses", ""),
        ("openai_chat", "strict='on'\n"),
    ] {
        let server = start_model(good()).await?;
        let route = route(protocol, server.base_url(), strict)?;
        let observed = probe(&route, limiter(&route)).await?;
        assert!(observed.ok, "{protocol} {strict}: {observed:?}");
        assert_eq!(observed.strict_line_breaks, Check::Passed);
        let requests = server.requests();
        assert_eq!(requests.len(), 3, "one more request");
        assert_eq!(sent_strict(&requests[2].body), Some(true));
        assert!(
            requests[2]
                .body
                .to_string()
                .contains(&format!("{NONCE}\\n{NONCE}")),
            "the prompt asks for the two-line value: {}",
            requests[2].body
        );
        server.shutdown().await?;
    }
    Ok(())
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_line_breaks_lost_only_under_strict_suggest_strict_off() -> TestResult {
    for mode in [StrictLineBreaks::Strip] {
        for protocol in ["openai_chat", "openai_responses"] {
            let mut scripts = first_steps();
            // The same scripted answer twice: the fake loses the line break only when the tool is sent strict.
            scripts.push(Script::new(vec![lines(&two_lines()), usage()]));
            scripts.push(Script::new(vec![lines(&two_lines()), usage()]));
            let options = FakeModelOptions {
                strict_line_breaks: Some(mode),
                ..FakeModelOptions::default()
            };
            let server =
                FakeModelServer::start_with(options, scripts, Arc::new(TokioSleeper)).await?;
            let route = route(protocol, server.base_url(), "")?;
            let before = route.clone();
            let observed = probe(&route, limiter(&route)).await?;
            assert!(!observed.ok, "{mode:?} {protocol}");
            assert_eq!(observed.strict_line_breaks, Check::Failed);
            assert_eq!(
                (observed.tool_call, observed.round_trip),
                (Check::Passed, Check::Passed),
                "the other checks keep their outcome"
            );
            let failure = observed.failure.as_ref().ok_or("no failure")?;
            assert_eq!(failure.code, "strict_line_breaks_lost");
            let suggestion = failure.suggestion.as_ref().ok_or("no suggestion")?;
            assert_eq!(
                (suggestion.field, suggestion.value.as_ref()),
                ("strict", "off")
            );
            let requests = server.requests();
            assert_eq!(requests.len(), 4, "at most two more requests");
            assert_eq!(sent_strict(&requests[2].body), Some(true));
            assert_eq!(
                sent_strict(&requests[3].body),
                Some(false),
                "the retry sends no strict"
            );
            assert_eq!(route, before, "the route is left as it is");
            server.shutdown().await?;
        }
    }
    Ok(())
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_a_model_that_never_writes_the_line_break_only_warns() -> TestResult {
    let mut scripts = first_steps();
    // A model that ignores the prompt, with and without strict: strict isn't shown to be the cause.
    scripts.push(Script::new(vec![lines(NONCE), usage()]));
    scripts.push(Script::new(vec![Scripted::Text("no".into()), usage()]));
    let server = start_model(scripts).await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    assert!(observed.ok, "{observed:?}");
    assert_eq!(observed.strict_line_breaks, Check::NotRun);
    assert_eq!(observed.warnings, ["line_breaks_unverified"]);
    assert_eq!(server.requests().len(), 4);
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_a_strict_refusal_keeps_its_suggestion() -> TestResult {
    let mut scripts = first_steps();
    scripts.push(Script::new(vec![Scripted::Status {
        code: 400,
        body: r#"{"error":{"message":"Invalid schema for function 'probe_lines': strict mode"}}"#
            .into(),
        retry_after: None,
    }]));
    let server = start_model(scripts).await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    assert_eq!(observed.strict_line_breaks, Check::Failed);
    let failure = observed.failure.as_ref().ok_or("no failure")?;
    assert_eq!(
        (failure.code, failure.status),
        ("upstream_error", Some(400))
    );
    assert_eq!(
        failure.suggestion.as_ref().map(|hint| hint.value.as_ref()),
        Some("off")
    );
    assert_eq!(server.requests().len(), 3, "a refusal is not retried");
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_a_route_that_sends_no_strict_tool_is_not_checked() -> TestResult {
    for (protocol, extra, expected) in [
        ("openai_chat", "strict='off'\n", Check::NotRun),
        (
            "openai_responses",
            "schema_keywords='basic'\n",
            Check::NotRun,
        ),
        ("anthropic_messages", "", Check::NotRun),
        ("openai_chat", "native_tools=false\n", Check::NotDeclared),
    ] {
        let scripts = if extra.starts_with("native_tools") {
            vec![Script::new(vec![Scripted::Text("ok".into()), usage()])]
        } else {
            first_steps()
        };
        let server = start_model(scripts).await?;
        let route = route(protocol, server.base_url(), extra)?;
        let observed = probe(&route, limiter(&route)).await?;
        assert!(observed.ok, "{protocol} {extra}: {observed:?}");
        assert_eq!(observed.strict_line_breaks, expected, "{protocol} {extra}");
        assert!(
            server
                .requests()
                .iter()
                .all(|request| sent_strict(&request.body).is_none()),
            "{protocol} {extra}: no strict-string request"
        );
        server.shutdown().await?;
    }
    Ok(())
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_a_strict_answer_that_is_not_the_asked_call_is_inconclusive() -> TestResult {
    let other = |name: &str, arguments: &str| Scripted::ToolCall {
        name: name.into(),
        arguments_json: arguments.into(),
        split_into: 1,
    };
    for answer in [
        Scripted::Text("no call".into()),
        other(
            "probe_echo",
            &serde_json::json!({ "value": NONCE }).to_string(),
        ),
        other("probe_lines", "[1]"),
        other(
            "probe_lines",
            &serde_json::json!({ "value": 7 }).to_string(),
        ),
        other(
            "probe_lines",
            &serde_json::json!({ "value": "something else" }).to_string(),
        ),
    ] {
        let mut scripts = first_steps();
        scripts.push(Script::new(vec![answer.clone(), usage()]));
        // A retry would find this line break and wrongly blame strict mode.
        scripts.push(Script::new(vec![lines(&two_lines()), usage()]));
        let server = start_model(scripts).await?;
        let route = route("openai_chat", server.base_url(), "")?;
        let observed = probe(&route, limiter(&route)).await?;
        assert!(observed.ok, "{answer:?}: {observed:?}");
        assert_eq!(observed.strict_line_breaks, Check::NotRun, "{answer:?}");
        assert_eq!(observed.warnings, ["line_breaks_unverified"], "{answer:?}");
        assert_eq!(server.requests().len(), 3, "{answer:?}: no retry");
        server.shutdown().await?;
    }
    Ok(())
}

/// A strict answer the model cut off at the line break, as a strict endpoint was seen to: `{"value": "<nonce>`.
fn cut() -> Scripted {
    Scripted::ToolCall {
        name: "probe_lines".into(),
        arguments_json: format!("{{\"value\": \"{NONCE}"),
        split_into: 1,
    }
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_a_strict_answer_cut_or_ended_on_length_counts_as_lost() -> TestResult {
    let length = vec![
        Scripted::Text("Calling the tool with".into()),
        Scripted::Finish(tqa_llm::FinishReason::Length),
        usage(),
    ];
    for (name, strict_answer) in [
        ("cut arguments", vec![cut(), usage()]),
        (
            "arguments not JSON",
            vec![
                Scripted::ToolCall {
                    name: "probe_lines".into(),
                    arguments_json: "{not json".into(),
                    split_into: 1,
                },
                usage(),
            ],
        ),
        ("ended on length", length),
    ] {
        let mut scripts = first_steps();
        scripts.push(Script::new(strict_answer));
        scripts.push(Script::new(vec![lines(&two_lines()), usage()]));
        let server = start_model(scripts).await?;
        let route = route("openai_chat", server.base_url(), "")?;
        let observed = probe(&route, limiter(&route)).await?;
        let failure = observed.failure.as_ref().ok_or("no failure")?;
        assert_eq!(
            failure.code, "strict_line_breaks_lost",
            "{name}: {observed:?}"
        );
        assert_eq!(
            failure.suggestion.as_ref().map(|hint| hint.value.as_ref()),
            Some("off"),
            "{name}"
        );
        assert_eq!(observed.strict_line_breaks, Check::Failed, "{name}");
        assert_eq!(
            (observed.tool_call, observed.round_trip),
            (Check::Passed, Check::Passed),
            "{name}"
        );
        assert_eq!(server.requests().len(), 4, "{name}: one retry");
        server.shutdown().await?;
    }
    Ok(())
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_a_retry_that_fails_too_only_warns() -> TestResult {
    let mut scripts = first_steps();
    scripts.push(Script::new(vec![cut(), usage()]));
    scripts.push(Script::new(vec![cut(), usage()]));
    let server = start_model(scripts).await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    assert!(observed.ok, "never a probe failure: {observed:?}");
    assert_eq!(observed.strict_line_breaks, Check::NotRun);
    assert_eq!(observed.warnings, ["line_breaks_unverified"]);
    assert_eq!(server.requests().len(), 4);
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-064
#[tokio::test]
async fn gw_u_064_the_check_has_its_own_allowance_within_the_routes_limit() -> TestResult {
    for (limit, expected) in [(4096, 256), (100, 100)] {
        let server = start_model(good()).await?;
        let mut route = route("openai_chat", server.base_url(), "")?;
        route.limits.max_output = Some(limit);
        let observed = probe(&route, limiter(&route)).await?;
        assert!(observed.ok, "{limit}: {observed:?}");
        let requests = server.requests();
        assert_eq!(
            requests[2].body["max_tokens"], expected,
            "{limit}: the check's request"
        );
        assert_eq!(
            requests[0].body["max_tokens"],
            limit.min(256),
            "{limit}: probe_echo keeps the probe's own allowance"
        );
        server.shutdown().await?;
    }
    Ok(())
}
