//! LLM-C-004/C-027/U-029: Messages requests built from our history, thinking and replayed state. Also LLM-U-015: a
//! request effort outside the route's `allowed` values is refused when the request is built.
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
use sse_server::{SseServer, fixture_in};
use std::error::Error;
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    CompletionRequest, ContentPart, LlmError, Message, Registry, Role, Route, ToolCall,
    request_body_len,
};
use tqa_testkit::FakeClock;

type TestResult = Result<(), Box<dyn Error>>;

const THINKING: &str = "max_output=16000\nreasoning={field='thinking',allowed=['low','high'],\
    default='low',budget_tokens={low=2048,high=8192}}\n";

fn route_at(base_url: &str, extra: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='test'\nprotocol='anthropic_messages'\nbase_url='{base_url}'\n\
         model='fake-model'\nauth={{kind='none'}}\n{extra}"
    ))?;
    Ok(registry.get("test").ok_or("missing route")?.clone())
}

/// A route for requests that are refused before any connection.
fn route(extra: &str) -> Result<Route, Box<dyn Error>> {
    route_at("http://127.0.0.1:9/v1", extra)
}

fn message(role: Role, content: Vec<ContentPart>) -> Message {
    Message {
        role,
        content,
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}
fn text(role: Role, value: &str) -> Message {
    message(role, vec![ContentPart::Text(value.into())])
}
fn calls(content: &str, calls: &[(&str, &str)]) -> Message {
    let mut message = text(Role::Assistant, content);
    message.tool_calls = calls
        .iter()
        .map(|(id, arguments)| ToolCall {
            id: (*id).into(),
            name: "read_file".into(),
            arguments: (*arguments).into(),
        })
        .collect();
    message
}
fn result(id: &str, output: &str) -> Message {
    Message {
        tool_call_id: Some(id.into()),
        ..text(Role::Tool, output)
    }
}
fn request(messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest {
        messages,
        tools: vec![fixtures::tool()],
        ..CompletionRequest::default()
    }
}

/// The body the adapter sends for a route with these registry lines, as the server received it.
async fn body(extra: &str, request: &CompletionRequest) -> Result<Value, Box<dyn Error>> {
    let server = SseServer::start(vec![vec![fixture_in("messages", "tools.sse")?]]).await?;
    let route = route_at(&server.base_url, extra)?;
    let provider = fixtures::provider(route.clone(), &FakeClock::new())?;
    let stream = provider
        .complete(request.clone(), CancellationToken::new())
        .await?;
    stream.collect::<Vec<_>>().await;
    let recorded = server.requests().pop().ok_or("no request")?;
    assert_eq!(request_body_len(&route, request)?, recorded.bytes);
    Ok(recorded.body)
}

#[tokio::test]
async fn llm_c_004_the_request_is_built_from_our_history() -> TestResult {
    let server = SseServer::start(vec![vec![fixture_in("messages", "tools.sse")?]]).await?;
    let route = route_at(
        &server.base_url,
        &format!(
            "vision=true\nparallel_tools=false\n{THINKING}extra_body={{service_tier='auto'}}\n"
        ),
    )?;
    let mut user = text(Role::User, "Look:");
    user.content.push(ContentPart::Image {
        media_type: tqa_types::ImageMediaType::Png,
        data: vec![1, 2, 3],
    });
    // The latest tool turn keeps its thinking state, so thinking stays on and the state is replayed.
    let mut second = calls("", &[("call_2", r#""{\"path\":\"b\"}""#)]);
    second
        .content
        .insert(0, ContentPart::ReasoningState(thinking_state("s2")));
    let mut request = request(vec![
        text(Role::System, "Be brief."),
        user,
        calls("Reading.", &[("call_1", r#"{"path":"a"}"#)]),
        result("call_1", "file body"),
        second,
        result("call_2", "second"),
    ]);
    request.reasoning_effort = Some("high".into());
    request.stop = vec!["END".into()];
    request.metadata.insert("run".into(), "r1".into());
    let provider = fixtures::provider(route.clone(), &FakeClock::new())?;
    let stream = provider
        .complete(request.clone(), CancellationToken::new())
        .await?;
    stream.collect::<Vec<_>>().await;
    let recorded = &server.requests()[0];
    assert_eq!(recorded.path, "/v1/messages");
    assert!(
        recorded
            .head
            .contains("\r\nanthropic-version: 2023-06-01\r\n")
    );
    assert_eq!(recorded.body, golden_body());
    assert_eq!(request_body_len(&route, &request)?, recorded.bytes);
    Ok(())
}

/// The LLM-C-004 request body: every block kind, a replayed state and the route's options.
fn golden_body() -> Value {
    let read = |id: &str, input: Value| json!({"type":"tool_use","id":id,"name":"read_file","input":input});
    let tool_result =
        |id: &str, output: &str| json!({"type":"tool_result","tool_use_id":id,"content":output});
    json!({
        "model":"fake-model","max_tokens":16000,"stream":true,
        "system":[{"type":"text","text":"Be brief."}],
        "messages":[
            {"role":"user","content":[{"type":"text","text":"Look:"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}}]},
            {"role":"assistant","content":[{"type":"text","text":"Reading."},read("call_1",json!({"path":"a"}))]},
            {"role":"user","content":[tool_result("call_1","file body")]},
            {"role":"assistant","content":[
                {"type":"thinking","thinking":"Plan.","signature":"s2"},read("call_2",json!({"path":"b"}))]},
            {"role":"user","content":[tool_result("call_2","second")]}
        ],
        "tools":[{"name":"read_file","description":"Read text",
            "input_schema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}],
        "tool_choice":{"type":"auto","disable_parallel_tool_use":true},
        "stop_sequences":["END"],
        "thinking":{"type":"enabled","budget_tokens":8192},
        "service_tier":"auto"
    })
}

#[tokio::test]
async fn llm_u_029_system_notices_and_tool_results_fold_into_user_turns() -> TestResult {
    let history = vec![
        text(Role::System, "Rules."),
        text(Role::System, "Instructions."),
        text(Role::User, "a"),
        calls("", &[("c1", "{}"), ("c2", "[1]")]),
        result("c1", "one"),
        result("c2", "two"),
        text(Role::System, "Environment changed."),
        text(Role::User, "more"),
    ];
    let body = body("max_output=100\nparallel_tools=true\n", &request(history)).await?;
    assert_eq!(
        body["system"],
        json!([{"type":"text","text":"Rules."},{"type":"text","text":"Instructions."}])
    );
    assert_eq!(
        body["messages"],
        json!([
            {"role":"user","content":[{"type":"text","text":"a"}]},
            // A non-object argument was already reported to the model as invalid; `{}` only
            // satisfies the protocol's shape.
            {"role":"assistant","content":[
                {"type":"tool_use","id":"c1","name":"read_file","input":{}},
                {"type":"tool_use","id":"c2","name":"read_file","input":{}}]},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"c1","content":"one"},
                {"type":"tool_result","tool_use_id":"c2","content":"two"},
                {"type":"text","text":"Environment changed."},
                {"type":"text","text":"more"}]}
        ])
    );
    assert!(body.get("thinking").is_none() && body.get("tool_choice").is_none());
    assert!(body.get("metadata").is_none() && body["tools"][0].get("cache_control").is_none());
    Ok(())
}

#[tokio::test]
async fn llm_u_029_cache_breakpoints_follow_the_hint_and_the_prefix_is_stable() -> TestResult {
    let hint = "max_output=100\ncache_hint='anthropic_cache_control'\n";
    let mut history = vec![text(Role::System, "Rules."), text(Role::System, "Tools.")];
    let mut prefix = None;
    for step in 0..20 {
        history.push(text(Role::User, &format!("step {step}")));
        let body = body(hint, &request(history.clone())).await?;
        let marked = json!({"type":"ephemeral"});
        assert_eq!(body["system"][1]["cache_control"], marked);
        assert!(body["system"][0].get("cache_control").is_none());
        assert_eq!(body["tools"][0]["cache_control"], marked);
        let bytes = serde_json::to_vec(&(&body["system"], &body["tools"]))?;
        assert_eq!(
            prefix.get_or_insert_with(|| bytes.clone()),
            &bytes,
            "step {step}"
        );
        history.push(text(Role::Assistant, "ok"));
    }
    Ok(())
}

#[test]
fn llm_u_029_unsupported_histories_fail_before_sending() -> TestResult {
    let plain = route("max_output=100\n")?;
    let thinking = route(THINKING)?;
    let mut hot = request(vec![text(Role::User, "a")]);
    hot.temperature = Some(0.5);
    let mut small = request(vec![text(Role::User, "a")]);
    small.max_output = Some(2048);
    let mut effort = request(vec![text(Role::User, "a")]);
    effort.reasoning_effort = Some("extreme".into());
    let mut no_output = plain.clone();
    no_output.limits.max_output = None;
    let cases = [
        (
            &plain,
            request(vec![text(Role::System, "only")]),
            "at least one message",
        ),
        (
            &plain,
            request(vec![text(Role::Assistant, "hi")]),
            "start with a user message",
        ),
        (&plain, request(vec![image(Role::User)]), "image input"),
        (
            &route("max_output=100\nvision=true\n")?,
            request(vec![text(Role::User, "a"), image(Role::Assistant)]),
            "only sent in user messages",
        ),
        (&plain, request(vec![text(Role::Tool, "x")]), "tool_call_id"),
        (
            &route("max_output=100\nnative_tools=false\n")?,
            request(vec![text(Role::User, "a")]),
            "native tools",
        ),
        (&thinking, hot, "temperature"),
        (&thinking, small, "above the thinking budget"),
        (&thinking, effort, "reasoning effort"),
        (
            &no_output,
            request(vec![text(Role::User, "a")]),
            "max_output",
        ),
    ];
    for (route, request, expected) in cases {
        let error = request_body_len(route, &request).err().ok_or(expected)?;
        assert!(
            matches!(&error, LlmError::Protocol(text) if text.contains(expected)),
            "{expected}: {error}"
        );
    }
    Ok(())
}

fn image(role: Role) -> Message {
    message(
        role,
        vec![ContentPart::Image {
            media_type: tqa_types::ImageMediaType::Png,
            data: vec![1],
        }],
    )
}

fn thinking_state(signature: &str) -> String {
    json!({"type":"thinking","thinking":"Plan.","signature":signature}).to_string()
}

#[tokio::test]
async fn llm_c_027_states_are_replayed_first_and_unchanged() -> TestResult {
    let mut first = calls("Reading.", &[("c1", "{}")]);
    first
        .content
        .insert(0, ContentPart::ReasoningState(thinking_state("s1")));
    let mut second = calls("", &[("c2", "{}")]);
    let redacted = json!({"type":"redacted_thinking","data":"ZW5j"}).to_string();
    second.content = vec![
        ContentPart::ReasoningState(redacted),
        ContentPart::ReasoningState(thinking_state("s2")),
    ];
    let history = vec![
        text(Role::User, "a"),
        first,
        result("c1", "x"),
        second,
        result("c2", "y"),
    ];
    let body = body(THINKING, &request(history)).await?;
    assert_eq!(
        body["thinking"],
        json!({"type":"enabled","budget_tokens":2048})
    );
    assert_eq!(
        body["messages"][1]["content"][0],
        json!({"type":"thinking","thinking":"Plan.","signature":"s1"})
    );
    assert_eq!(body["messages"][1]["content"][1]["type"], "text");
    assert_eq!(
        body["messages"][3]["content"],
        json!([{"type":"redacted_thinking","data":"ZW5j"},
            {"type":"thinking","thinking":"Plan.","signature":"s2"},
            {"type":"tool_use","id":"c2","name":"read_file","input":{}}])
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_027_a_latest_tool_turn_without_state_sends_no_thinking() -> TestResult {
    let mut first = calls("", &[("c1", "{}")]);
    first
        .content
        .insert(0, ContentPart::ReasoningState(thinking_state("s1")));
    let history = vec![
        text(Role::User, "a"),
        first,
        result("c1", "x"),
        calls("", &[("c2", "{}")]),
        result("c2", "y"),
    ];
    let lost = body(THINKING, &request(history.clone())).await?;
    assert!(lost.get("thinking").is_none());
    assert!(
        !lost.to_string().contains("\"signature\""),
        "no state is sent"
    );
    let mut next = history;
    next.push(text(Role::Assistant, "done"));
    next.push(text(Role::User, "again"));
    let body = body(THINKING, &request(next)).await?;
    assert_eq!(body["thinking"]["type"], "enabled", "only that one request");
    Ok(())
}

#[test]
fn llm_c_027_foreign_or_malformed_states_are_refused() -> TestResult {
    for state in [
        json!({"type":"reasoning","encrypted_content":"x"}).to_string(),
        "[1]".to_owned(),
        "not json".to_owned(),
    ] {
        let mut assistant = calls("", &[("c1", "{}")]);
        assistant
            .content
            .insert(0, ContentPart::ReasoningState(state.clone()));
        let history = vec![text(Role::User, "a"), assistant, result("c1", "x")];
        let error = request_body_len(&route(THINKING)?, &request(history))
            .err()
            .ok_or("accepted")?;
        assert!(
            matches!(&error, LlmError::Protocol(text) if text.contains("reasoning state")),
            "{state}: {error}"
        );
    }
    Ok(())
}

/// LLM-U-044: the provider refuses an empty system text block (observed: "system: text content blocks must be
/// non-empty"), and the runner's prompt has slots that can be empty. So empty and whitespace-only system texts are left
/// out, no `system` is sent when none remains, and the cache marker goes on the last text kept.
#[tokio::test]
async fn llm_u_044_empty_system_texts_are_left_out() -> TestResult {
    let request = |system: &[&str]| {
        let mut messages: Vec<Message> = system
            .iter()
            .map(|text_| text(Role::System, text_))
            .collect();
        messages.push(text(Role::User, "hi"));
        CompletionRequest {
            messages,
            ..CompletionRequest::default()
        }
    };
    let sent = body(
        "max_output=1000\n",
        &request(&["", "Be brief.", "  \n\t", "Use tabs."]),
    )
    .await?;
    assert_eq!(
        sent["system"],
        json!([{"type":"text","text":"Be brief."},{"type":"text","text":"Use tabs."}])
    );
    let none = body("max_output=1000\n", &request(&["", " "])).await?;
    assert!(none.get("system").is_none(), "{none}");
    let cached = body(
        "max_output=1000\ncache_hint='anthropic_cache_control'\n",
        &request(&["Be brief.", ""]),
    )
    .await?;
    assert_eq!(
        cached["system"],
        json!([{"type":"text","text":"Be brief.","cache_control":{"type":"ephemeral"}}])
    );
    Ok(())
}
