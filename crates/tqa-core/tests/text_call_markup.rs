//! CORE-U-070: on a native-tools route, a call the model wrote as text (CORE-U-068) runs, and its markup never reaches
//! the transcript: only the narration is streamed and journaled, as for the text protocol. What could begin such a
//! call waits for the whole reply (PROMPT-U-014); anything that turns out not to be a call is shown then, in full.
mod support;
use serde_json::json;
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_testkit::{FakeTool, ProviderReply};
use tqa_types::Event;

/// A reply streamed in small pieces, finished as `finish` says.
fn streamed(content: &str, finish: FinishReason) -> ProviderReply {
    let mut chunks: Vec<StreamChunk> = content
        .as_bytes()
        .chunks(9)
        .map(|piece| StreamChunk::TextDelta(String::from_utf8_lossy(piece).into_owned()))
        .collect();
    chunks.extend([usage(), StreamChunk::Finish(finish)]);
    reply(chunks)
}

const NARRATION: &str = "I'll write it to notes.md.\n\n";
const MARKUP: &str = "<function-call>\n{\n  \"name\": \"write_file\",\n  \"arguments\": {\n    \"path\": \
                      \"notes.md\"\n  }\n}\n</function-call>";

fn harness(script: Vec<ProviderReply>, tool: &Arc<FakeTool>) -> Result<Harness, Box<dyn Error>> {
    let h = Harness::new(script, registry(std::slice::from_ref(tool))?)?;
    assert!(h.deps.route.native_tools, "a native-tools route");
    Ok(h)
}

async fn run(h: Harness) -> Result<RunOutcome, Box<dyn Error>> {
    Ok(Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}

/// The first step's text deltas: what the transcript received before any call.
fn first_step_text(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .take_while(|event| !matches!(event, Event::ToolCall { .. } | Event::StepCompleted { .. }))
        .filter_map(|event| match event {
            Event::TextDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn retried(events: &[Event]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, Event::ProviderRetry { .. }))
}

// TEST: CORE-U-070
#[tokio::test]
async fn core_u_070_a_recognised_text_call_shows_only_its_narration() -> TestResult {
    let tool = fake("write_file", true, vec![output("written", 0)])?;
    let reply = streamed(&format!("{NARRATION}{MARKUP}"), FinishReason::Stop);
    let h = harness(vec![reply, answer()], &tool)?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "notes.md"})]);
    let events = sink.events();
    assert_eq!(first_step_text(&events).concat(), NARRATION);
    let all: String = events
        .iter()
        .filter_map(|event| match event {
            Event::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !all.contains("function-call") && !all.contains("\"name\""),
        "{all}"
    );
    assert!(!retried(&events));
    Ok(())
}

// TEST: CORE-U-070
#[tokio::test]
async fn core_u_070_a_reply_that_is_only_a_call_object_shows_nothing() -> TestResult {
    let tool = fake("write_file", true, vec![output("written", 0)])?;
    let object = "{\"function\": \"write_file\", \"arguments\": {\"path\": \"notes.md\"}}";
    let h = harness(vec![streamed(object, FinishReason::Stop), answer()], &tool)?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "notes.md"})]);
    assert_eq!(first_step_text(&sink.events()).concat(), "");
    Ok(())
}

// TEST: CORE-U-070
#[tokio::test]
async fn core_u_070_a_long_json_answer_that_isnt_a_call_shows_in_full_at_the_end() -> TestResult {
    // A person asked for JSON output: several KB, one object, naming no tool. Held while it arrives, since it could
    // have been a call, then shown whole, with no retry and no failure.
    let tool = fake("write_file", true, vec![])?;
    let items: Vec<_> = (0..120)
        .map(|index| json!({"id": index, "name": format!("item {index}"), "note": "kept as asked"}))
        .collect();
    let answer = serde_json::to_string_pretty(&json!({"items": items}))?;
    assert!(answer.len() > 6_000 && answer.starts_with('{'));
    let h = harness(vec![streamed(&answer, FinishReason::Stop)], &tool)?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert!(tool.calls().is_empty(), "nothing ran");
    let events = sink.events();
    let shown = first_step_text(&events);
    assert_eq!(shown.concat(), answer);
    assert_eq!(shown.len(), 1, "held until the end, then shown at once");
    assert!(!retried(&events));
    Ok(())
}

// TEST: CORE-U-070
#[tokio::test]
async fn core_u_070_prose_and_code_still_stream_as_they_come() -> TestResult {
    let tool = fake("write_file", true, vec![])?;
    let answer = "If a < b and b < c, then a < c.\n```python\nassert a < c\n```\nThat's all.";
    let h = harness(vec![streamed(answer, FinishReason::Stop)], &tool)?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let shown = first_step_text(&sink.events());
    assert_eq!(shown.concat(), answer);
    assert!(shown.len() > 5, "streamed piece by piece: {shown:?}");
    Ok(())
}

// TEST: CORE-U-070
#[tokio::test]
async fn core_u_070_a_reply_cut_at_the_limit_shows_as_it_came() -> TestResult {
    // Cut mid-call: it isn't read as a call, so what was held is shown as it arrived.
    let tool = fake("write_file", true, vec![])?;
    let cut = format!("{NARRATION}<function-call>\n{{\n  \"name\": \"write_");
    let h = harness(vec![streamed(&cut, FinishReason::Length)], &tool)?;
    let sink = h.sink.clone();
    run(h).await?;
    assert!(tool.calls().is_empty(), "nothing ran");
    assert_eq!(first_step_text(&sink.events()).concat(), cut);
    Ok(())
}

// TEST: CORE-U-075
#[tokio::test]
async fn core_u_075_a_trailing_json_fenced_call_runs_and_its_fence_never_shows() -> TestResult {
    // F25 (candidate 12): a local model wrote its call in a json fence at the end of its reply. It runs; only the
    // narration reaches the transcript, and the run doesn't retry.
    let tool = fake("write_file", true, vec![output("written", 0)])?;
    let narration = "I'll save the note.\n";
    let reply = streamed(
        &format!(
            "{narration}```json\n{{\"name\":\"write_file\",\"arguments\":{{\"path\":\"notes.md\"}}}}\n```"
        ),
        FinishReason::Stop,
    );
    let h = harness(vec![reply, answer()], &tool)?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "notes.md"})]);
    let events = sink.events();
    assert_eq!(first_step_text(&events).concat(), narration);
    assert!(!retried(&events));
    Ok(())
}

// TEST: CORE-U-075
#[tokio::test]
async fn core_u_075_a_json_answer_about_tools_stays_the_answer() -> TestResult {
    let tool = fake("write_file", true, vec![])?;
    let answer = "```json\n{\"tools\":[\"write_file\"]}\n```";
    let h = harness(vec![streamed(answer, FinishReason::Stop)], &tool)?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert!(tool.calls().is_empty(), "nothing ran");
    let events = sink.events();
    assert_eq!(first_step_text(&events).concat(), answer);
    assert!(!retried(&events));
    Ok(())
}

const TEXT_CALL_CORRECTION: &str = "[The last reply wrote a tool call as text, so nothing ran. Call the tool through the \
                                    tool interface instead of writing it in the reply.]";

/// Text in small pieces, then one native call.
fn text_then_native_call(content: &str, (id, name, args): (&str, &str, &str)) -> ProviderReply {
    let mut chunks: Vec<StreamChunk> = content
        .as_bytes()
        .chunks(9)
        .map(|piece| StreamChunk::TextDelta(String::from_utf8_lossy(piece).into_owned()))
        .collect();
    chunks.extend([
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some(id.into()),
            name: Some(name.into()),
            arguments_delta: args.into(),
        },
        usage(),
        StreamChunk::Finish(FinishReason::ToolCalls),
    ]);
    reply(chunks)
}

// TEST: CORE-U-087
#[tokio::test]
async fn core_u_087_a_correction_copied_before_a_native_call_is_dropped() -> TestResult {
    let tool = fake("write_file", true, vec![output("written", 0)])?;
    let reply = text_then_native_call(
        &format!("{TEXT_CALL_CORRECTION}\n\nWriting it.\n"),
        ("call_1", "write_file", r#"{"path":"notes.md"}"#),
    );
    let h = harness(vec![reply, answer()], &tool)?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "notes.md"})]);
    let events = sink.events();
    assert_eq!(first_step_text(&events).concat(), "Writing it.\n");
    assert!(!retried(&events));
    // Never journalled either: the next request's history holds no trace of it.
    let next = request_value(&provider.requests()[1]).to_string();
    assert!(!next.contains("[The last reply"), "{next}");
    assert!(next.contains("Writing it."), "{next}");
    Ok(())
}

// TEST: CORE-U-087
#[tokio::test]
async fn core_u_087_a_correction_copied_before_a_text_call_is_dropped() -> TestResult {
    let tool = fake("write_file", true, vec![output("written", 0)])?;
    let reply = streamed(
        &format!("{TEXT_CALL_CORRECTION}\n{NARRATION}{MARKUP}"),
        FinishReason::Stop,
    );
    let h = harness(vec![reply, answer()], &tool)?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "notes.md"})]);
    let events = sink.events();
    assert_eq!(first_step_text(&events).concat(), NARRATION);
    assert!(!retried(&events));
    let next = request_value(&provider.requests()[1]).to_string();
    assert!(!next.contains("[The last reply"), "{next}");
    Ok(())
}
