//! LLM-C-029: the Chat adapter against the two hosted Chat provider fakes, in CI. The request set is accepted as
//! production sends it; the fakes' refusals are classified and reach the user as the provider's words.
#[path = "fixtures/provider_matrix.rs"]
mod matrix;

use matrix::{answers, request_set, send};
use serde_json::json;
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    CompletionRequest, HttpConfig, LlmError, Registry, Role, Route, StreamChunk, StreamLimits,
    ToolSpec, provider_for,
};
use tqa_testkit::{FakeClock, FakeModelServer, FakeProfile, FakeSleeper};

type TestResult = Result<(), Box<dyn Error>>;

fn route(base_url: &str) -> Result<Route, Box<dyn Error>> {
    route_with(base_url, "")
}

/// The hosted route with `extra` route fields appended.
fn route_with(base_url: &str, extra: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='hosted'\nprotocol='openai_chat'\nbase_url='{base_url}'\nmodel='fake-model'\n\
         auth={{kind='none'}}\nnative_tools=true\nparallel_tools=true\nstream_usage=true\n{extra}"
    ))?;
    Ok(registry.get("hosted").ok_or("missing route")?.clone())
}

async fn server(profile: FakeProfile, scripts: usize) -> Result<FakeModelServer, Box<dyn Error>> {
    Ok(FakeModelServer::start_as(
        profile,
        answers(scripts),
        Arc::new(FakeSleeper::new(FakeClock::new())),
    )
    .await?)
}

#[tokio::test]
async fn llm_c_029_deepseek_accepts_the_request_set_as_production_sends_it() -> TestResult {
    let set = request_set();
    let server = server(FakeProfile::DeepseekChat, set.len()).await?;
    let route = route(server.base_url())?;
    for (name, request) in &set {
        let chunks = send(&route, "/chat/completions", request)
            .await?
            .map_err(|(status, body)| format!("{name}: refused {status} {body}"))?;
        assert!(
            chunks
                .iter()
                .any(|chunk| matches!(chunk, StreamChunk::TextDelta(text) if text == "ok")),
            "{name}: {chunks:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn llm_c_029_deepseek_refusals_are_permanent_and_carry_the_providers_words() -> TestResult {
    let clock = FakeClock::new();
    let server = server(FakeProfile::DeepseekChat, 1).await?;
    let provider = provider_for(
        {
            // strict = "on": the default (auto) sends an open schema non-strict, which keeps real runs from being refused.
            let mut route = route(server.base_url())?;
            route.strict = Some("on".into());
            route
        },
        None,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?;
    let request = CompletionRequest {
        messages: vec![matrix::message(Role::User, "hi")],
        tools: vec![ToolSpec {
            name: "list_files".into(),
            description: "List".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            strict: true,
        }],
        ..CompletionRequest::default()
    };
    let Err(error) = provider.complete(request, CancellationToken::new()).await else {
        return Err("the fake accepted an open strict schema".into());
    };
    assert!(
        matches!(error, LlmError::Http { status: 400, .. }),
        "{error:?}"
    );
    assert!(!error.retryable());
    assert!(
        error
            .provider_message()
            .is_some_and(|message| message.starts_with("Invalid schema for function 'list_files'")),
        "{error:?}"
    );
    assert_eq!(server.requests().len(), 1, "no retry");
    Ok(())
}

/// With the basic keyword subset, the compatible endpoint accepts the request set as production sends it: nothing the
/// subset keeps is refused there (`additionalProperties` included, which it refuses).
#[tokio::test]
async fn llm_c_029_the_compatible_endpoint_accepts_basic_schema_keywords() -> TestResult {
    // Only requests without earlier tool calls: a round trip must carry each call's signature back, which history
    // made elsewhere doesn't have (the two-step case covers round trips).
    let set: Vec<_> = request_set()
        .into_iter()
        .filter(|(_, request)| {
            request
                .messages
                .iter()
                .all(|message| message.tool_calls.is_empty())
        })
        .collect();
    assert!(!set.is_empty());
    let server = server(FakeProfile::GeminiOpenaiCompat, set.len()).await?;
    let route = route_with(server.base_url(), "schema_keywords='basic'\n")?;
    for (name, request) in &set {
        let chunks = send(&route, "/chat/completions", request)
            .await?
            .map_err(|(status, body)| format!("{name}: refused {status} {body}"))?;
        assert!(
            chunks
                .iter()
                .any(|chunk| matches!(chunk, StreamChunk::TextDelta(text) if text == "ok")),
            "{name}: {chunks:?}"
        );
    }
    Ok(())
}

/// Without the basic keyword subset, the compatible endpoint refuses a strict tool's `additionalProperties`: such a route
/// needs `schema_keywords = "basic"` (LLM-U-041), and its words reach the user.
#[tokio::test]
async fn llm_c_029_the_compatible_endpoint_refuses_full_schema_keywords() -> TestResult {
    let set = request_set();
    let server = server(FakeProfile::GeminiOpenaiCompat, 0).await?;
    let route = route(server.base_url())?;
    let (name, request) = &set[0];
    let Err((status, body)) = send(&route, "/chat/completions", request).await? else {
        return Err(format!("{name}: accepted full schema keywords").into());
    };
    assert_eq!(status, 400);
    let error = LlmError::Http {
        status,
        body_excerpt: body,
    };
    assert!(
        error.provider_message().is_some_and(|message| message
            .starts_with("Invalid JSON payload received. Unknown name \"additionalProperties\"")),
        "{error:?}"
    );
    assert!(!error.retryable());
    Ok(())
}

/// The compatible endpoint streams tool-call deltas without `index`, as seen live; a single-call stream decodes.
#[tokio::test]
async fn llm_c_029_a_single_tool_call_without_an_index_decodes() -> TestResult {
    let script = tqa_testkit::Script {
        header_delay: std::time::Duration::ZERO,
        events: vec![tqa_testkit::Scripted::ToolCall {
            name: "read_file".into(),
            arguments_json: r#"{"path":"a"}"#.into(),
            split_into: 3,
        }],
    };
    let server = FakeModelServer::start_as(
        FakeProfile::GeminiOpenaiCompat,
        vec![script],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    )
    .await?;
    let mut route = route(server.base_url())?;
    route.schema_keywords = Some("basic".into());
    let clock = FakeClock::new();
    let provider = provider_for(
        route,
        None,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?;
    let (_, request) = request_set().remove(0);
    let stream = provider.complete(request, CancellationToken::new()).await?;
    let completion = tqa_llm::collect(stream, StreamLimits::RECOMMENDED).await?;
    let calls = &completion.tool_calls;
    assert_eq!(calls.len(), 1, "{completion:?}");
    assert_eq!(
        (calls[0].name.as_str(), calls[0].arguments.as_str()),
        ("read_file", r#"{"path":"a"}"#)
    );
    Ok(())
}

/// Step 1 of a tool loop against the compatible endpoint: the fake answers one signed `read_file` call.
async fn signed_first_step() -> Result<
    (
        FakeModelServer,
        Arc<dyn tqa_llm::ModelProvider>,
        CompletionRequest,
        tqa_llm::Completion,
    ),
    Box<dyn Error>,
> {
    let call = tqa_testkit::Script {
        header_delay: std::time::Duration::ZERO,
        events: vec![tqa_testkit::Scripted::ToolCall {
            name: "read_file".into(),
            arguments_json: r#"{"path":"a"}"#.into(),
            split_into: 2,
        }],
    };
    let server = FakeModelServer::start_as(
        FakeProfile::GeminiOpenaiCompat,
        vec![call, answers(1).remove(0)],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    )
    .await?;
    let mut route = route(server.base_url())?;
    route.schema_keywords = Some("basic".into());
    let clock = FakeClock::new();
    let provider = provider_for(
        route,
        None,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?;
    let request = CompletionRequest {
        messages: vec![
            matrix::message(Role::System, "You are a coding agent."),
            matrix::message(Role::User, "Read a."),
        ],
        tools: vec![matrix::strict_tool()],
        ..CompletionRequest::default()
    };
    let stream = provider
        .complete(request.clone(), CancellationToken::new())
        .await?;
    let first = tqa_llm::collect(stream, StreamLimits::RECOMMENDED).await?;
    Ok((server, provider, request, first))
}

/// Step 2: the call sent back with its result; `replay` says whether the completion's opaque states go with it.
fn second_step(
    request: &CompletionRequest,
    first: &tqa_llm::Completion,
    replay: bool,
) -> CompletionRequest {
    let states = first
        .reasoning_state
        .iter()
        .filter(|_| replay)
        .map(|item| tqa_llm::ContentPart::ReasoningState(item.clone()));
    let assistant = tqa_llm::Message {
        content: states
            .chain([tqa_llm::ContentPart::Text(String::new())])
            .collect(),
        tool_calls: first.tool_calls.clone(),
        ..matrix::message(Role::Assistant, "")
    };
    let result = tqa_llm::Message {
        tool_call_id: first.tool_calls.first().map(|call| call.id.clone()),
        ..matrix::message(Role::Tool, "contents")
    };
    let mut messages = request.messages.clone();
    messages.extend([assistant, result]);
    CompletionRequest {
        messages,
        ..request.clone()
    }
}

/// Without the call's opaque state sent back, the round trip is refused with the provider's words, permanently.
#[tokio::test]
async fn llm_c_029_a_round_trip_without_the_calls_signature_is_refused() -> TestResult {
    let (_server, provider, request, first) = signed_first_step().await?;
    assert_eq!(first.tool_calls.len(), 1, "{first:?}");
    let Err(error) = provider
        .complete(
            second_step(&request, &first, false),
            CancellationToken::new(),
        )
        .await
    else {
        return Err("the round trip was accepted without the signature".into());
    };
    assert!(
        matches!(error, LlmError::Http { status: 400, .. }),
        "{error:?}"
    );
    assert!(!error.retryable());
    assert!(
        error.provider_message().is_some_and(
            |message| message.starts_with("Function call is missing a thought_signature")
        ),
        "{error:?}"
    );
    Ok(())
}

/// A two-step tool loop: the call's opaque state comes back as reasoning state and goes out on its call, so the
/// endpoint accepts the round trip (red until the Chat decoder keeps it and the encoder puts it back, LLM-U-043).
#[tokio::test]
async fn llm_c_029_a_two_step_loop_carries_the_calls_signature_back() -> TestResult {
    let (_server, provider, request, first) = signed_first_step().await?;
    assert!(
        !first.reasoning_state.is_empty(),
        "the call's opaque state was kept: {first:?}"
    );
    let stream = provider
        .complete(
            second_step(&request, &first, true),
            CancellationToken::new(),
        )
        .await?;
    let second = tqa_llm::collect(stream, StreamLimits::RECOMMENDED).await?;
    assert_eq!(second.text, "ok");
    Ok(())
}
