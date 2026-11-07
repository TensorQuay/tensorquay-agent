//! LLM-E-019: `openai_responses` HTTP errors, in-stream errors and cut streams.
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
use serde_json::json;
use sse_server::{SseServer, event, fixture};
use std::{
    error::Error,
    fmt::{Debug, Write as _},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    HttpConfig, LlmError, Registry, Route, StreamChunk, StreamLimitKind, StreamLimits, provider_for,
};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};

type TestResult = Result<(), Box<dyn Error>>;

fn route(base_url: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='test'\nprotocol='openai_responses'\nbase_url='{base_url}'\nmodel='fake-model'\nauth={{kind='none'}}\n"
    ))?;
    Ok(registry.get("test").ok_or("missing route")?.clone())
}

/// The stream's chunks up to and including its first error.
async fn run(
    body: String,
    limits: StreamLimits,
) -> Result<(Vec<StreamChunk>, Option<LlmError>), Box<dyn Error>> {
    let server = SseServer::start(vec![vec![body]]).await?;
    let clock = FakeClock::new();
    let provider = provider_for(
        route(&server.base_url)?,
        None,
        HttpConfig::new(limits),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?;
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

#[tokio::test]
async fn llm_e_019_http_statuses_keep_the_chat_classification_and_retry_after() -> TestResult {
    let cases = [
        (400, None),
        (408, None),
        (429, Some("90")),
        (500, None),
        (502, None),
        (503, Some("30")),
        (503, None),
        (529, None),
    ];
    let scripts = cases
        .iter()
        .map(|(code, retry_after)| {
            Script::new(vec![Scripted::Status {
                code: *code,
                body: "busy".into(),
                retry_after: retry_after.map(str::to_owned),
            }])
        })
        .collect();
    let clock = FakeClock::new();
    let server = FakeModelServer::start(scripts, Arc::new(FakeSleeper::new(clock.clone()))).await?;
    let provider = fixtures::provider(route(server.base_url())?, &clock)?;
    for (status, retry_after) in cases {
        let error = provider
            .complete(fixtures::request(), CancellationToken::new())
            .await
            .err()
            .ok_or("accepted an HTTP error")?;
        assert_eq!(error.retryable(), status != 400, "{status}: {error}");
        match (status, retry_after) {
            (429, _) => assert!(matches!(
                error,
                LlmError::RateLimited { retry_after: Some(delay) } if delay == Duration::from_secs(90)
            )),
            (503, Some(_)) => assert!(matches!(
                error,
                LlmError::Unavailable { retry_after: Some(delay) } if delay == Duration::from_secs(30)
            )),
            _ => assert!(matches!(error, LlmError::Http { status: code, .. } if code == status)),
        }
    }
    assert!(
        server
            .requests()
            .iter()
            .all(|request| request.path == "/v1/responses")
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_e_019_a_refused_connection_is_retryable() -> TestResult {
    // Bound but never listening: refused, and no other test's server can take the port (see messages_errors.rs).
    let reserved = tokio::net::TcpSocket::new_v4()?;
    reserved.bind("127.0.0.1:0".parse()?)?;
    let base = format!("http://{}/v1", reserved.local_addr()?);
    let provider = fixtures::provider(route(&base)?, &FakeClock::new())?;
    let error = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await
        .err()
        .ok_or("connected to a closed port")?;
    assert!(matches!(error, LlmError::Connection(_)), "{error}");
    assert!(error.retryable());
    Ok(())
}

const ECHO: &str = "SECRET-PROMPT-ECHO";

fn error_event(code: &str) -> String {
    event(
        &json!({"type":"error","code":code,"message":format!("Your prompt was: {ECHO}"),"param":null}),
    )
}
fn failed_event(code: &str) -> String {
    event(
        &json!({"type":"response.failed","response":{"status":"failed",
        "error":{"code":code,"message":format!("Your prompt was: {ECHO}")}}}),
    )
}

/// A text delta, then the error event: the text arrives, then exactly the expected error.
async fn check_stream_error(body: String, code: &str, expected: &LlmError) -> TestResult {
    let prefix = event(&json!({"type":"response.output_text.delta",
        "item_id":"m","output_index":0,"content_index":0,"delta":"partial"}));
    let (chunks, error) = run(prefix + &body, StreamLimits::RECOMMENDED).await?;
    assert_eq!(chunks, [StreamChunk::TextDelta("partial".into())]);
    let error = error.ok_or("an error event was ignored")?;
    assert_eq!(format!("{error:?}"), format!("{expected:?}"), "{code}");
    assert!(!format!("{error} {error:?}").contains(ECHO));
    Ok(())
}

struct Recorder(Arc<Mutex<String>>);
struct Visitor<'a>(&'a mut String);
impl tracing::field::Visit for Visitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn Debug) {
        assert!(writeln!(self.0, "{field}={value:?}").is_ok());
    }
}
impl tracing::Subscriber for Recorder {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        event.record(&mut Visitor(
            &mut self.0.lock().unwrap_or_else(PoisonError::into_inner),
        ));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn llm_e_019_in_stream_errors_map_by_code_and_never_keep_the_message() -> TestResult {
    let output = Arc::new(Mutex::new(String::new()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let cases = [
        ("server_error", LlmError::Unavailable { retry_after: None }),
        (
            "internal_error",
            LlmError::Unavailable { retry_after: None },
        ),
        ("overloaded", LlmError::Unavailable { retry_after: None }),
        (
            "rate_limit_exceeded",
            LlmError::RateLimited { retry_after: None },
        ),
        (
            "insufficient_quota",
            LlmError::Protocol("model error: insufficient_quota".into()),
        ),
        (
            "invalid_prompt",
            LlmError::Protocol("model error: invalid_prompt".into()),
        ),
        (
            "not a code!",
            LlmError::Protocol("model error: unknown".into()),
        ),
    ];
    tracing::subscriber::with_default(Recorder(output.clone()), || {
        runtime.block_on(async {
            let bodies = cases.iter().flat_map(|(code, expected)| {
                [error_event(code), failed_event(code)].map(|body| (body, *code, expected))
            });
            for (body, code, expected) in bodies {
                check_stream_error(body, code, expected).await?;
            }
            Ok::<_, Box<dyn Error>>(())
        })
    })?;
    assert!(
        !cases[4].1.retryable(),
        "account errors must not switch routes"
    );
    let logged = output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert!(!logged.contains(ECHO), "{logged}");
    Ok(())
}

/// The parallel-call fixture cut after `events` data records.
fn cut(events: usize) -> Result<String, Box<dyn Error>> {
    let body = fixture("parallel_tools.sse")?;
    Ok(body.split_inclusive("\n\n").take(events).collect())
}

#[tokio::test]
async fn llm_e_019_a_cut_in_the_middle_of_a_call_emits_no_part_of_it() -> TestResult {
    // Both calls opened and received argument fragments, but neither finished.
    let (chunks, error) = run(cut(10)?, StreamLimits::RECOMMENDED).await?;
    assert_eq!(chunks, [StreamChunk::ReasoningDelta("Two lookups.".into())]);
    let Some(LlmError::StreamCut { partial }) = error else {
        return Err(format!("expected a cut, got {error:?}").into());
    };
    assert!(partial.tool_calls.is_empty(), "{partial:?}");
    assert_eq!(partial.reasoning, "Two lookups.");
    // One call finished before the cut: only that call is kept.
    let (_, error) = run(cut(12)?, StreamLimits::RECOMMENDED).await?;
    let Some(LlmError::StreamCut { partial }) = error else {
        return Err("expected a cut".into());
    };
    let ids: Vec<_> = partial
        .tool_calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    assert_eq!(ids, ["call_b"]);
    Ok(())
}

#[tokio::test]
async fn llm_e_019_a_stream_without_a_terminal_event_is_cut() -> TestResult {
    let text = fixture("text.sse")?;
    let without_terminal: String = text
        .split_inclusive("\n\n")
        .filter(|record| !record.contains("response.completed"))
        .collect();
    let (chunks, error) = run(without_terminal, StreamLimits::RECOMMENDED).await?;
    assert_eq!(chunks.len(), 2);
    let Some(LlmError::StreamCut { partial }) = error else {
        return Err(format!("expected a cut, got {error:?}").into());
    };
    assert_eq!(partial.text, "Hello, wörld");
    Ok(())
}

#[tokio::test]
async fn llm_e_019_inconsistent_calls_are_protocol_errors() -> TestResult {
    let added = |index: u32, id: &str| {
        event(
            &json!({"type":"response.output_item.added","output_index":index,
            "item":{"type":"function_call","id":id,"call_id":format!("call_{id}"),"name":"grep","arguments":""}}),
        )
    };
    let delta = |index: u32, text: &str| {
        event(
            &json!({"type":"response.function_call_arguments.delta","item_id":"x","output_index":index,"delta":text}),
        )
    };
    let done = |index: u32, arguments: &str| {
        event(
            &json!({"type":"response.function_call_arguments.done","item_id":"x","output_index":index,"arguments":arguments}),
        )
    };
    let cases = [
        (
            added(0, "a") + &delta(0, "{\"a\":1}") + &done(0, "{\"a\":2}"),
            "do not match",
        ),
        (delta(3, "{}"), "unknown call"),
        (done(3, "{}"), "unknown call"),
        (
            event(
                &json!({"type":"response.output_item.added","output_index":0,
                "item":{"type":"function_call","id":"a","arguments":""}}),
            ) + &delta(0, "{}")
                + &done(0, "{}"),
            "identifier or name",
        ),
        (
            added(0, "a")
                + &delta(0, "{}")
                + &event(
                    &json!({"type":"response.completed","response":{"status":"completed",
                    "usage":{"input_tokens":1,"output_tokens":1}}}),
                ),
            "unfinished",
        ),
        (
            event(&json!({"type":"response.output_text.delta"})),
            "invalid",
        ),
        (added(0, "a") + &added(0, "b"), "duplicate call"),
        ("data: not json\n\n".to_owned(), "invalid Responses event"),
        ("data: {\"no\":\"type\"}\n\n".to_owned(), "no type"),
    ];
    for (body, expected) in cases {
        let (_, error) = run(body, StreamLimits::RECOMMENDED).await?;
        assert!(
            matches!(&error, Some(LlmError::Malformed(text)) if text.contains(expected)),
            "{expected}: {error:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn llm_e_019_buffered_arguments_count_toward_the_stream_limits() -> TestResult {
    let limits = StreamLimits {
        max_completion_bytes: 16,
        max_tool_calls: 1,
        ..StreamLimits::RECOMMENDED
    };
    let added = |index: u32| {
        event(
            &json!({"type":"response.output_item.added","output_index":index,
            "item":{"type":"function_call","id":"f","call_id":"c","name":"n","arguments":""}}),
        )
    };
    let delta = event(
        &json!({"type":"response.function_call_arguments.delta","item_id":"f",
        "output_index":0,"delta":"x".repeat(17)}),
    );
    let (_, error) = run(added(0) + &delta, limits).await?;
    assert!(matches!(
        error,
        Some(LlmError::StreamLimit {
            which: StreamLimitKind::Completion,
            ..
        })
    ));
    let (_, error) = run(added(0) + &added(1), limits).await?;
    assert!(matches!(
        error,
        Some(LlmError::StreamLimit {
            which: StreamLimitKind::ToolCalls,
            ..
        })
    ));
    Ok(())
}
