//! The provider matrix (LLM-C-029..031): one request set every adapter sends to every must-pass provider fake, sent
//! as production sends it (the adapter's bytes, then the relay's shaping), plus the helpers to send it.

use futures::StreamExt as _;
use serde_json::{Value, json};
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    CompletionRequest, ContentPart, HttpConfig, Message, Role, Route, StreamChunk, StreamLimits,
    ToolCall, ToolSpec, decode_response, encode_request, relay_shaping::shape_request,
};
use tqa_testkit::{FakeClock, FakeSleeper, Script, Scripted};

pub fn message(role: Role, text: &str) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(text.into())],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}

/// A tool whose schema meets the strict rules, sent strict, as `strict_compatible` lets the runner send it.
pub fn strict_tool() -> ToolSpec {
    ToolSpec {
        name: "read_file".into(),
        description: "Read a workspace file".into(),
        parameters: json!({"type":"object","properties":{"path":{"type":"string"}},
            "required":["path"],"additionalProperties":false}),
        strict: true,
    }
}

/// A tool with an optional argument, which breaks the strict rules, so the runner sends it without strict.
pub fn loose_tool() -> ToolSpec {
    ToolSpec {
        name: "list_files".into(),
        description: "List a directory".into(),
        parameters: json!({"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},
            "required":["path"]}),
        strict: false,
    }
}

fn call(id: &str, path: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "read_file".into(),
        arguments: format!(r#"{{"path":"{path}"}}"#),
    }
}

fn result(id: &str, output: &str) -> Message {
    Message {
        tool_call_id: Some(id.into()),
        ..message(Role::Tool, output)
    }
}

/// The runner's three leading slots: the system prompt, an empty instructions slot and the tools text.
fn slots() -> Vec<Message> {
    vec![
        message(
            Role::System,
            "You are a coding agent working in a sandboxed workspace.",
        ),
        message(Role::System, ""),
        message(
            Role::System,
            r#"[{"name":"read_file"},{"name":"list_files"}]"#,
        ),
    ]
}

/// The request set, named: every request here must be accepted by every provider fake an adapter serves.
pub fn request_set() -> Vec<(&'static str, CompletionRequest)> {
    let tools = vec![strict_tool(), loose_tool()];
    let conversation: Vec<Message> = slots()
        .into_iter()
        .chain([
            message(Role::User, "hello"),
            message(Role::User, "hello"),
            message(Role::User, "try again "),
            message(Role::System, "Environment changed to workstation."),
            message(Role::User, "hi"),
        ])
        .collect();
    let round_trip: Vec<Message> = slots()
        .into_iter()
        .chain([
            message(Role::User, "Read a and b."),
            Message {
                tool_calls: vec![call("call_1", "a"), call("call_2", "b")],
                ..message(Role::Assistant, "Reading both.")
            },
            result("call_1", "first file"),
            result("call_2", "second file"),
        ])
        .collect();
    vec![
        (
            "the runner's slots, four user turns and a later notice",
            CompletionRequest {
                messages: conversation,
                tools: tools.clone(),
                ..CompletionRequest::default()
            },
        ),
        (
            "a parallel tool round trip, strict and non-strict tools",
            CompletionRequest {
                messages: round_trip.clone(),
                tools: tools.clone(),
                ..CompletionRequest::default()
            },
        ),
        (
            "a reasoning effort the route maps",
            CompletionRequest {
                messages: round_trip,
                tools,
                reasoning_effort: Some("high".into()),
                ..CompletionRequest::default()
            },
        ),
    ]
}

/// One scripted answer per request.
pub fn answers(count: usize) -> Vec<Script> {
    (0..count)
        .map(|_| Script {
            header_delay: std::time::Duration::ZERO,
            events: vec![
                Scripted::Text("ok".into()),
                Scripted::Finish(tqa_llm::FinishReason::Stop),
            ],
        })
        .collect()
}

/// Sends a request as production does: the adapter's bytes, the relay's shaping for the route, then the upstream.
/// Returns the status and, on success, the decoded chunks; on a refusal, the error body.
pub async fn send(
    route: &Route,
    path: &str,
    request: &CompletionRequest,
) -> Result<Result<Vec<StreamChunk>, (u16, String)>, Box<dyn Error>> {
    let mut body: Value = serde_json::from_slice(&encode_request(route, request)?)?;
    shape_request(route, &mut body)?;
    let url = format!("{}{path}", route.base_url.trim_end_matches('/'));
    let response = reqwest::Client::new().post(url).json(&body).send().await?;
    if !response.status().is_success() {
        return Ok(Err((response.status().as_u16(), response.text().await?)));
    }
    let clock = FakeClock::new();
    let stream = decode_response(
        route.protocol,
        response,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(FakeSleeper::new(clock)),
        CancellationToken::new(),
    );
    Ok(Ok(stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?))
}
