//! GW-U-095: a model that thinks before answering although its route declares no reasoning (Ollama's qwen3:4b on
//! defaults thinks 279 to 577 tokens before its call) runs out of the plain check's 256 tokens. A step cut at the output
//! limit after reasoning is asked once more with room to think (4096, within the route's limit); when that passes, the
//! check passes and notes that the model thinks (`thinks_unasked`). A cut without reasoning is not retried.

use super::probe_tests::{
    NONCE, TestResult, call, limiter, lines, probe, route, start_model, usage,
};
use tqa_llm::{FinishReason, Usage};
use tqa_testkit::{Script, Scripted};

/// About 600 tokens of thinking, as the fake model streams it.
fn thinking() -> Scripted {
    Scripted::Reasoning("Let me think about which tool to call. ".repeat(75))
}
fn cut_usage(completion: u64) -> Scripted {
    Scripted::Usage(Usage {
        prompt_tokens: 20,
        completion_tokens: completion,
        cached_tokens: None,
        reasoning_tokens: Some(completion),
    })
}
/// The model's thinking runs out at the cap before its answer.
fn cut() -> Script {
    Script::new(vec![
        thinking(),
        Scripted::Finish(FinishReason::Length),
        cut_usage(256),
    ])
}
fn caps(server: &tqa_testkit::FakeModelServer) -> Vec<u64> {
    server
        .requests()
        .iter()
        .map(|request| request.body["max_tokens"].as_u64().unwrap_or(0))
        .collect()
}

// TEST: GW-U-095
#[tokio::test]
async fn gw_u_095_a_model_that_thinks_unasked_gets_room_and_the_check_says_so() -> TestResult {
    let value = format!("{NONCE}\n{NONCE}");
    let scripts = vec![
        cut(),
        Script::new(vec![thinking(), call(NONCE), usage()]),
        Script::new(vec![thinking(), Scripted::Text("ok".into()), usage()]),
        Script::new(vec![thinking(), lines(&value), usage()]),
    ];
    let server = start_model(scripts).await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    assert!(observed.ok, "{observed:?}");
    assert_eq!(observed.warnings, vec!["thinks_unasked"], "{observed:?}");
    // One retry of the cut step with room; the steps after it start with that room.
    assert_eq!(caps(&server), vec![256, 4096, 4096, 4096]);
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-095
#[tokio::test]
async fn gw_u_095_the_room_stays_within_the_routes_limit_and_is_asked_once() -> TestResult {
    // (the route's output limit, the scripts, the expected caps, the failure code if any)
    for (limit, scripts, expected, code) in [
        // The route's own limit bounds the room.
        (
            1024,
            vec![
                cut(),
                Script::new(vec![call(NONCE), usage()]),
                Script::new(vec![Scripted::Text("ok".into()), usage()]),
            ],
            vec![256, 1024, 1024],
            None,
        ),
        // Still cut with room: the check fails as before, after one retry.
        (
            4096,
            vec![cut(), cut()],
            vec![256, 4096],
            Some("no_tool_call"),
        ),
        // No room beyond the cap: nothing to retry.
        (256, vec![cut()], vec![256], Some("no_tool_call")),
        // Cut without reasoning: the model didn't think, so room wouldn't help.
        (
            4096,
            vec![Script::new(vec![
                Scripted::Text("Let me see".into()),
                Scripted::Finish(FinishReason::Length),
                usage(),
            ])],
            vec![256],
            Some("no_tool_call"),
        ),
    ] {
        let server = start_model(scripts).await?;
        let mut route = route("openai_chat", server.base_url(), "strict='off'\n")?;
        route.limits.max_output = Some(limit);
        let observed = probe(&route, limiter(&route)).await?;
        assert_eq!(caps(&server), expected, "{limit}: {observed:?}");
        assert_eq!(
            observed.failure.as_ref().map(|failure| failure.code),
            code,
            "{limit}: {observed:?}"
        );
        if let Some(failure) = &observed.failure {
            assert_eq!(failure.detail.as_deref(), Some("finish: length"), "{limit}");
            assert!(
                !observed.warnings.contains(&"thinks_unasked"),
                "{limit}: {observed:?}"
            );
        } else {
            assert_eq!(observed.warnings, vec!["thinks_unasked"], "{limit}");
        }
        server.shutdown().await?;
    }
    Ok(())
}

// TEST: GW-U-095
#[tokio::test]
async fn gw_u_095_a_round_trip_cut_while_thinking_is_asked_again_with_room() -> TestResult {
    let scripts = vec![
        Script::new(vec![call(NONCE), usage()]),
        cut(),
        Script::new(vec![thinking(), Scripted::Text("ok".into()), usage()]),
    ];
    let server = start_model(scripts).await?;
    let route = route("openai_chat", server.base_url(), "strict='off'\n")?;
    let observed = probe(&route, limiter(&route)).await?;
    assert!(observed.ok, "{observed:?}");
    assert_eq!(caps(&server), vec![256, 256, 4096]);
    assert_eq!(observed.warnings, vec!["thinks_unasked"]);
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-095
#[tokio::test]
async fn gw_u_095_a_route_that_declares_reasoning_already_has_the_room() -> TestResult {
    // The route asks for thinking, so its check starts with 4096 and no note is needed.
    let effort =
        "reasoning={field='reasoning_effort',allowed=['low'],default='low'}\nstrict='off'\n";
    let scripts = vec![
        Script::new(vec![thinking(), call(NONCE), usage()]),
        Script::new(vec![thinking(), Scripted::Text("ok".into()), usage()]),
    ];
    let server = start_model(scripts).await?;
    let route = route("openai_chat", server.base_url(), effort)?;
    let observed = probe(&route, limiter(&route)).await?;
    assert!(observed.ok, "{observed:?}");
    assert_eq!(caps(&server), vec![4096, 4096]);
    assert!(observed.warnings.is_empty(), "{observed:?}");
    server.shutdown().await?;
    Ok(())
}
