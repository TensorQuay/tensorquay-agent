//! LLM-E-028: `anthropic_messages` HTTP statuses, in-stream errors, malformed streams, cuts and limits.
#[allow(
    dead_code,
    reason = "This suite uses only some of the shared provider fixtures."
)]
#[path = "fixtures/http.rs"]
mod fixtures;
#[allow(
    dead_code,
    reason = "This suite does not inspect the recorded requests."
)]
#[path = "fixtures/sse_server.rs"]
mod sse_server;

use futures::StreamExt as _;
use serde_json::{Value, json};
use sse_server::{SseServer, event, fixture_in};
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    HttpConfig, LlmError, ModelProvider, Registry, Route, StreamChunk, StreamLimitKind,
    StreamLimits, provider_for,
};
use tqa_testkit::{FakeClock, FakeSleeper};

type TestResult = Result<(), Box<dyn Error>>;

fn route(base_url: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='test'\nprotocol='anthropic_messages'\nbase_url='{base_url}'\nmodel='fake-model'\nauth={{kind='none'}}\nmax_output=1024\n"
    ))?;
    Ok(registry.get("test").ok_or("missing route")?.clone())
}

fn provider(
    base_url: &str,
    limits: StreamLimits,
) -> Result<Arc<dyn ModelProvider>, Box<dyn Error>> {
    let clock = FakeClock::new();
    Ok(provider_for(
        route(base_url)?,
        None,
        HttpConfig::new(limits),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?)
}

/// The stream's chunks up to and including its first error.
async fn run(
    body: String,
    limits: StreamLimits,
) -> Result<(Vec<StreamChunk>, Option<LlmError>), Box<dyn Error>> {
    let server = SseServer::start(vec![vec![body]]).await?;
    let provider = provider(&server.base_url, limits)?;
    let mut stream = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await?;
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => chunks.push(chunk),
            Err(error) => return Ok((chunks, Some(error))),
        }
    }
    Ok((chunks, None))
}

fn body(events: &[Value]) -> String {
    events.iter().map(event).collect()
}
fn start() -> Value {
    json!({"type":"message_start","message":{"usage":{"input_tokens":1,"output_tokens":0}}})
}
fn open(index: u64, block: &Value) -> Value {
    json!({"type":"content_block_start","index":index,"content_block":block})
}
fn delta(index: u64, delta: &Value) -> Value {
    json!({"type":"content_block_delta","index":index,"delta":delta})
}
fn tool(id: &str) -> Value {
    json!({"type":"tool_use","id":id,"name":"read_file","input":{}})
}

#[tokio::test]
async fn llm_e_028_in_stream_errors_map_by_type_and_never_keep_the_message() -> TestResult {
    let secret = "prompt text that must not leak";
    for (kind, expected) in [
        (json!("rate_limit_error"), "rate_limited"),
        (json!("overloaded_error"), "unavailable"),
        (json!("api_error"), "unavailable"),
        (
            json!("invalid_request_error"),
            "model error: invalid_request_error",
        ),
        (json!("billing_error"), "model error: billing_error"),
        (json!("bad type!"), "model error: unknown"),
        (json!(7), "model error: unknown"),
    ] {
        let error = json!({"type":"error","error":{"type":kind,"message":secret}});
        let (_, error) = run(body(&[start(), error]), StreamLimits::RECOMMENDED).await?;
        let error = error.ok_or("no error")?;
        let text = format!("{error} {} {error:?}", error.code());
        assert!(text.contains(expected), "{kind}: {text}");
        assert!(!text.contains(secret), "{kind}");
        let retryable = matches!(expected, "rate_limited" | "unavailable");
        assert_eq!(error.retryable(), retryable, "{kind}");
    }
    Ok(())
}

#[tokio::test]
async fn llm_e_028_a_cut_in_the_middle_of_a_call_emits_no_part_of_it() -> TestResult {
    let text = json!({"type":"text","text":"Reading."});
    let events = [
        start(),
        open(0, &text),
        json!({"type":"content_block_stop","index":0}),
        open(1, &tool("toolu_a")),
        delta(
            1,
            &json!({"type":"input_json_delta","partial_json":"{\"path\":"}),
        ),
    ];
    let (chunks, error) = run(body(&events), StreamLimits::RECOMMENDED).await?;
    let Some(LlmError::StreamCut { partial }) = error else {
        return Err(format!("expected a cut, got {error:?}").into());
    };
    assert_eq!(chunks, [StreamChunk::TextDelta("Reading.".into())]);
    assert!(partial.tool_calls.is_empty());
    assert_eq!(partial.text, "Reading.");
    Ok(())
}

#[tokio::test]
async fn llm_e_028_malformed_streams_are_protocol_errors() -> TestResult {
    let text = json!({"type":"text","text":""});
    let stop = |index: u64| json!({"type":"content_block_stop","index":index});
    let end = json!({"type":"message_stop"});
    let cases: [(Vec<Value>, &str); 8] = [
        (
            vec![start(), delta(4, &json!({"type":"text_delta","text":"x"}))],
            "unknown content block",
        ),
        (
            vec![
                start(),
                open(0, &tool("t")),
                delta(0, &json!({"type":"text_delta","text":"x"})),
            ],
            "does not match",
        ),
        (
            vec![start(), open(0, &text), open(0, &text)],
            "duplicate content block",
        ),
        (vec![start(), stop(3)], "unknown content block"),
        (vec![start(), open(0, &text), end.clone()], "unfinished"),
        (vec![start(), open(0, &tool(""))], "identifier or name"),
        (
            vec![start(), open(0, &json!({"type":"text","text":5}))],
            "not a string",
        ),
        (vec![json!({"no":"type"})], "no type"),
    ];
    for (events, expected) in cases {
        let (_, error) = run(body(&events), StreamLimits::RECOMMENDED).await?;
        let error = error.ok_or(expected)?;
        assert!(
            matches!(&error, LlmError::Malformed(text) if text.contains(expected)),
            "{expected}: {error}"
        );
    }
    let (_, error) = run("data: not json\n\n".into(), StreamLimits::RECOMMENDED).await?;
    assert!(
        matches!(error, Some(LlmError::Malformed(text)) if text.contains("invalid Messages event"))
    );
    Ok(())
}

#[tokio::test]
async fn llm_e_028_buffered_calls_and_thinking_count_toward_the_stream_limits() -> TestResult {
    let calls = StreamLimits {
        max_tool_calls: 2,
        ..StreamLimits::RECOMMENDED
    };
    let (_, error) = run(fixture_in("messages", "tools.sse")?, calls).await?;
    assert!(matches!(
        error,
        Some(LlmError::StreamLimit {
            which: StreamLimitKind::ToolCalls,
            max: 2
        })
    ));
    let bytes = StreamLimits {
        max_completion_bytes: 8,
        ..StreamLimits::RECOMMENDED
    };
    for name in ["tools.sse", "thinking_text.sse", "unknown.sse"] {
        let (_, error) = run(fixture_in("messages", name)?, bytes).await?;
        assert!(
            matches!(
                error,
                Some(LlmError::StreamLimit {
                    which: StreamLimitKind::Completion,
                    max: 8
                })
            ),
            "{name}: {error:?}"
        );
    }
    Ok(())
}

/// A complete HTTP error reply, written verbatim by the test server.
fn status(code: u16, retry_after: Option<&str>) -> String {
    let header = retry_after.map_or_else(String::new, |value| format!("Retry-After: {value}\r\n"));
    let body = r#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
    format!(
        "HTTP/1.1 {code} Status\r\nContent-Type: application/json\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn llm_e_028_http_statuses_keep_the_chat_classification_and_retry_after() -> TestResult {
    for (code, retry_after) in [
        (400, None),
        (401, None),
        (429, Some("90")),
        (500, None),
        (503, Some("30")),
        (503, None),
        (529, None),
    ] {
        let server = SseServer::start(vec![vec![status(code, retry_after)]]).await?;
        let error = provider(&server.base_url, StreamLimits::RECOMMENDED)?
            .complete(fixtures::request(), CancellationToken::new())
            .await
            .err()
            .ok_or("accepted an HTTP error")?;
        assert_eq!(
            error.retryable(),
            !matches!(code, 400 | 401),
            "{code}: {error}"
        );
        let delay = |seconds| Some(std::time::Duration::from_secs(seconds));
        match (code, retry_after) {
            (429, _) => assert!(
                matches!(error, LlmError::RateLimited { retry_after } if retry_after == delay(90))
            ),
            (503, Some(_)) => assert!(
                matches!(error, LlmError::Unavailable { retry_after } if retry_after == delay(30))
            ),
            _ => assert!(
                matches!(error, LlmError::Http { status, .. } if status == code),
                "{code}"
            ),
        }
    }
    Ok(())
}

#[tokio::test]
async fn llm_e_028_a_refused_connection_is_retryable() -> TestResult {
    // The port stays bound but never listens, so a connection is refused and no other test's server can take it: a
    // dropped listener freed the port for another test's server, which this test then reached (a cross-test race).
    let reserved = tokio::net::TcpSocket::new_v4()?;
    reserved.bind("127.0.0.1:0".parse()?)?;
    let base_url = format!("http://{}/v1", reserved.local_addr()?);
    let error = provider(&base_url, StreamLimits::RECOMMENDED)?
        .complete(fixtures::request(), CancellationToken::new())
        .await
        .err()
        .ok_or("connected to a closed port")?;
    assert!(error.retryable(), "{error}");
    Ok(())
}
