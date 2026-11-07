//! GW-I-181: the gateway's relayed streams carry small writes at once. Without `TCP_NODELAY`, a small write waits for
//! the peer's ACK of the one before it, and a peer that delays its ACK (40 ms on Linux) holds the end of every other
//! relayed stream on a kept-alive connection that long. Each round times only the span the stall lengthens (the
//! reply's first byte to its end) against a baseline in the same round that can't stall: the same reply straight from
//! the upstream, which sets `TCP_NODELAY` (TK-I-007). A loaded host slows both; the test kit's paired rule fails three
//! or more of 20 rounds over their baseline by more than 20 ms (a full or partial stall) and passes two (hiccups).
//! A client's live event stream is not timed here: on loopback its back-to-back events arrive at once with or without
//! `TCP_NODELAY` (measured 2 Oct 2026), so a timing test can't show the defect; GW-U-119 checks its listener's sockets.
#[path = "support/fixture.rs"]
mod support;

use futures::StreamExt as _;
use secrecy::ExposeSecret as _;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration, time::Instant};
use support::TestResult;
use tqa_testkit::{FakeModelServer, Script, Scripted};

const ROUNDS: usize = 20;

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='nodelay'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nauth={{kind='none'}}\n",
        server.base_url()
    ))?)
}

// TEST: GW-I-181
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_181_sequential_relayed_streams_on_one_connection_never_stall(
    pool: PgPool,
) -> TestResult {
    let script = Script::new(
        (0..20)
            .map(|chunk| Scripted::Text(format!("chunk-{chunk:02} ")))
            .collect(),
    );
    // Each round takes two replies: one straight from the upstream (the baseline), one through the gateway.
    let server =
        FakeModelServer::start(vec![script; 2 * ROUNDS], Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let (owner, _) = f.admin().await?;
        let conversation = f.gateway.create_for_user(owner.id, "Fixture", "nodelay").await?;
        let token = f
            .gateway
            .runtime_tokens()
            .mint(&conversation.id, &"nodelay-sandbox".parse()?, std::net::Ipv4Addr::LOCALHOST.into())
            .await?;
        let url = f.url("/relay/r/nodelay/chat/completions");
        let direct_url = format!("{}/chat/completions", server.base_url());
        let direct = reqwest::Client::new();
        let body = serde_json::json!({"model":"m","stream":true,"messages":[{"role":"user","content":"hello"}]});
        let mut rounds = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let baseline = reply_span(direct.post(&direct_url).json(&body).send().await?).await?;
            let relayed =
                reply_span(f.client.post(&url).bearer_auth(token.expose_secret()).json(&body).send().await?).await?;
            rounds.push((relayed, baseline));
        }
        tqa_testkit::stall::check_paired("relayed streams", &rounds)?;
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

/// A streamed reply's first byte to its end: the span a stall at its end lengthens, without the request's handling.
async fn reply_span(response: reqwest::Response) -> Result<Duration, Box<dyn std::error::Error>> {
    let mut stream = response.error_for_status()?.bytes_stream();
    let mut first = None;
    while let Some(chunk) = stream.next().await {
        chunk?;
        first.get_or_insert_with(Instant::now);
    }
    Ok(first.map_or(Duration::ZERO, |first| first.elapsed()))
}
