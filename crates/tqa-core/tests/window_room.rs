//! CORE-U-073: a run makes room in its window rather than failing "context window exceeded" (candidate 12: with Max
//! output 16384 on a guessed 32k window, a task failed after two small steps). Past reasoning isn't sent, so it isn't
//! counted; a set Max output is a cap, so the request asks for what the window leaves, down to the documented reserve;
//! and when that still doesn't fit, compaction folds older turns even when fewer than its usual six are there.
mod support;
use std::error::Error;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{DEFAULT_OUTPUT_RESERVE, RunOutcome, Runner, RunnerDeps};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_tools::ToolRegistry;
use tqa_types::{Event, ServerFrame, ToolStatus};

async fn run(deps: RunnerDeps, history: &[ServerFrame]) -> Result<RunOutcome, Box<dyn Error>> {
    Ok(Runner::new(deps)?
        .run(
            history,
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}

/// The candidate 12 route: Max output 16384, no context window set (so 32768, guessed).
fn route_16k(h: &mut Harness) {
    h.deps.route.context_window = None;
    h.deps.route.max_output = Some(16_384);
    h.deps.limits.max_output_tokens = 1_000_000;
}

/// A step that thinks at length (about 11k estimated tokens) before its call.
fn thinking_call(id: &str) -> tqa_testkit::ProviderReply {
    reply(vec![
        StreamChunk::ReasoningDelta(
            "I should write the file with the exact content asked for. ".repeat(700),
        ),
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some(id.into()),
            name: Some("write_file".into()),
            arguments_delta: r#"{"path":"notes.md"}"#.into(),
        },
        usage(),
        StreamChunk::Finish(FinishReason::ToolCalls),
    ])
}

/// Earlier completed runs, each one user message and one step whose result is `result_bytes` long.
fn earlier(runs: usize, result_bytes: usize) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let mut events = Vec::new();
    for index in 1..=runs {
        let run_id: tqa_types::RunId = format!("r-{index}").parse()?;
        let call_id: tqa_types::ToolCallId = format!("c{index}").parse()?;
        events.extend([
            Event::MessageUser {
                lead_brief: None,
                team_wake: None,
                text: format!("request {index}"),
                attachments: vec![],
            },
            Event::RunStarted {
                run_id: run_id.clone(),
            },
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
            Event::ToolCall {
                call_id: call_id.clone(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": format!("src/{index}.rs")}),
            },
            Event::ModelCompleted {
                step: 1,
                attempt: 1,
                finish_reason: tqa_types::FinishReason::ToolCalls,
            },
            Event::ToolResult {
                call_id,
                status: ToolStatus::Ok,
                content: format!("{index:04}{}", "x".repeat(result_bytes - 4)),
                target: None,
                evidence: None,
            },
            Event::StepCompleted {
                step: 1,
                duration_ms: 0,
            },
            Event::RunCompleted { run_id },
        ]);
    }
    history(events)
}

// TEST: CORE-U-073
#[tokio::test]
async fn core_u_073_past_reasoning_is_neither_sent_nor_counted() -> TestResult {
    let tool = fake(
        "write_file",
        true,
        vec![output("written", 0), output("written", 0)],
    )?;
    let mut h = Harness::new(
        vec![thinking_call("w1"), thinking_call("w2"), answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    route_16k(&mut h);
    assert_eq!(run(h.deps, &[]).await?, RunOutcome::Completed);
    let requests = h.provider.requests();
    assert_eq!(requests.len(), 3);
    let last = format!("{:?}", requests[2].messages);
    assert!(
        !last.contains("exact content asked for"),
        "no past reasoning is sent"
    );
    Ok(())
}

// TEST: CORE-U-073
#[tokio::test]
async fn core_u_073_a_set_max_output_asks_for_what_the_window_leaves() -> TestResult {
    // Two earlier runs of about 5.7k tokens each: too few turns to compact, more than 16384 leaves of a guessed window.
    let history = earlier(2, 20_000)?;
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    route_16k(&mut h);
    h.sink_with(MemorySink::new(&history));
    assert_eq!(run(h.deps, &history).await?, RunOutcome::Completed);
    let asked = h.provider.requests()[0].max_output.ok_or("no limit")?;
    assert!((DEFAULT_OUTPUT_RESERVE..16_384).contains(&asked), "{asked}");
    Ok(())
}

// TEST: CORE-U-073
#[tokio::test]
async fn core_u_073_with_too_few_turns_to_fold_as_usual_compaction_keeps_fewer() -> TestResult {
    // Two earlier runs of about 11.4k tokens each: even with the output narrowed to 4096 they don't fit a guessed 32k
    // window (24576), and there are fewer than six turns, so compaction folds the older run instead of the run failing.
    let history = earlier(2, 40_000)?;
    let mut h = Harness::new(
        vec![
            reply(vec![
                StreamChunk::TextDelta(r#"{"goal":"answer","done":["read src/1.rs"]}"#.into()),
                usage(),
                StreamChunk::Finish(FinishReason::Stop),
            ]),
            answer(),
        ],
        ToolRegistry::new(),
    )?;
    route_16k(&mut h);
    h.sink_with(MemorySink::new(&history));
    let sink = h.sink.clone();
    assert_eq!(run(h.deps, &history).await?, RunOutcome::Completed);
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, Event::Compaction { .. })),
        "compacted"
    );
    Ok(())
}

/// One earlier answered run whose step carried an opaque reasoning state of about `bytes` for `route` and `model`.
fn with_state(route: &str, model: &str, bytes: usize) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let run_id: tqa_types::RunId = "r-1".parse()?;
    let item = serde_json::json!({"type": "reasoning", "encrypted_content": "e".repeat(bytes)})
        .to_string();
    history(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "request 1".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: run_id.clone(),
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
        Event::ReasoningState {
            protocol: tqa_types::WireProtocol::OpenAiChat,
            route: route.into(),
            model: model.into(),
            bytes: u32::try_from(item.len())?,
            item: Some(tqa_types::ReasoningItem::new(item)?),
        },
        Event::TextDelta {
            text: "answer 1".into(),
        },
        Event::ModelCompleted {
            step: 1,
            attempt: 1,
            finish_reason: tqa_types::FinishReason::Stop,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::RunCompleted { run_id },
    ])
}

// TEST: CORE-U-074
#[tokio::test]
async fn core_u_074_a_replayed_reasoning_state_takes_room_in_the_window() -> TestResult {
    // A state of about 20k tokens for this very route and model is sent back, so it counts: on a 32k window the
    // request's 16384 narrows to what's left. One for another route or model is neither sent nor counted.
    for (route, model, narrowed) in [
        ("test", "fake-model", true),
        ("other", "fake-model", false),
        ("test", "older-model", false),
    ] {
        let history = with_state(route, model, 70_000)?;
        let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
        h.deps.route.context_window = Some(32_768);
        h.deps.route.max_output = Some(16_384);
        h.deps.limits.max_output_tokens = 1_000_000;
        h.sink_with(MemorySink::new(&history));
        assert_eq!(
            run(h.deps, &history).await?,
            RunOutcome::Completed,
            "{route} {model}"
        );
        let request = &h.provider.requests()[0];
        let asked = request.max_output.ok_or("no limit")?;
        assert_eq!(asked < 16_384, narrowed, "{route} {model}: {asked}");
        let sent = format!("{:?}", request.messages).contains("encrypted_content");
        assert_eq!(sent, narrowed, "{route} {model}: sent only when counted");
    }
    Ok(())
}
