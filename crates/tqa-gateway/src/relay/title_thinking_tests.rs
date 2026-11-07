//! GW-U-099 (candidate 13): a model that thinks although its route declares no reasoning (Ollama's qwen3:4b on its
//! defaults) spends the plain title call's 32 tokens thinking and names nothing, so the task kept its first words. A
//! title answer cut at its cap after reasoning, with no text, is asked once more with room to think (2048, within the
//! route's own limit), as the route check does (GW-U-095). A plain model keeps the one cheap call, and a cut without
//! reasoning is not retried.

use super::{
    probe_tests::{TestResult, route, start_model},
    title::{answer, clean, request},
};
use std::time::Duration;
use tqa_llm::{FinishReason, Usage};
use tqa_testkit::{FakeModelServer, Script, Scripted};

const IDLE: Duration = Duration::from_secs(5);

/// About 300 tokens of thinking, as the fake model streams it.
fn thinking() -> Scripted {
    Scripted::Reasoning("The task asks for a report on quarterly sales. ".repeat(30))
}
fn used(completion: u64, reasoning: u64) -> Scripted {
    Scripted::Usage(Usage {
        prompt_tokens: 40,
        completion_tokens: completion,
        cached_tokens: None,
        reasoning_tokens: Some(reasoning),
    })
}
fn caps(server: &FakeModelServer) -> Vec<u64> {
    server
        .requests()
        .iter()
        .map(|request| request.body["max_tokens"].as_u64().unwrap_or(0))
        .collect()
}

// TEST: GW-U-099
#[tokio::test]
async fn gw_u_099_a_model_that_thinks_first_is_asked_again_with_room_and_names_the_task()
-> TestResult {
    let server = start_model(vec![
        Script::new(vec![
            thinking(),
            Scripted::Finish(FinishReason::Length),
            used(32, 32),
        ]),
        Script::new(vec![
            thinking(),
            Scripted::Text("Quarterly sales report".into()),
            used(320, 300),
        ]),
    ])
    .await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let mut asked = request("Write the quarterly sales report for the team", &route);
    let (named, usage) = answer(&reqwest::Client::new(), IDLE, &route, None, &mut asked).await?;
    assert_eq!(caps(&server), [32, 2048]);
    assert_eq!(
        clean(&named.text).as_deref(),
        Some("Quarterly sales report")
    );
    // Both calls count as the title's usage.
    assert_eq!(usage.map(|usage| usage.completion_tokens), Some(352));
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-099
#[tokio::test]
async fn gw_u_099_a_plain_model_keeps_one_cheap_call_and_a_cut_without_thinking_is_not_retried()
-> TestResult {
    // A plain model answers within 32 tokens: one call.
    let server = start_model(vec![Script::new(vec![
        Scripted::Text("Quarterly sales report".into()),
        used(5, 0),
    ])])
    .await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let mut asked = request("Write the quarterly sales report for the team", &route);
    let (named, _) = answer(&reqwest::Client::new(), IDLE, &route, None, &mut asked).await?;
    assert_eq!(
        (caps(&server), named.text.as_str()),
        (vec![32], "Quarterly sales report")
    );
    server.shutdown().await?;
    // Cut at the cap with no thinking: the answer as it came, no second call.
    let server = start_model(vec![Script::new(vec![
        Scripted::Text("A very long rambling name that".into()),
        Scripted::Finish(FinishReason::Length),
        used(32, 0),
    ])])
    .await?;
    let route = super::probe_tests::route("openai_chat", server.base_url(), "")?;
    let mut asked = request("Write the quarterly sales report for the team", &route);
    answer(&reqwest::Client::new(), IDLE, &route, None, &mut asked).await?;
    assert_eq!(caps(&server), [32]);
    server.shutdown().await?;
    // The room stays within the route's own limit.
    let server = start_model(vec![
        Script::new(vec![
            thinking(),
            Scripted::Finish(FinishReason::Length),
            used(32, 32),
        ]),
        Script::new(vec![
            thinking(),
            Scripted::Text("Sales report".into()),
            used(600, 590),
        ]),
    ])
    .await?;
    let small = super::probe_tests::route("openai_chat", server.base_url(), "")?;
    let small = tqa_llm::Route {
        limits: tqa_llm::Limits {
            max_output: Some(600),
            ..small.limits
        },
        ..small
    };
    let mut asked = request("Write the quarterly sales report for the team", &small);
    answer(&reqwest::Client::new(), IDLE, &small, None, &mut asked).await?;
    assert_eq!(caps(&server), [32, 600]);
    server.shutdown().await?;
    Ok(())
}
