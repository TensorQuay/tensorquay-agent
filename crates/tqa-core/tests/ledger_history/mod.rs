//! The ledger histories, journals and harness the compaction-trigger suites share (CORE-U-084): completed runs whose
//! tool results are dense ledger lines, usage frames as the runner journals them, and the next turn run on them.
#![allow(dead_code, reason = "Each compaction suite uses the helpers it needs.")]
use crate::support::*;
use serde_json::json;
use std::{error::Error, fmt::Write};
use tokio_util::sync::CancellationToken;
use tqa_context::estimate;
use tqa_core::{RunOutcome, Runner, UserTurn};
use tqa_llm::{CompletionRequest, ContentPart, FinishReason, StreamChunk};
use tqa_testkit::ProviderReply;
use tqa_tools::ToolRegistry;
use tqa_types::{
    CompactionSummary, Event, FinishReason as WireFinish, ReasoningItem, ServerFrame, ToolStatus,
    WireProtocol,
};

/// About 300 estimated tokens: ceil(1050 * 2 / 7).
pub const RESULT_BYTES: usize = 1050;

/// Window and output reserve as in CORE-U-025: an input capacity of 7168, compaction above 85 % of it.
pub const WINDOW: u32 = 8192;

pub const RESERVE: u32 = 1024;

pub const THRESHOLD: u64 = (WINDOW - RESERVE) as u64 * 85 / 100;

/// Completed runs of history: their estimate stays under the threshold.
pub const RUNS: usize = 16;

/// A tool result of `RESULT_BYTES` bytes, distinct per run (identical results would be deduplicated, CTX-U-012).
pub fn ledger(index: usize) -> Result<String, std::fmt::Error> {
    let mut text = String::new();
    let mut line = 0;
    while text.len() < RESULT_BYTES {
        writeln!(
            text,
            "entry {index:02}-{line:03} account A-{:04} amount {:5}",
            (index * 7919 + line * 104_729) % 10_000,
            (index * 31 + line * 977) % 10_000
        )?;
        line += 1;
    }
    text.truncate(RESULT_BYTES);
    Ok(text)
}

/// The same size in CJK text: three bytes a character, where a model counts about a token a character.
pub fn cjk(index: usize) -> Result<String, std::fmt::Error> {
    let mut text = format!("{index:03}");
    for (at, character) in "账目核对第项金额备注工资差旅费用"
        .chars()
        .cycle()
        .enumerate()
    {
        if text.len() + character.len_utf8() > RESULT_BYTES {
            break;
        }
        // Distinct per run without changing the size much: the run's number now and then.
        if at % 40 == 39 {
            write!(text, "{:03}", index % 1000)?;
        } else {
            text.push(character);
        }
    }
    Ok(text)
}

/// One completed run: the user message, a tool step whose result is `content`, and an answer step.
pub fn completed_run(
    index: usize,
    seq: usize,
    content: String,
) -> Result<Vec<Event>, Box<dyn Error>> {
    let run_id: tqa_types::RunId = format!("r-{seq}").parse()?;
    let call_id: tqa_types::ToolCallId = format!("c{index}").parse()?;
    Ok(vec![
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
            arguments: json!({"path": format!("ledger/part-{index:02}.txt")}),
        },
        Event::ModelCompleted {
            step: 1,
            attempt: 1,
            finish_reason: WireFinish::ToolCalls,
        },
        Event::ToolResult {
            call_id,
            status: ToolStatus::Ok,
            content,
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::StepStarted {
            step: 2,
            attempt: 1,
        },
        Event::TextDelta {
            text: format!("answer {index}"),
        },
        Event::ModelCompleted {
            step: 2,
            attempt: 1,
            finish_reason: WireFinish::Stop,
        },
        Event::StepCompleted {
            step: 2,
            duration_ms: 0,
        },
        Event::RunCompleted { run_id },
    ])
}

pub fn long_history(
    content: fn(usize) -> Result<String, std::fmt::Error>,
) -> Result<Vec<Event>, Box<dyn Error>> {
    long_history_of(content, RUNS)
}

pub fn long_history_of(
    content: fn(usize) -> Result<String, std::fmt::Error>,
    runs: usize,
) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut events = Vec::new();
    for index in 1..=runs {
        let seq = events.len() + 1;
        events.extend(completed_run(index, seq, content(index)?)?);
    }
    Ok(events)
}

/// The last call's usage as an older runner journals it (no estimate of its request): after its reply, before
/// `model.completed`.
pub fn older_usage(prompt_tokens: u64, completion_tokens: u64) -> Event {
    Event::Usage {
        prompt_tokens,
        completion_tokens,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: false,
        estimated_input: None,
        estimated_images: None,
        estimated_route: None,
    }
}

/// A step call's usage as the runner journals it since c16: with its own estimate of the request it sent.
pub fn recorded_usage(prompt_tokens: u64, completion_tokens: u64, estimated_input: u64) -> Event {
    Event::Usage {
        prompt_tokens,
        completion_tokens,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: false,
        estimated_input: Some(estimated_input),
        estimated_images: Some(0),
        estimated_route: Some("test".into()),
    }
}

pub fn with_last_usage(mut events: Vec<Event>, usage: Event) -> Result<Vec<Event>, Box<dyn Error>> {
    let last_completed = events
        .iter()
        .rposition(|event| matches!(event, Event::ModelCompleted { .. }))
        .ok_or("no model call")?;
    events.insert(last_completed, usage);
    Ok(events)
}

pub fn harness(script: Vec<ProviderReply>) -> Result<Harness, Box<dyn Error>> {
    let mut h = Harness::new(script, ToolRegistry::new())?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.max_output_tokens = 1_000_000;
    Ok(h)
}

pub fn model_summary() -> CompactionSummary {
    CompactionSummary {
        goal: "audit the ledger".into(),
        done: vec!["read ledger/part-01.txt".into()],
        key_paths: vec!["ledger/part-01.txt".into()],
        ..CompactionSummary::default()
    }
}

pub fn summary_reply() -> Result<ProviderReply, Box<dyn Error>> {
    Ok(reply(vec![
        StreamChunk::TextDelta(serde_json::to_string(&model_summary())?),
        crate::support::usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ]))
}

pub fn compactions(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Event::Compaction { .. }))
        .count()
}

/// The window's accounting of a request: text, call identifiers, names and arguments, plus 4 per message.
pub fn request_tokens(request: &tqa_llm::CompletionRequest) -> u64 {
    request
        .messages
        .iter()
        .map(|message| {
            let text: String = message
                .content
                .iter()
                .map(|part| match part {
                    ContentPart::Text(text) => text.as_str(),
                    _ => "",
                })
                .collect();
            let calls: String = message
                .tool_calls
                .iter()
                .flat_map(|call| [call.id.as_str(), &call.name, &call.arguments])
                .collect();
            let answered = message.tool_call_id.clone().unwrap_or_default();
            u64::from(estimate(&format!("{text}{calls}{answered}")).tokens) + 4
        })
        .sum()
}

/// Runs the next turn on `events` with a summary reply and an answer scripted; the compactions it wrote, and the
/// estimate of its first request.
pub async fn next_turn(events: Vec<Event>) -> Result<(usize, u64), Box<dyn Error>> {
    let (compacted, requests) = next_turn_with(events, |_| Ok(()), &[]).await?;
    Ok((
        compacted,
        requests.first().map(request_tokens).ok_or("no request")?,
    ))
}

/// [`next_turn`] with the harness adjusted by `setup` and the turn attaching `attachments`; every request it sent.
pub async fn next_turn_with(
    events: Vec<Event>,
    setup: impl FnOnce(&mut Harness) -> Result<(), Box<dyn Error>>,
    attachments: &[&str],
) -> Result<(usize, Vec<tqa_llm::CompletionRequest>), Box<dyn Error>> {
    let (written, requests) = next_turn_journal(events, setup, attachments).await?;
    Ok((compactions(&written), requests))
}
/// [`next_turn_with`], returning the events the turn wrote instead of its compaction count.
pub async fn next_turn_journal(
    events: Vec<Event>,
    setup: impl FnOnce(&mut Harness) -> Result<(), Box<dyn Error>>,
    attachments: &[&str],
) -> Result<(Vec<Event>, Vec<tqa_llm::CompletionRequest>), Box<dyn Error>> {
    let history: Vec<ServerFrame> = history(events)?;
    let mut h = harness(vec![summary_reply()?, answer()])?;
    setup(&mut h)?;
    h.sink_with(MemorySink::new(&history));
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "the current request".into(),
        attachments: attachments.iter().map(|path| (*path).to_owned()).collect(),
    };
    let outcome = Runner::new(h.deps)?
        .run(
            &history,
            Some(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    let all = sink.events();
    let written = all[all.len().min(history.len())..].to_vec();
    Ok((written, provider.requests()))
}

/// Each tool call preceded by a replayable reasoning state of `padding` bytes, as the harness's route on the Responses
/// protocol records it (ADR-0027).
pub fn with_states(events: Vec<Event>, padding: usize) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut out = Vec::new();
    for (index, event) in events.into_iter().enumerate() {
        if matches!(event, Event::ToolCall { .. }) {
            let item = json!({"type": "reasoning", "id": format!("rs_{index}"),
                              "encrypted_content": "x".repeat(padding)})
            .to_string();
            out.push(Event::ReasoningState {
                protocol: WireProtocol::OpenAiResponses,
                route: "test".into(),
                model: "fake-model".into(),
                bytes: u32::try_from(item.len())?,
                item: Some(ReasoningItem::new(item)?),
            });
        }
        out.push(event);
    }
    Ok(out)
}

/// How many replayed states a request carries.
pub fn states_sent(request: &tqa_llm::CompletionRequest) -> usize {
    request
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter(|part| matches!(part, ContentPart::ReasoningState(_)))
        .count()
}

/// A vision route with `uploads/a.png` in the workspace.
pub fn vision(h: &mut Harness) -> Result<(), Box<dyn Error>> {
    h.deps.route.vision = true;
    let uploads = h.directory.path().join("uploads");
    std::fs::create_dir_all(&uploads)?;
    std::fs::write(uploads.join("a.png"), b"\x89PNG\r\n\x1a\nimage-bytes")?;
    Ok(())
}

/// `runs` runs of ledger history whose last user message attached the image.
pub fn with_image(runs: usize) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut events = long_history_of(ledger, runs)?;
    let last_user = events
        .iter()
        .rposition(|event| matches!(event, Event::MessageUser { .. }))
        .ok_or("no user message")?;
    if let Event::MessageUser { attachments, .. } = &mut events[last_user] {
        attachments.push("uploads/a.png".into());
    }
    Ok(events)
}

/// A request's size as the window counts it: its messages, its replayed states (their bytes at the estimate's rate)
/// and its tools.
pub fn full_tokens(request: &CompletionRequest) -> u64 {
    let states: usize = request
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .map(|part| match part {
            ContentPart::ReasoningState(item) => item.len(),
            _ => 0,
        })
        .sum();
    // The tools travel beside the messages, and a server counts them as input as well.
    let mut tools = String::new();
    for tool in &request.tools {
        tools.push_str(&tool.name);
        tools.push_str(&tool.description);
        tools.push_str(&tool.parameters.to_string());
    }
    request_tokens(request) + (states as u64 * 2).div_ceil(7) + u64::from(estimate(&tools).tokens)
}

/// The latest assistant message's replayed states, at the estimate's rate.
pub fn latest_states(request: &CompletionRequest) -> u64 {
    let bytes: usize = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == tqa_llm::Role::Assistant)
        .map_or(0, |message| {
            message
                .content
                .iter()
                .map(|part| match part {
                    ContentPart::ReasoningState(item) => item.len(),
                    _ => 0,
                })
                .sum()
        });
    (bytes as u64 * 2).div_ceil(7)
}
