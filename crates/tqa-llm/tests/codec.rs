//! LLM-C-028: the public request encoder and response decoder are exactly what the providers use, on every protocol,
//! so a caller outside a sandbox (the route probe) sends and reads what a runtime would.
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
use sse_server::{SseServer, fixture_in};
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    HttpConfig, LlmError, Registry, Route, StreamChunk, StreamLimits, TokioSleeper,
    decode_response, encode_request,
};
use tqa_testkit::FakeClock;

type TestResult = Result<(), Box<dyn Error>>;

fn route(protocol: &str, base_url: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='t'\nprotocol='{protocol}'\nbase_url='{base_url}'\nmodel='m'\nauth={{kind='none'}}\n\
         max_output=1024\nstream_usage=true\n"
    ))?;
    Ok(registry.get("t").ok_or("missing route")?.clone())
}

/// A protocol, its recorded-shape response body, and the path its provider posts to.
struct Case {
    protocol: &'static str,
    body: String,
    path: &'static str,
}

fn cases() -> Result<Vec<Case>, Box<dyn Error>> {
    let case = |protocol, body, path| Case {
        protocol,
        body,
        path,
    };
    Ok(vec![
        case("openai_chat", chat_body(), "/v1/chat/completions"),
        case(
            "openai_responses",
            fixture_in("responses", "parallel_tools.sse")?,
            "/v1/responses",
        ),
        case(
            "anthropic_messages",
            fixture_in("messages", "tools.sse")?,
            "/v1/messages",
        ),
    ])
}

fn chat_body() -> String {
    [
        r#"{"choices":[{"index":0,"delta":{"content":"Hel"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a\"}"}}]}}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#,
        "[DONE]",
    ]
    .iter()
    .fold(String::new(), |mut body, data| {
        body.push_str("data: ");
        body.push_str(data);
        body.push_str("\n\n");
        body
    })
}

async fn chunks(stream: tqa_llm::CompletionStream) -> Result<Vec<StreamChunk>, LlmError> {
    stream.collect::<Vec<_>>().await.into_iter().collect()
}

#[tokio::test]
async fn llm_c_028_the_encoder_sends_exactly_the_providers_bytes() -> TestResult {
    let mut request = fixtures::request();
    request.tools.push(fixtures::tool());
    for Case {
        protocol,
        body,
        path,
    } in cases()?
    {
        let server = SseServer::start(vec![vec![body]]).await?;
        let route = route(protocol, &server.base_url)?;
        let provider = fixtures::provider(route.clone(), &FakeClock::new())?;
        drop(
            provider
                .complete(request.clone(), CancellationToken::new())
                .await?,
        );
        let recorded = server.requests().pop().ok_or("no request")?;
        assert_eq!(recorded.path, path);
        let encoded = encode_request(&route, &request)?;
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&encoded)?,
            recorded.body,
            "{protocol}"
        );
        assert_eq!(encoded.len(), recorded.bytes, "{protocol}");
    }
    Ok(())
}

#[tokio::test]
async fn llm_c_028_the_decoder_reads_exactly_the_providers_chunks() -> TestResult {
    for Case {
        protocol,
        body,
        path,
    } in cases()?
    {
        let server = SseServer::start(vec![vec![body.clone()], vec![body]]).await?;
        let route = route(protocol, &server.base_url)?;
        let provider = fixtures::provider(route.clone(), &FakeClock::new())?;
        let expected = chunks(
            provider
                .complete(fixtures::request(), CancellationToken::new())
                .await?,
        )
        .await?;
        let response = reqwest::Client::new()
            .post(format!(
                "{}{}",
                server.base_url.trim_end_matches("/v1"),
                path
            ))
            .body("{}")
            .send()
            .await?;
        let protocol = route.protocol;
        let decoded = decode_response(
            protocol,
            response,
            HttpConfig::new(StreamLimits::RECOMMENDED),
            Arc::new(TokioSleeper),
            CancellationToken::new(),
        );
        assert_eq!(chunks(decoded).await?, expected, "{protocol:?}");
        assert!(!expected.is_empty());
    }
    Ok(())
}
