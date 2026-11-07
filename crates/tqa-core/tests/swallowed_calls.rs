//! CORE-U-065 (Gate 5, 29 Sep): on a text-protocol route, a reply that delivered no call while the server held part of
//! it back (withheld chunks, or an empty reply that reports tokens) is asked again once in that step with the tools
//! declared natively, since the server's own tool-call parser took the call the model wrote in its native format. The
//! retry is a `provider.retry` whose reason names it and the withheld chunks. A genuine final answer (nothing held
//! back) is never retried; a step retries at most once; a native route never does; and a server that refuses declared
//! tools marks the route text-only for the run, which then goes on as before.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk, Usage};
use tqa_testkit::ProviderReply;
use tqa_types::Event;

fn tokens(completion: u64) -> StreamChunk {
    StreamChunk::Usage(Usage {
        prompt_tokens: 100,
        completion_tokens: completion,
        cached_tokens: None,
        reasoning_tokens: None,
    })
}
/// Prose, then `withheld` chunks the server held back, then a normal stop: a call swallowed after a sentence.
fn swallowed(withheld: usize) -> ProviderReply {
    let mut chunks = vec![StreamChunk::TextDelta(
        "I'll start by reading the file.\n\n".into(),
    )];
    chunks.extend((0..withheld).map(|_| StreamChunk::Withheld));
    chunks.push(StreamChunk::Finish(FinishReason::Stop));
    chunks.push(tokens(60));
    reply(chunks)
}
/// Nothing at all, though the server reports tokens.
fn empty_with_tokens() -> ProviderReply {
    reply(vec![StreamChunk::Finish(FinishReason::Stop), tokens(40)])
}
/// The model's call as the server returns it once the tools are declared: a native tool call.
fn native_call() -> ProviderReply {
    reply(vec![
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some("call_7".into()),
            name: Some("read_file".into()),
            arguments_delta: r#"{"path":"a"}"#.into(),
        },
        StreamChunk::Finish(FinishReason::ToolCalls),
        tokens(20),
    ])
}
fn done() -> ProviderReply {
    reply(vec![
        StreamChunk::TextDelta("The file says hello; nothing else to do.".into()),
        StreamChunk::Finish(FinishReason::Stop),
        tokens(12),
    ])
}

struct Run {
    outcome: RunOutcome,
    events: Vec<Event>,
    /// For each request: whether it declared the tools natively.
    declared: Vec<bool>,
    reads: usize,
}
async fn run(script: Vec<ProviderReply>, native: bool) -> Result<Run, Box<dyn std::error::Error>> {
    let tool = fake(
        "read_file",
        true,
        (0..4).map(|_| output("hello", 0)).collect(),
    )?;
    let mut h = Harness::new(script, registry(std::slice::from_ref(&tool))?)?;
    h.deps.route.native_tools = native;
    let (sink, provider) = (h.sink.clone(), h.provider.clone());
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    Ok(Run {
        outcome,
        events: sink.events(),
        declared: provider
            .requests()
            .iter()
            .map(|request| request.declare_tools && !request.tools.is_empty())
            .collect(),
        reads: tool.calls().len(),
    })
}
fn retries(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

// TEST: CORE-U-065
#[tokio::test]
async fn core_u_065_a_swallowed_call_is_asked_again_once_with_the_tools_declared() -> TestResult {
    let run = run(vec![swallowed(26), native_call(), done()], false).await?;
    assert!(
        matches!(run.outcome, RunOutcome::Completed),
        "{:?}",
        run.outcome
    );
    assert_eq!(retries(&run.events), ["native_tools withheld=26"]);
    assert_eq!(
        run.declared,
        [false, true, true],
        "the retry, then every request after the confirmed swallow"
    );
    assert_eq!(run.reads, 1, "the call the server returned natively ran");
    assert!(
        run.events.iter().any(|event| matches!(event, Event::ToolCall { call_id, .. } if call_id.as_str().starts_with("fb-"))),
        "named like any text-protocol call"
    );
    // An empty reply that reports tokens is the same case, with nothing withheld on the wire.
    let empty = run_empty().await?;
    assert_eq!(retries(&empty.events), ["native_tools withheld=0"]);
    assert_eq!(empty.reads, 1);
    Ok(())
}
async fn run_empty() -> Result<Run, Box<dyn std::error::Error>> {
    run(vec![empty_with_tokens(), native_call(), done()], false).await
}

// TEST: CORE-U-065
#[tokio::test]
async fn core_u_065_a_real_answer_a_second_swallow_and_a_native_route_are_never_retried()
-> TestResult {
    // A genuine final answer holds nothing back: it ends the run as before.
    let answer = run(vec![done()], false).await?;
    assert!(retries(&answer.events).is_empty());
    assert_eq!(answer.declared, [false]);
    // At most once per step: a second swallowed reply in the step is taken as it is.
    let twice = run(vec![swallowed(5), swallowed(5)], false).await?;
    assert_eq!(retries(&twice.events), ["native_tools withheld=5"]);
    assert_eq!(twice.declared, [false, true]);
    assert!(matches!(twice.outcome, RunOutcome::Completed));
    // A native route never gets this retry.
    let native = run(vec![swallowed(5)], true).await?;
    assert!(retries(&native.events).is_empty());
    Ok(())
}

// TEST: CORE-U-065
#[tokio::test]
async fn core_u_065_a_server_that_refuses_declared_tools_leaves_the_route_text_only() -> TestResult
{
    let run = run(vec![swallowed(8), http(400), swallowed(8)], false).await?;
    assert_eq!(
        retries(&run.events),
        [
            "native_tools withheld=8",
            "native_tools refused (status 400)"
        ],
        "the refusal is named, then the step goes on as text"
    );
    assert_eq!(
        run.declared,
        [false, true, false],
        "never declared again in the run"
    );
    assert!(
        matches!(run.outcome, RunOutcome::Completed),
        "{:?}",
        run.outcome
    );
    Ok(())
}

/// A reply of `text`, then one bare keep-alive chunk (some proxies send them), then a normal stop.
fn with_keep_alive(text: &str) -> ProviderReply {
    reply(vec![
        StreamChunk::TextDelta(text.into()),
        StreamChunk::Withheld,
        StreamChunk::Finish(FinishReason::Stop),
        tokens(30),
    ])
}

// TEST: CORE-U-065
#[tokio::test]
async fn core_u_065_a_written_call_is_never_retried_and_a_keep_alive_after_an_answer_costs_one_call()
-> TestResult {
    // The fenced call is parsed before the check, so a keep-alive beside it changes nothing.
    let written = run(
        vec![
            with_keep_alive(
                "```tqa_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a\"}}\n```",
            ),
            done(),
        ],
        false,
    )
    .await?;
    assert!(retries(&written.events).is_empty());
    assert_eq!(written.reads, 1);
    // The known edge: a genuine answer followed by a keep-alive looks swallowed, and costs one extra model call.
    let answer = run(vec![with_keep_alive("All done."), done()], false).await?;
    assert_eq!(retries(&answer.events), ["native_tools withheld=1"]);
    assert_eq!(answer.declared, [false, true]);
    assert!(matches!(answer.outcome, RunOutcome::Completed));
    Ok(())
}

// TEST: CORE-U-065
#[tokio::test]
async fn core_u_065_on_the_declared_attempt_native_calls_win_over_written_ones() -> TestResult {
    // The model writes the same call both ways on the retry: it runs once, as the native call.
    let both = reply(vec![
        StreamChunk::TextDelta(
            "```tqa_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a\"}}\n```".into(),
        ),
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some("call_9".into()),
            name: Some("read_file".into()),
            arguments_delta: r#"{"path":"a"}"#.into(),
        },
        StreamChunk::Finish(FinishReason::ToolCalls),
        tokens(30),
    ]);
    let run = run(vec![swallowed(4), both, done()], false).await?;
    assert_eq!(run.reads, 1, "not twice");
    Ok(())
}

// TEST: CORE-U-065
#[tokio::test]
async fn core_u_065_a_confirmed_swallow_declares_the_tools_for_the_rest_of_the_run() -> TestResult {
    // The retry came back with a native call: every later request declares the tools, so one doubling per run.
    let run = run(
        vec![swallowed(6), native_call(), native_call(), done()],
        false,
    )
    .await?;
    assert_eq!(retries(&run.events), ["native_tools withheld=6"]);
    assert_eq!(run.declared, [false, true, true, true]);
    assert_eq!(run.reads, 2);
    Ok(())
}
