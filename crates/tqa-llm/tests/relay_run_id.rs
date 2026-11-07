//! LLM-U-049: a call's run goes to the gateway's relay as `X-TQA-Run-ID`, so the relay's usage meter and secret-read
//! audit name the run. A provider never learns it, and a request without a run sends no header.
#[allow(
    dead_code,
    reason = "This suite uses only the server and its fixtures."
)]
#[path = "fixtures/sse_server.rs"]
mod sse_server;

use futures::StreamExt as _;
use sse_server::{SseServer, fixture_in};
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    CompletionRequest, ContentPart, HttpConfig, Message, Peer, RUN_HEADER, Registry, Role,
    StreamLimits, provider_for,
};
use tqa_testkit::{FakeClock, FakeSleeper};
use tqa_types::RunId;

type TestResult = Result<(), Box<dyn Error>>;

const CHAT: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n\
    data: [DONE]\n\n";

/// The request head one call sends, lowercased, for this protocol, peer and run.
async fn head(protocol: &str, peer: Peer, run: Option<&str>) -> Result<String, Box<dyn Error>> {
    let body = match protocol {
        "openai_chat" => CHAT.to_owned(),
        "openai_responses" => fixture_in("responses", "text.sse")?,
        _ => fixture_in("messages", "thinking_text.sse")?,
    };
    let server = SseServer::start(vec![vec![body]]).await?;
    let max_output = if protocol == "anthropic_messages" {
        "max_output=1024\n"
    } else {
        ""
    };
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='r'\nprotocol='{protocol}'\nbase_url='{}'\nmodel='m'\nauth={{kind='none'}}\n{max_output}",
        server.base_url
    ))?;
    let route = registry.get("r").ok_or("missing route")?.clone();
    let clock = FakeClock::new();
    let provider = provider_for(
        route,
        None,
        HttpConfig {
            peer,
            ..HttpConfig::new(StreamLimits::RECOMMENDED)
        },
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?;
    let request = CompletionRequest {
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentPart::Text("Hello".into())],
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }],
        run_id: run.map(str::parse::<RunId>).transpose()?,
        ..CompletionRequest::default()
    };
    provider
        .complete(request, CancellationToken::new())
        .await?
        .collect::<Vec<_>>()
        .await;
    Ok(server.requests().pop().ok_or("no request")?.head)
}

// TEST: LLM-U-049
#[tokio::test]
async fn llm_u_049_the_relay_learns_the_run_and_a_provider_never_does() -> TestResult {
    assert_eq!(RUN_HEADER, "x-tqa-run-id");
    for protocol in ["openai_chat", "openai_responses", "anthropic_messages"] {
        let relayed = head(protocol, Peer::Relay, Some("run-7f3a_01")).await?;
        assert_eq!(
            relayed.matches("x-tqa-run-id:").count(),
            1,
            "{protocol}: {relayed}"
        );
        assert!(
            relayed.contains("x-tqa-run-id: run-7f3a_01\r\n"),
            "{protocol}: {relayed}"
        );
        for (peer, run) in [(Peer::Provider, Some("run-7f3a_01")), (Peer::Relay, None)] {
            let head = head(protocol, peer, run).await?;
            assert!(
                !head.contains("x-tqa-run-id"),
                "{protocol} {peer:?} {run:?}: {head}"
            );
        }
    }
    Ok(())
}
