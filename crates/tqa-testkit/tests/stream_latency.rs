//! TK-I-007: the fake model server answers a streamed reply without a TCP stall. Without `TCP_NODELAY`, a server's small
//! last write waits for the client's ACK of the one before it, and a client that delays its ACK (40 ms on Linux) holds
//! the end of every other reply on a kept-alive connection by that much: a latency benchmark against the server
//! (PERF-003) then measures the stall, not the code under test.
use futures::StreamExt as _;
use std::{sync::Arc, time::Instant};
use tqa_testkit::{FakeModelServer, Script, Scripted};

const REQUESTS: usize = 20;

// TEST: TK-I-007
#[tokio::test]
async fn tk_i_007_sequential_streamed_replies_on_one_connection_never_stall()
-> Result<(), Box<dyn std::error::Error>> {
    let script = Script::new(
        (0..20)
            .map(|chunk| Scripted::Text(format!("chunk-{chunk:02} ")))
            .collect(),
    );
    let server =
        FakeModelServer::start(vec![script; REQUESTS], Arc::new(tqa_llm::TokioSleeper)).await?;
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", server.base_url());
    let body = serde_json::json!({"model":"m","stream":true,"messages":[{"role":"user","content":"hello"}]});
    let mut totals = Vec::with_capacity(REQUESTS);
    for _ in 0..REQUESTS {
        let started = Instant::now();
        let mut stream = client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .bytes_stream();
        while let Some(chunk) = stream.next().await {
            chunk?;
        }
        totals.push(started.elapsed());
    }
    server.shutdown().await?;
    // The rule (`tqa_testkit::stall`): three or more replies over 20 ms fail; two are scheduling hiccups.
    tqa_testkit::stall::check("replies", &totals)?;
    Ok(())
}
