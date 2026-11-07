//! LLM-C-004/LLM-C-026: the `anthropic_messages` adapter against recorded-shape SSE fixtures.
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
use sse_server::{SseServer, event, fixture_in};
use std::error::Error;
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    FinishReason, LlmError, Registry, Route, StreamAssembler, StreamChunk, StreamLimits, ToolCall,
    Usage,
};
use tqa_testkit::FakeClock;

type TestResult = Result<(), Box<dyn Error>>;

fn messages_route(base_url: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='test'\nprotocol='anthropic_messages'\nbase_url='{base_url}'\nmodel='fake-model'\nauth={{kind='none'}}\nmax_output=1024\n"
    ))?;
    Ok(registry.get("test").ok_or("missing route")?.clone())
}

/// Streams one body, split into pieces of `piece` characters, and returns its chunks.
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
    let provider = fixtures::provider(messages_route(&server.base_url)?, &FakeClock::new())?;
    let stream = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await?;
    let result = stream.collect::<Vec<_>>().await;
    assert_eq!(server.requests()[0].path, "/v1/messages");
    Ok(result)
}

async fn ok_chunks(name: &str) -> Result<Vec<StreamChunk>, Box<dyn Error>> {
    let body = fixture_in("messages", name)?;
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

fn usage(prompt: u64, completion: u64, cached: Option<u64>) -> StreamChunk {
    StreamChunk::Usage(Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        cached_tokens: cached,
        reasoning_tokens: None,
    })
}

#[tokio::test]
async fn llm_c_004_thinking_then_text_with_cache_usage() -> TestResult {
    let state = json!({"type":"thinking","thinking":"Check first.","signature":"c2lnLWE="});
    assert_eq!(
        ok_chunks("thinking_text.sse").await?,
        [
            StreamChunk::ReasoningDelta("Check ".into()),
            StreamChunk::ReasoningDelta("first.".into()),
            StreamChunk::ReasoningState(state.to_string()),
            StreamChunk::TextDelta("Hel".into()),
            StreamChunk::TextDelta("lo, wörld".into()),
            // The whole prompt: 20 uncached, 3 written to the cache and 5 read from it.
            usage(28, 15, Some(5)),
            StreamChunk::Finish(FinishReason::Stop),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_004_tool_use_blocks_are_emitted_whole_at_their_stop() -> TestResult {
    let chunks = ok_chunks("tools.sse").await?;
    let call = |index: u32, id: &str, name: &str, arguments: &str| StreamChunk::ToolCallDelta {
        index,
        id: Some(id.into()),
        name: Some(name.into()),
        arguments_delta: arguments.into(),
    };
    assert_eq!(
        chunks,
        [
            StreamChunk::TextDelta("Two lookups.".into()),
            call(0, "toolu_a", "read_file", r#"{"path":"src/lib.rs"}"#),
            call(1, "toolu_b", "grep", r#"{"pattern":"fn mäin"}"#),
            call(2, "toolu_c", "list_files", "{}"),
            usage(40, 22, None),
            StreamChunk::Finish(FinishReason::ToolCalls),
        ]
    );
    let mut assembler = StreamAssembler::new(StreamLimits::RECOMMENDED);
    for chunk in &chunks {
        assembler.push(chunk)?;
    }
    let calls = assembler.finish()?.tool_calls;
    assert_eq!(
        calls.first(),
        Some(&ToolCall {
            id: "toolu_a".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"src/lib.rs"}"#.into()
        })
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_026_unknown_events_blocks_and_fields_are_ignored() -> TestResult {
    let redacted = json!({"type":"redacted_thinking","data":"ZW5jcnlwdGVk"});
    assert_eq!(
        ok_chunks("unknown.sse").await?,
        [
            StreamChunk::ReasoningState(redacted.to_string()),
            StreamChunk::TextDelta("Still ".into()),
            StreamChunk::TextDelta("fine.".into()),
            usage(3, 2, None),
            StreamChunk::Finish(FinishReason::Other("future_reason".into())),
        ]
    );
    Ok(())
}

/// A minimal stream that ends with the given `message_delta`.
fn ending(delta: &serde_json::Value) -> String {
    [
        json!({"type":"message_start","message":{"usage":{"input_tokens":1,"output_tokens":0}}}),
        json!({"type":"message_delta","delta":delta,"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ]
    .iter()
    .map(event)
    .collect()
}

#[tokio::test]
async fn llm_c_004_stop_reasons_are_mapped() -> TestResult {
    for (reason, expected) in [
        (json!("end_turn"), FinishReason::Stop),
        (json!("stop_sequence"), FinishReason::Stop),
        (json!("tool_use"), FinishReason::ToolCalls),
        (json!("max_tokens"), FinishReason::Length),
        (json!("refusal"), FinishReason::ContentFilter),
        (
            json!("pause_turn"),
            FinishReason::Other("pause_turn".into()),
        ),
        (json!(null), FinishReason::Other("incomplete".into())),
    ] {
        let result: Vec<StreamChunk> = chunks(&ending(&json!({ "stop_reason": reason })), 7)
            .await?
            .into_iter()
            .collect::<Result<_, _>>()?;
        assert_eq!(
            result,
            [usage(1, 2, None), StreamChunk::Finish(expected)],
            "{reason}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn llm_c_026_a_thinking_block_without_a_signature_keeps_no_state() -> TestResult {
    let body: String = [
        json!({"type":"message_start","message":{"usage":{"input_tokens":1,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Hmm."}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ]
    .iter()
    .map(event)
    .collect();
    let result: Vec<StreamChunk> = chunks(&body, usize::MAX)
        .await?
        .into_iter()
        .collect::<Result<_, _>>()?;
    assert_eq!(
        result,
        [
            StreamChunk::ReasoningDelta("Hmm.".into()),
            usage(1, 2, None),
            StreamChunk::Finish(FinishReason::Stop),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_026_bytes_after_message_stop_are_not_read() -> TestResult {
    let body = format!(
        "{}event: content_block_delta\ndata: not json\n\n",
        fixture_in("messages", "thinking_text.sse")?
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

fn indexed(index: u32, id: Option<&str>, name: Option<&str>, delta: &str) -> tqa_testkit::Scripted {
    tqa_testkit::Scripted::ToolCallDelta {
        index,
        id: id.map(str::to_owned),
        name: name.map(str::to_owned),
        arguments_delta: delta.into(),
    }
}

/// Scripts covering every rendered shape: text and thinking runs, split and indexed calls, usage,
/// every non-stop finish and each framing variation.
fn scripts() -> Vec<Vec<tqa_testkit::Scripted>> {
    use tqa_testkit::{MalformedSse, Scripted};
    let framed = |variant| {
        vec![
            Scripted::Text("framed ✓".into()),
            Scripted::MalformedSse(variant),
        ]
    };
    let call = |name: &str, arguments: &str, split_into| Scripted::ToolCall {
        name: name.into(),
        arguments_json: arguments.into(),
        split_into,
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
                reasoning_tokens: None,
            }),
        ],
        vec![
            Scripted::Text("Calling.".into()),
            call("read_file", r#"{"path":"a/ä.rs"}"#, 3),
            call("grep", r#"{"pattern":"x"}"#, 1),
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
    ]
}

/// The thinking states the fake model renders for the first script: one per thinking run, each
/// with the synthetic signature of its block index.
fn first_script_states() -> [String; 2] {
    let thinking = |text: &str, signature: &str| {
        json!({"type":"thinking","thinking":text,"signature":signature}).to_string()
    };
    [thinking("think more", "sig-0"), thinking("again", "sig-2")]
}

async fn collected(
    provider: &std::sync::Arc<dyn tqa_llm::ModelProvider>,
) -> Result<tqa_llm::Completion, LlmError> {
    let stream = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await?;
    tqa_llm::collect(stream, StreamLimits::RECOMMENDED).await
}

/// The fake model server renders one script for Chat and for Messages; both adapters assemble the
/// same completion. Messages adds thinking states (with a synthetic signature) and reports no
/// reasoning-token count, so those two fields are compared separately.
#[tokio::test]
async fn llm_c_026_the_fake_model_serves_messages_streams_like_chat() -> TestResult {
    use tqa_testkit::{FakeModelServer, FakeSleeper, Script, Scripted};
    let clock = FakeClock::new();
    let serve = |scripts: Vec<Vec<Scripted>>| {
        FakeModelServer::start(
            scripts.into_iter().map(Script::new).collect(),
            std::sync::Arc::new(FakeSleeper::new(clock.clone())),
        )
    };
    let chat_server = serve(scripts()).await?;
    let messages_server = serve(scripts()).await?;
    let chat = fixtures::provider(fixtures::route(chat_server.base_url())?, &clock)?;
    let messages = fixtures::provider(messages_route(messages_server.base_url())?, &clock)?;
    for index in 0..scripts().len() {
        let mut expected = collected(&chat).await?;
        let mut actual = collected(&messages).await?;
        if index == 0 {
            assert_eq!(actual.reasoning_state, first_script_states());
        }
        actual.reasoning_state.clear();
        expected.reasoning_state.clear();
        assert_eq!(actual, expected, "script {index}");
    }
    let requests = messages_server.requests();
    assert!(
        requests
            .iter()
            .all(|request| request.path == "/v1/messages")
    );
    // A cut is a cut, and a raw unknown event is ignored.
    let server = serve(vec![
        vec![Scripted::Text("partial".into()), Scripted::CutAfter(2)],
        vec![
            Scripted::RawJson(r#"{"type":"future_event","x":1}"#.into()),
            Scripted::Text("kept".into()),
        ],
    ])
    .await?;
    let provider = fixtures::provider(messages_route(server.base_url())?, &clock)?;
    let cut = collected(&provider).await;
    assert!(matches!(cut, Err(LlmError::StreamCut { .. })), "{cut:?}");
    assert_eq!(collected(&provider).await?.text, "kept");
    Ok(())
}
