//! LLM-C-003/LLM-C-025: opaque reasoning state on the Responses protocol, captured whole and replayed unchanged.
#[allow(
    dead_code,
    reason = "This suite uses only some of the shared provider fixtures."
)]
#[path = "fixtures/http.rs"]
mod fixtures;
#[allow(dead_code, reason = "This suite uses only some of the server helpers.")]
#[path = "fixtures/sse_server.rs"]
mod sse_server;

use futures::StreamExt as _;
use serde_json::{Value, json};
use sse_server::{SseServer, fixture};
use std::error::Error;
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    CompletionRequest, ContentPart, FinishReason, LlmError, Message, Registry, Role, Route,
    StreamAssembler, StreamChunk, StreamLimits, ToolCall, Usage, request_body_len,
};
use tqa_testkit::FakeClock;

type TestResult = Result<(), Box<dyn Error>>;

fn route(protocol: &str, base_url: &str, extra: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='test'\nprotocol='{protocol}'\nbase_url='{base_url}'\nmodel='fake-model'\nauth={{kind='none'}}\nnative_tools=true\n{extra}"
    ))?;
    Ok(registry.get("test").ok_or("missing route")?.clone())
}
const POLICY: &str = "reasoning={field='reasoning_effort',allowed=['low'],default='low'}\n";

/// The item as the adapter records it: the received item without its `status`.
fn recorded_item() -> String {
    json!({"type":"reasoning","id":"rs_a","encrypted_content":"b3BhcXVlLXN0YXRlLWE=",
        "summary":[{"type":"summary_text","text":"Plan the call."}]})
    .to_string()
}

#[tokio::test]
async fn llm_c_025_reasoning_items_with_encrypted_content_become_opaque_states() -> TestResult {
    let server = SseServer::start(vec![vec![fixture("reasoning_items.sse")?]]).await?;
    let provider = fixtures::provider(
        route("openai_responses", &server.base_url, POLICY)?,
        &FakeClock::new(),
    )?;
    let chunks: Vec<StreamChunk> = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await?
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_, _>>()?;
    assert_eq!(
        chunks,
        [
            StreamChunk::ReasoningDelta("Plan the call.".into()),
            StreamChunk::ReasoningState(recorded_item()),
            StreamChunk::ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("read_file".into()),
                arguments_delta: r#"{"path":"a"}"#.into(),
            },
            StreamChunk::Usage(Usage {
                prompt_tokens: 20,
                completion_tokens: 9,
                cached_tokens: None,
                reasoning_tokens: Some(6),
            }),
            StreamChunk::Finish(FinishReason::ToolCalls),
        ],
        "an item without encrypted_content is not recorded"
    );
    let mut assembler = StreamAssembler::new(StreamLimits::RECOMMENDED);
    for chunk in &chunks {
        assembler.push(chunk)?;
    }
    assert_eq!(assembler.finish()?.reasoning_state, [recorded_item()]);
    Ok(())
}

#[tokio::test]
async fn llm_c_025_encrypted_content_is_requested_only_with_a_reasoning_policy() -> TestResult {
    for (extra, include) in [
        (POLICY, Some(json!(["reasoning.encrypted_content"]))),
        ("", None),
    ] {
        let server = SseServer::start(vec![vec![fixture("text.sse")?]]).await?;
        let provider = fixtures::provider(
            route("openai_responses", &server.base_url, extra)?,
            &FakeClock::new(),
        )?;
        provider
            .complete(fixtures::request(), CancellationToken::new())
            .await?
            .collect::<Vec<_>>()
            .await;
        assert_eq!(server.requests()[0].body.get("include").cloned(), include);
    }
    Ok(())
}

fn text(role: Role, value: &str) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(value.into())],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}

/// A tool turn whose assistant message carries two recorded states before its text.
fn with_states(states: &[String]) -> CompletionRequest {
    let mut assistant = text(Role::Assistant, "Reading.");
    let mut content: Vec<ContentPart> = states
        .iter()
        .map(|item| ContentPart::ReasoningState(item.clone()))
        .collect();
    content.append(&mut assistant.content);
    assistant.content = content;
    assistant.tool_calls = vec![ToolCall {
        id: "call_1".into(),
        name: "read_file".into(),
        arguments: r#"{"path":"a"}"#.into(),
    }];
    let result = Message {
        tool_call_id: Some("call_1".into()),
        ..text(Role::Tool, "body")
    };
    CompletionRequest {
        messages: vec![text(Role::User, "task"), assistant, result],
        ..CompletionRequest::default()
    }
}

#[tokio::test]
async fn llm_c_003_states_replay_unchanged_before_their_response_items() -> TestResult {
    let second =
        json!({"type":"reasoning","id":"rs_c","encrypted_content":"c2Vjb25k","summary":[]})
            .to_string();
    let states = [recorded_item(), second.clone()];
    let server = SseServer::start(vec![vec![fixture("text.sse")?]]).await?;
    let provider = fixtures::provider(
        route("openai_responses", &server.base_url, POLICY)?,
        &FakeClock::new(),
    )?;
    provider
        .complete(with_states(&states), CancellationToken::new())
        .await?
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        server.requests()[0].body["input"],
        json!([
            {"type":"message","role":"user","content":"task"},
            serde_json::from_str::<Value>(&recorded_item())?,
            serde_json::from_str::<Value>(&second)?,
            {"type":"message","role":"assistant","content":"Reading."},
            {"type":"function_call","call_id":"call_1","name":"read_file","arguments":"{\"path\":\"a\"}"},
            {"type":"function_call_output","call_id":"call_1","output":"body"}
        ])
    );
    // Chat has no reasoning items: the states are left out and the request is otherwise unchanged.
    let chat = route("openai_chat", "http://localhost/v1", "")?;
    assert_eq!(
        request_body_len(&chat, &with_states(&states))?,
        request_body_len(&chat, &with_states(&[]))?
    );
    Ok(())
}

#[test]
fn llm_c_025_misplaced_or_malformed_states_are_refused() -> TestResult {
    let responses = route("openai_responses", "http://localhost/v1", "")?;
    let mut on_user = with_states(&[]);
    on_user.messages[0]
        .content
        .push(ContentPart::ReasoningState(recorded_item()));
    let malformed = with_states(&["not json".into()]);
    let array = with_states(&["[1]".into()]);
    for (request, expected) in [
        (on_user, "reasoning state"),
        (malformed, "reasoning state"),
        (array, "reasoning state"),
    ] {
        let error = request_body_len(&responses, &request)
            .err()
            .ok_or(expected)?;
        assert!(
            matches!(&error, LlmError::Protocol(text) if text.contains(expected)),
            "{error}"
        );
    }
    Ok(())
}
