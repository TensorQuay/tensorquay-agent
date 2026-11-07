//! CORE-U-068: on a native-tools route, a reply that delivers no native call but writes one as text (qwen3:4b on
//! Ollama, 30 Sep) never ends the run as if the call had happened. A readable call to a known tool runs like a native
//! one; one that can't be read is asked again once with a correction for this route (`fallback_parse: text_call`), and a
//! second fails the run with `fallback_parse`; a plain answer completes as before.
mod support;
use serde_json::json;
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{ContentPart, FinishReason, StreamChunk};
use tqa_testkit::{FakeTool, ProviderReply};
use tqa_types::{ErrorCode, Event};

/// A text reply streamed in small pieces, finished as the model finished it.
fn text(content: &str) -> ProviderReply {
    let mut chunks: Vec<StreamChunk> = content
        .as_bytes()
        .chunks(9)
        .map(|piece| StreamChunk::TextDelta(String::from_utf8_lossy(piece).into_owned()))
        .collect();
    chunks.extend([usage(), StreamChunk::Finish(FinishReason::Stop)]);
    reply(chunks)
}

const WRITTEN: &str = "I'll write it to notes.md.\n\n<function-call>\n{\n  \"name\": \"write_file\",\n  \
                       \"arguments\": {\n    \"path\": \"notes.md\"\n  }\n}\n</function-call>";
const CUT: &str = "I'll write it to notes.md.\n\n<function-call>\n{\n  \"name\": \"write_file\",\n  \
                   \"arguments\": {\n    \"path\": \n}\n</function-call>";

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

fn retries(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

// TEST: CORE-U-068
#[tokio::test]
async fn core_u_068_a_call_written_as_text_runs_on_a_native_route() -> TestResult {
    let tool = fake("write_file", true, vec![output("written", 0)])?;
    let h = harness(vec![text(WRITTEN), answer()], &tool)?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "notes.md"})]);
    let events = sink.events();
    assert!(events.iter().any(|event| matches!(event,
        Event::ToolCall { call_id, name, .. } if name == "write_file" && call_id.as_str() == "fb-1-1-1")));
    assert_eq!(retries(&events), Vec::<String>::new());
    // The next request carries the call and its result, as for a native call.
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let history = format!("{:?}", requests[1].messages);
    assert!(history.contains("written"), "{history}");
    Ok(())
}

// TEST: CORE-U-068
#[tokio::test]
async fn core_u_068_an_unreadable_text_call_is_asked_again_then_fails_plainly() -> TestResult {
    let tool = fake("write_file", true, vec![])?;
    let h = harness(vec![text(CUT), text(CUT)], &tool)?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert!(matches!(
        run(h).await?,
        RunOutcome::Failed { code: ErrorCode::FallbackParse, ref reason } if reason == "fallback_parse: text_call"
    ));
    assert!(tool.calls().is_empty(), "nothing ran");
    let events = sink.events();
    assert_eq!(retries(&events), ["fallback_parse: text_call"]);
    // The one retry says what went wrong for this route: call the tool, don't write it.
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let last = requests[1]
        .messages
        .last()
        .ok_or("a correction")?
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(
        last.contains("wrote a tool call as text") && last.contains("tool interface"),
        "{last}"
    );
    assert!(!last.contains("tqa_call"), "{last}");
    Ok(())
}

// TEST: CORE-U-068
#[tokio::test]
async fn core_u_068_after_the_correction_a_native_call_runs() -> TestResult {
    let tool = fake("write_file", true, vec![output("written", 0)])?;
    let h = harness(
        vec![
            text(CUT),
            calls(&[("call-1", "write_file", r#"{"path":"notes.md"}"#)]),
            answer(),
        ],
        &tool,
    )?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "notes.md"})]);
    assert_eq!(retries(&sink.events()), ["fallback_parse: text_call"]);
    Ok(())
}

// TEST: CORE-U-068
#[tokio::test]
async fn core_u_068_a_plain_answer_completes_without_a_retry() -> TestResult {
    let tool = fake("write_file", true, vec![])?;
    let h = harness(
        vec![text("Photosynthesis turns light into sugar in the leaves.")],
        &tool,
    )?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert!(retries(&sink.events()).is_empty());
    assert!(tool.calls().is_empty());
    Ok(())
}

// TEST: CORE-U-068
#[tokio::test]
async fn core_u_068_a_reply_that_is_only_a_call_object_runs() -> TestResult {
    // The shape qwen3:4b sent on 30 Sep: the whole reply one JSON object, the tool under `function`.
    let tool = fake("write_file", true, vec![output("written", 0)])?;
    let h = harness(
        vec![
            text(
                "{\n  \"function\": \"write_file\",\n  \"arguments\": {\n    \"path\": \"notes.md\"\n  }\n}",
            ),
            answer(),
        ],
        &tool,
    )?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "notes.md"})]);
    assert!(retries(&sink.events()).is_empty());
    Ok(())
}

// TEST: CORE-U-068
#[tokio::test]
async fn core_u_068_json_that_mentions_a_tool_in_an_answer_never_runs() -> TestResult {
    let tool = fake("write_file", true, vec![])?;
    let h = harness(
        vec![text(
            "Here is the JSON you asked for:\n```json\n{\"function\": \"write_file\", \"arguments\": \
             {\"path\": \"notes.md\"}}\n```\nThat's how a call would look.",
        )],
        &tool,
    )?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert!(tool.calls().is_empty(), "nothing ran");
    assert!(retries(&sink.events()).is_empty());
    Ok(())
}
