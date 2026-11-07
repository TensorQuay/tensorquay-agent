//! CORE-U-088 / CORE-U-089: after an unreadable text-protocol reply, the retry names what was wrong (ADR-0007). The
//! `provider.retry` keeps its stable reason and carries `fallback_problem`; the retry request's sentence comes from
//! the journal, so a resumed run sends the same request, and a journal without the field (an older gateway's) sends
//! today's sentence.
mod support;
use serde_json::{Value, json};
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{CompletionRequest, ContentPart, FinishReason, Role, StreamChunk};
use tqa_testkit::{FakeTool, ProviderReply};
use tqa_tools::{ToolError, ToolName, ToolSpec};
use tqa_types::{ErrorCode, Event, FallbackProblem, ServerFrame};

const NO_NAME: &str = "```tqa_call\n{\"arguments\": {\"path\": \"a.rs\"}}\n```\n";
const NO_NAME_SENTENCE: &str = "[The last reply could not be read as a tool call: the tqa_call block has no \"name\" \
    key. Each block holds one object with exactly two keys: \"name\", the tool, and \"arguments\". Reply again with \
    each call in its own tqa_call block.]";
const TODAYS_SENTENCE: &str = "[The last reply could not be read as a tool call: the tqa_call block is not valid \
    JSON. Reply again with each call in its own tqa_call block.]";

fn text(content: &str) -> ProviderReply {
    reply(vec![
        StreamChunk::TextDelta(content.into()),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ])
}
fn harness(script: Vec<ProviderReply>, tools: &[Arc<FakeTool>]) -> Result<Harness, Box<dyn Error>> {
    let mut h = Harness::new(script, registry(tools)?)?;
    h.deps.route.native_tools = false;
    h.deps.route.parallel_tools = false;
    Ok(h)
}
async fn run(h: Harness, history: &[ServerFrame]) -> Result<RunOutcome, Box<dyn Error>> {
    let message = history.is_empty().then(turn);
    Ok(Runner::new(h.deps)?
        .run(
            history,
            message,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}
fn texts(request: &CompletionRequest) -> Vec<(Role, String)> {
    request
        .messages
        .iter()
        .map(|message| {
            let text = message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            (message.role, text)
        })
        .collect()
}
fn last_user(request: &CompletionRequest) -> Option<String> {
    texts(request)
        .into_iter()
        .rev()
        .find(|(role, _)| *role == Role::User)
        .map(|(_, text)| text)
}
fn retries(events: &[Event]) -> Vec<(String, Option<FallbackProblem>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry {
                reason,
                fallback_problem,
                ..
            } => Some((reason.clone(), fallback_problem.clone())),
            _ => None,
        })
        .collect()
}
/// A `write_file` whose schema is the builtin's: `create_parents` required, a boolean.
fn write_file() -> Result<Arc<FakeTool>, ToolError> {
    Ok(Arc::new(FakeTool::new(
        ToolSpec {
            name: ToolName::new("write_file")?,
            description: "Test write_file".into(),
            parameters: json!({"type":"object","required":["path","content","create_parents"],
                "additionalProperties":false,"properties":{"path":{"type":"string"},"content":{"type":"string"},
                "create_parents":{"type":"boolean"}}}),
            parallel_safe: false,
            changes_workspace: true,
        },
        vec![output("written", 0)],
    )))
}

// TEST: CORE-U-088
#[tokio::test]
async fn core_u_088_a_block_without_a_name_is_retried_naming_it() -> TestResult {
    let tool = fake("read_file", false, vec![output("fn main() {}", 0)])?;
    let call = "```tqa_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n```\n";
    let h = harness(
        vec![text(NO_NAME), text(call), text("Done.")],
        std::slice::from_ref(&tool),
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h, &[]).await?, RunOutcome::Completed);
    assert_eq!(
        retries(&sink.events()),
        vec![(
            "fallback_parse: malformed".to_owned(),
            Some(FallbackProblem::NoName)
        )]
    );
    let requests = provider.requests();
    assert_eq!(last_user(&requests[1]).as_deref(), Some(NO_NAME_SENTENCE));
    assert_eq!(tool.calls(), vec![json!({"path": "a.rs"})]);
    Ok(())
}

/// The journal of a run cut right after its fallback retry, and the full run's retry request.
async fn cut_after_retry() -> Result<(Vec<ServerFrame>, CompletionRequest), Box<dyn Error>> {
    let full = harness(vec![text(NO_NAME), text("Done.")], &[])?;
    let (provider, sink) = (full.provider.clone(), full.sink.clone());
    assert_eq!(run(full, &[]).await?, RunOutcome::Completed);
    let retry_request = provider
        .requests()
        .get(1)
        .cloned()
        .ok_or("no retry request")?;
    let at = sink
        .types()
        .iter()
        .position(|kind| kind == "provider.retry")
        .ok_or("no provider.retry")?;
    let shutdown = CancellationToken::new();
    let mut cut = harness(vec![text(NO_NAME)], &[])?;
    let mut memory = MemorySink::new(&[]);
    memory.cut = Some((at + 1, shutdown.clone()));
    cut.sink_with(memory);
    let frames_of = cut.sink.clone();
    assert_eq!(
        Runner::new(cut.deps)?
            .run(&[], Some(turn()), CancellationToken::new(), shutdown)
            .await?,
        RunOutcome::Interrupted
    );
    let frames = frames_of.frames();
    assert_eq!(
        frames_of.types().last().map(String::as_str),
        Some("provider.retry")
    );
    Ok((frames, retry_request))
}

async fn resumed_request(frames: &[ServerFrame]) -> Result<CompletionRequest, Box<dyn Error>> {
    let mut resumed = harness(vec![text("Done.")], &[])?;
    resumed.sink_with(MemorySink::new(frames));
    let provider = resumed.provider.clone();
    assert_eq!(run(resumed, frames).await?, RunOutcome::Completed);
    provider
        .requests()
        .first()
        .cloned()
        .ok_or_else(|| "no request".into())
}

// TEST: CORE-U-088
#[tokio::test]
async fn core_u_088_a_resumed_retry_sends_the_same_request() -> TestResult {
    let (frames, retry_request) = cut_after_retry().await?;
    let resumed = resumed_request(&frames).await?;
    assert_eq!(texts(&resumed), texts(&retry_request));
    assert_eq!(last_user(&resumed).as_deref(), Some(NO_NAME_SENTENCE));
    Ok(())
}

// TEST: CORE-U-088
#[tokio::test]
async fn core_u_088_a_journal_without_the_problem_resumes_with_todays_sentence() -> TestResult {
    let (frames, _) = cut_after_retry().await?;
    // As an older gateway writes the journal back: the field it doesn't know is dropped.
    let mut written: Value = serde_json::to_value(&frames)?;
    let mut dropped = 0;
    for frame in written.as_array_mut().ok_or("frames")? {
        if let Some(event) = frame.get_mut("event").and_then(Value::as_object_mut)
            && event.remove("fallback_problem").is_some()
        {
            dropped += 1;
        }
    }
    assert_eq!(dropped, 1);
    let older: Vec<ServerFrame> = serde_json::from_value(written)?;
    let resumed = resumed_request(&older).await?;
    assert_eq!(last_user(&resumed).as_deref(), Some(TODAYS_SENTENCE));
    Ok(())
}

// TEST: CORE-U-089
#[tokio::test]
async fn core_u_089_a_write_file_without_create_parents_names_both() -> TestResult {
    let tool = write_file()?;
    let without = "```tqa_call\n{\"name\": \"write_file\", \"arguments\": {\"path\": \"a/b.py\", \"content\": \"x = 1\"}}\n```\n";
    let with = "```tqa_call\n{\"name\": \"write_file\", \"arguments\": {\"path\": \"a/b.py\", \"content\": \"x = 1\", \"create_parents\": true}}\n```\n";
    let h = harness(
        vec![text(without), text(with), text("Done.")],
        std::slice::from_ref(&tool),
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h, &[]).await?, RunOutcome::Completed);
    let retried = retries(&sink.events());
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].0, "fallback_parse: schema");
    assert!(
        matches!(&retried[0].1, Some(FallbackProblem::MissingProperty { tool, property, .. })
            if tool.as_str() == "write_file" && property.as_str() == "create_parents"),
        "{retried:?}"
    );
    assert_eq!(
        last_user(&provider.requests()[1]).as_deref(),
        Some(
            "[The last reply could not be read as a tool call: write_file needs \"create_parents\" (a boolean). Add \
             it to the arguments. Reply again with each call in its own tqa_call block.]"
        )
    );
    // Never the call's content.
    for event in sink.events() {
        if matches!(event, Event::ProviderRetry { .. }) {
            let written = serde_json::to_string(&event)?;
            assert!(
                !written.contains("x = 1") && !written.contains("a/b.py"),
                "{written}"
            );
        }
    }
    Ok(())
}

// TEST: CORE-U-089
#[tokio::test]
async fn core_u_089_a_detailed_sentence_copied_back_is_an_echo() -> TestResult {
    let h = harness(
        vec![text(NO_NAME), text(NO_NAME_SENTENCE), text("Done.")],
        &[],
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(
        run(h, &[]).await?,
        RunOutcome::Failed {
            code: ErrorCode::FallbackParse,
            reason: "fallback_parse: echo".into()
        }
    );
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(retries(&sink.events()).len(), 1);
    // Nothing of the copied sentence was shown as the agent's words.
    assert!(!sink.events().iter().any(|event| matches!(event,
        Event::TextDelta { text } if text.contains("could not be read"))));
    Ok(())
}
