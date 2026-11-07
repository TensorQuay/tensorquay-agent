//! LLM-C-003/LLM-C-024: the `openai_responses` adapter against recorded-shape SSE fixtures and its request shape.
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
use serde_json::json;
use sse_server::{SseServer, event, fixture};
use std::{collections::BTreeMap, error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    CompletionRequest, ContentPart, FinishReason, LlmError, Message, Registry, Role, Route,
    StreamChunk, ToolCall, Usage, request_body_len,
};
use tqa_testkit::FakeClock;

type TestResult = Result<(), Box<dyn Error>>;

fn responses_route(base_url: &str, extra: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='test'\nprotocol='openai_responses'\nbase_url='{base_url}'\nmodel='fake-model'\nauth={{kind='none'}}\n{extra}"
    ))?;
    Ok(registry.get("test").ok_or("missing route")?.clone())
}

/// Streams one fixture body, split into pieces of `piece` characters, and returns its chunks.
async fn chunks(
    body: &str,
    piece: usize,
) -> Result<Vec<Result<StreamChunk, LlmError>>, Box<dyn Error>> {
    let pieces: Vec<String> = body
        .chars()
        .collect::<Vec<_>>()
        .chunks(piece)
        .map(|part| part.iter().collect())
        .collect();
    let server = SseServer::start(vec![pieces]).await?;
    let provider = fixtures::provider(responses_route(&server.base_url, "")?, &FakeClock::new())?;
    let stream = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await?;
    let result = stream.collect::<Vec<_>>().await;
    assert_eq!(server.requests()[0].path, "/v1/responses");
    Ok(result)
}

async fn ok_chunks(name: &str) -> Result<Vec<StreamChunk>, Box<dyn Error>> {
    let body = fixture(name)?;
    let whole: Vec<StreamChunk> = chunks(&body, usize::MAX)
        .await?
        .into_iter()
        .collect::<Result<_, _>>()?;
    let split: Vec<StreamChunk> = chunks(&body, 5)
        .await?
        .into_iter()
        .collect::<Result<_, _>>()?;
    assert_eq!(whole, split, "{name}: framing must not change the result");
    Ok(whole)
}

fn usage(prompt: u64, completion: u64, cached: Option<u64>, reasoning: Option<u64>) -> StreamChunk {
    StreamChunk::Usage(Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        cached_tokens: cached,
        reasoning_tokens: reasoning,
    })
}

#[tokio::test]
async fn llm_c_024_text_streams_with_usage_and_stop() -> TestResult {
    assert_eq!(
        ok_chunks("text.sse").await?,
        [
            StreamChunk::TextDelta("Hel".into()),
            StreamChunk::TextDelta("lo, wörld".into()),
            usage(12, 5, Some(4), Some(0)),
            StreamChunk::Finish(FinishReason::Stop),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_003_reasoning_summary_then_text() -> TestResult {
    assert_eq!(
        ok_chunks("reasoning.sse").await?,
        [
            StreamChunk::ReasoningDelta("Check the ".into()),
            StreamChunk::ReasoningDelta("file first.".into()),
            StreamChunk::TextDelta("Done.".into()),
            usage(30, 20, Some(0), Some(7)),
            StreamChunk::Finish(FinishReason::Stop),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_003_parallel_calls_are_emitted_whole_when_each_is_done() -> TestResult {
    let chunks = ok_chunks("parallel_tools.sse").await?;
    let call = |index: u32, id: &str, name: &str, arguments: &str| StreamChunk::ToolCallDelta {
        index,
        id: Some(id.into()),
        name: Some(name.into()),
        arguments_delta: arguments.into(),
    };
    assert_eq!(
        chunks,
        [
            StreamChunk::ReasoningDelta("Two lookups.".into()),
            call(1, "call_b", "grep", r#"{"pattern":"fn mäin"}"#),
            call(0, "call_a", "read_file", r#"{"path":"src/lib.rs"}"#),
            usage(40, 22, None, None),
            StreamChunk::Finish(FinishReason::ToolCalls),
        ]
    );
    let mut assembler = tqa_llm::StreamAssembler::new(tqa_llm::StreamLimits::RECOMMENDED);
    for chunk in &chunks {
        assembler.push(chunk)?;
    }
    assert_eq!(
        assembler.finish()?.tool_calls,
        [
            ToolCall {
                id: "call_a".into(),
                name: "read_file".into(),
                arguments: r#"{"path":"src/lib.rs"}"#.into()
            },
            ToolCall {
                id: "call_b".into(),
                name: "grep".into(),
                arguments: r#"{"pattern":"fn mäin"}"#.into()
            },
        ]
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_024_incomplete_maps_its_reason() -> TestResult {
    assert_eq!(
        ok_chunks("incomplete.sse").await?,
        [
            StreamChunk::TextDelta("A long answ".into()),
            usage(9, 64, None, None),
            StreamChunk::Finish(FinishReason::Length),
        ]
    );
    for (reason, expected) in [
        ("content_filter", FinishReason::ContentFilter),
        (
            "a_future_reason",
            FinishReason::Other("a_future_reason".into()),
        ),
        ("", FinishReason::Other("incomplete".into())),
    ] {
        let details = if reason.is_empty() {
            json!(null)
        } else {
            json!({ "reason": reason })
        };
        let body = event(
            &json!({"type":"response.incomplete","response":{"status":"incomplete",
            "incomplete_details":details,"usage":{"input_tokens":1,"output_tokens":2}}}),
        );
        let result: Vec<StreamChunk> = chunks(&body, usize::MAX)
            .await?
            .into_iter()
            .collect::<Result<_, _>>()?;
        assert_eq!(
            result,
            [usage(1, 2, None, None), StreamChunk::Finish(expected)]
        );
    }
    Ok(())
}

#[tokio::test]
async fn llm_c_024_unknown_events_items_and_fields_are_ignored() -> TestResult {
    assert_eq!(
        ok_chunks("unknown.sse").await?,
        [
            StreamChunk::TextDelta("Still fine.".into()),
            usage(3, 2, None, None),
            StreamChunk::Finish(FinishReason::Stop),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_024_bytes_after_the_terminal_event_are_not_read() -> TestResult {
    let body = format!(
        "{}event: response.output_text.delta\ndata: not json\n\n",
        fixture("text.sse")?
    );
    let result: Vec<StreamChunk> = chunks(&body, usize::MAX)
        .await?
        .into_iter()
        .collect::<Result<_, _>>()?;
    assert_eq!(
        result.last(),
        Some(&StreamChunk::Finish(FinishReason::Stop))
    );
    Ok(())
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
fn text(value: &str) -> Vec<ContentPart> {
    vec![ContentPart::Text(value.into())]
}
fn call(id: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "read_file".into(),
        arguments: arguments.into(),
    }
}
fn result(id: &str, output: &str) -> Message {
    Message {
        tool_call_id: Some(id.into()),
        name: Some("read_file".into()),
        ..message(Role::Tool, text(output))
    }
}

/// A history with every item kind: system, a user image, assistant text with a call, a call without
/// text, and results.
fn full_request() -> CompletionRequest {
    let mut user = message(
        Role::User,
        vec![
            ContentPart::Text("Look:".into()),
            ContentPart::Image {
                media_type: tqa_types::ImageMediaType::Png,
                data: vec![1, 2, 3],
            },
        ],
    );
    user.name = Some("participant".into());
    let mut first = message(Role::Assistant, text("Reading."));
    first.tool_calls = vec![call("call_1", r#"{"path":"a"}"#)];
    let mut second = message(Role::Assistant, vec![]);
    second.tool_calls = vec![call("call_2", r#""{\"path\":\"b\"}""#)];
    CompletionRequest {
        messages: vec![
            message(Role::System, text("Be brief.")),
            user,
            first,
            result("call_1", "file body"),
            second,
            result("call_2", "second"),
        ],
        tools: vec![fixtures::tool()],
        reasoning_effort: Some("high".into()),
        max_output: Some(100),
        temperature: Some(0.5),
        stop: vec![],
        metadata: BTreeMap::from([("run".into(), "r1".into())]),
        declare_tools: false,
        // The run travels as a header to the relay only: the exact body below holds none of it (LLM-U-049).
        run_id: tqa_types::RunId::try_from("run-in-header".to_owned()).ok(),
    }
}

const ROUTE_EXTRA: &str = "vision=true\nnative_tools=true\nparallel_tools=true\n\
    reasoning={field='reasoning_effort',allowed=['low','high'],default='low'}\n\
    extra_body={reasoning={summary='auto'},service_tier='flex'}\n";

#[tokio::test]
async fn llm_c_003_the_request_is_built_from_our_history_without_server_state() -> TestResult {
    let server = SseServer::start(vec![vec![fixture("text.sse")?]]).await?;
    let route = responses_route(&server.base_url, ROUTE_EXTRA)?;
    let provider = fixtures::provider(route.clone(), &FakeClock::new())?;
    let request = full_request();
    let stream = provider
        .complete(request.clone(), CancellationToken::new())
        .await?;
    stream.collect::<Vec<_>>().await;
    let recorded = &server.requests()[0];
    assert_eq!(recorded.path, "/v1/responses");
    assert_eq!(
        recorded.body,
        json!({
            "model": "fake-model",
            "stream": true,
            "store": false,
            "include": ["reasoning.encrypted_content"],
            "instructions": "Be brief.",
            "input": [
                {"type":"message","role":"user","content":[
                    {"type":"input_text","text":"Look:"},
                    {"type":"input_image","image_url":"data:image/png;base64,AQID"}]},
                {"type":"message","role":"assistant","content":"Reading."},
                {"type":"function_call","call_id":"call_1","name":"read_file","arguments":"{\"path\":\"a\"}"},
                {"type":"function_call_output","call_id":"call_1","output":"file body"},
                {"type":"function_call","call_id":"call_2","name":"read_file","arguments":"{\"path\":\"b\"}"},
                {"type":"function_call_output","call_id":"call_2","output":"second"}
            ],
            "tools": [{"type":"function","name":"read_file","description":"Read text",
                "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false},
                "strict":true}],
            "parallel_tool_calls": true,
            "reasoning": {"effort":"high","summary":"auto"},
            "max_output_tokens": 100,
            "temperature": 0.5,
            "metadata": {"run":"r1"},
            "service_tier": "flex"
        })
    );
    assert_eq!(request_body_len(&route, &request)?, recorded.bytes);
    Ok(())
}

#[tokio::test]
async fn llm_c_024_the_route_default_effort_and_limit_apply() -> TestResult {
    let server = SseServer::start(vec![vec![fixture("text.sse")?]]).await?;
    let route = responses_route(
        &server.base_url,
        "reasoning={field='reasoning_effort',allowed=['low'],default='low'}\nmax_output=64\n",
    )?;
    let provider = fixtures::provider(route, &FakeClock::new())?;
    let stream = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await?;
    stream.collect::<Vec<_>>().await;
    let body = &server.requests()[0].body;
    assert_eq!(body["reasoning"], json!({"effort":"low"}));
    assert_eq!(body["max_output_tokens"], json!(64));
    assert!(
        body.get("reasoning_effort").is_none(),
        "the Chat field is not used"
    );
    assert!(body.get("tools").is_none() && body.get("parallel_tool_calls").is_none());
    Ok(())
}

#[tokio::test]
async fn llm_c_024_unsupported_inputs_fail_before_any_request() -> TestResult {
    let server = SseServer::start(vec![]).await?;
    let plain = responses_route(&server.base_url, "")?;
    let mut stop = fixtures::request();
    stop.stop = vec!["END".into()];
    let mut tools = fixtures::request();
    tools.tools.push(fixtures::tool());
    let mut empty = fixtures::request();
    empty.messages.clear();
    let mut temperature = fixtures::request();
    temperature.temperature = Some(f32::NAN);
    let mut user_call = fixtures::request();
    user_call.messages[1].tool_calls = vec![call("c", "{}")];
    let mut assistant_image = full_request();
    assistant_image.messages[2]
        .content
        .push(ContentPart::Image {
            media_type: tqa_types::ImageMediaType::Png,
            data: vec![1],
        });
    let mut effort = fixtures::request();
    effort.reasoning_effort = Some("extreme".into());
    let cases = [
        (plain.clone(), stop, "stop sequences"),
        (
            responses_route(&server.base_url, "native_tools=false\n")?,
            tools,
            "native tools",
        ),
        // An image on a tool result is no longer refused: it goes as a user message after the tool outputs
        // (LLM-U-052, ADR-0039).
        (
            responses_route(&server.base_url, "native_tools=true\n")?,
            full_request(),
            "image input",
        ),
        (
            responses_route(&server.base_url, ROUTE_EXTRA)?,
            effort,
            "reasoning effort",
        ),
        (plain.clone(), empty, "at least one message"),
        (plain.clone(), temperature, "finite"),
        (plain.clone(), user_call, "only assistant messages"),
        (
            responses_route(&server.base_url, ROUTE_EXTRA)?,
            assistant_image,
            "only sent in user messages",
        ),
    ];
    for (route, request, expected) in cases {
        let error = request_body_len(&route, &request).err().ok_or(expected)?;
        assert!(
            matches!(&error, LlmError::Protocol(text) if text.contains(expected)),
            "{expected}: {error}"
        );
        let provider = fixtures::provider(route, &FakeClock::new())?;
        assert!(
            provider
                .complete(request, CancellationToken::new())
                .await
                .is_err()
        );
    }
    assert!(server.requests().is_empty());
    Ok(())
}

#[tokio::test]
async fn llm_c_024_a_cancelled_request_is_not_sent() -> TestResult {
    let server = SseServer::start(vec![]).await?;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let provider = fixtures::provider(responses_route(&server.base_url, "")?, &FakeClock::new())?;
    assert!(matches!(
        provider.complete(fixtures::request(), cancel).await,
        Err(LlmError::Cancelled)
    ));
    assert!(server.requests().is_empty());
    Ok(())
}

async fn collected(
    provider: &Arc<dyn tqa_llm::ModelProvider>,
) -> Result<tqa_llm::Completion, LlmError> {
    let stream = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await?;
    tqa_llm::collect(stream, tqa_llm::StreamLimits::RECOMMENDED).await
}

fn indexed(index: u32, id: Option<&str>, name: Option<&str>, delta: &str) -> tqa_testkit::Scripted {
    tqa_testkit::Scripted::ToolCallDelta {
        index,
        id: id.map(str::to_owned),
        name: name.map(str::to_owned),
        arguments_delta: delta.into(),
    }
}

/// Scripts covering every rendered shape: runs of text and reasoning, split and interleaved calls,
/// usage, every non-stop finish and each framing variation.
fn shared_scripts() -> Vec<Vec<tqa_testkit::Scripted>> {
    use tqa_testkit::{MalformedSse, Scripted};
    let framed = |variant| {
        vec![
            Scripted::Text("framed ✓".into()),
            Scripted::MalformedSse(variant),
        ]
    };
    vec![
        vec![
            Scripted::Reasoning("think ".into()),
            Scripted::Reasoning("more".into()),
            Scripted::Text("Hel".into()),
            Scripted::Text("lo".into()),
            Scripted::Reasoning("again".into()),
            Scripted::Usage(Usage {
                prompt_tokens: 7,
                completion_tokens: 3,
                cached_tokens: Some(1),
                reasoning_tokens: Some(2),
            }),
        ],
        vec![
            Scripted::Text("Calling.".into()),
            Scripted::ToolCall {
                name: "read_file".into(),
                arguments_json: r#"{"path":"a/ä.rs"}"#.into(),
                split_into: 3,
            },
            Scripted::ToolCall {
                name: "grep".into(),
                arguments_json: r#"{"pattern":"x"}"#.into(),
                split_into: 1,
            },
        ],
        vec![
            indexed(0, Some("c0"), Some("read_file"), r#"{"pa"#),
            indexed(1, Some("c1"), Some("grep"), r#"{"pattern""#),
            indexed(0, None, None, r#"th":"b"}"#),
            indexed(1, None, None, r#":"y"}"#),
        ],
        vec![
            Scripted::Text("trunc".into()),
            Scripted::Finish(FinishReason::Length),
        ],
        vec![Scripted::Finish(FinishReason::ContentFilter)],
        vec![
            Scripted::Text("odd".into()),
            Scripted::Finish(FinishReason::Other("custom".into())),
        ],
        framed(MalformedSse::CrLf),
        framed(MalformedSse::Comments),
        framed(MalformedSse::SplitUtf8),
        framed(MalformedSse::SplitData),
        framed(MalformedSse::MissingDone),
    ]
}

/// The fake model server renders one script for either protocol; both adapters must assemble the
/// same completion from it.
#[tokio::test]
async fn llm_c_024_scripted_streams_assemble_identically_on_both_protocols() -> TestResult {
    use tqa_testkit::{FakeModelServer, FakeSleeper, Script, Scripted};
    let clock = FakeClock::new();
    let serve = |scripts: Vec<Vec<Scripted>>| {
        FakeModelServer::start(
            scripts.into_iter().map(Script::new).collect(),
            Arc::new(FakeSleeper::new(clock.clone())),
        )
    };
    let scripts = shared_scripts();
    let count = scripts.len();
    let chat_server = serve(scripts.clone()).await?;
    let responses_server = serve(scripts).await?;
    let chat = fixtures::provider(fixtures::route(chat_server.base_url())?, &clock)?;
    let responses = fixtures::provider(responses_route(responses_server.base_url(), "")?, &clock)?;
    for index in 0..count {
        assert_eq!(
            collected(&responses).await?,
            collected(&chat).await?,
            "script {index}"
        );
    }
    assert!(
        responses_server
            .requests()
            .iter()
            .all(|request| request.path == "/v1/responses")
    );
    // A cut is a cut, and a raw unknown event is ignored.
    let server = serve(vec![
        vec![Scripted::Text("partial".into()), Scripted::CutAfter(2)],
        vec![
            Scripted::RawJson(r#"{"type":"response.future","x":1}"#.into()),
            Scripted::Text("kept".into()),
        ],
    ])
    .await?;
    let provider = fixtures::provider(responses_route(server.base_url(), "")?, &clock)?;
    let cut = collected(&provider).await;
    assert!(matches!(cut, Err(LlmError::StreamCut { .. })), "{cut:?}");
    assert_eq!(collected(&provider).await?.text, "kept");
    Ok(())
}
