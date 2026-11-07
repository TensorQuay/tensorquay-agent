//! PERF-003 (TEST-PLAN §15.1, Gate 9): relay latency. The same streaming request to a fake model server directly and
//! through the relay as a runtime sends it, in pairs whose order alternates. Targets: ≤ 10 ms p50 added per request;
//! ≤ 20 ms p50 added to the first token (p99 of both recorded).
#[path = "perf_relay/pairs.rs"]
mod pairs;
#[path = "support/fixture.rs"]
mod support;

use futures::StreamExt as _;
use pairs::{Path, Timing, added, content_deltas, order, summary, verdict};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::json;
use sqlx::PgPool;
use std::{io::Write as _, sync::Arc, time::Instant};
use support::TestResult;
use tqa_testkit::{FakeModelServer, Script, Scripted};

/// §15.1: 5 warm-up pairs, then 50 measured.
const WARMUPS: usize = 5;
const PAIRS: usize = 50;
/// The fixed reply: 20 content chunks, no delay, so the time measured is the paths', not the model's.
const CHUNKS: usize = 20;

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='perf'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nauth={{kind='none'}}\n",
        server.base_url()
    ))?)
}

/// One streaming request, timed from sending it to its first content chunk and to the end of its stream.
async fn timed(
    request: reqwest::RequestBuilder,
) -> Result<(Timing, Vec<String>), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let response = request.send().await?.error_for_status()?;
    let mut body = response.bytes_stream();
    let (mut text, mut first_token) = (String::new(), None);
    while let Some(bytes) = body.next().await {
        text.push_str(std::str::from_utf8(&bytes?)?);
        if first_token.is_none() && !content_deltas(&text).is_empty() {
            first_token = Some(start.elapsed());
        }
    }
    let total = start.elapsed();
    let first_token = first_token.ok_or("the stream carried no content")?;
    Ok((
        Timing {
            first_token_ns: i64::try_from(first_token.as_nanos())?,
            total_ns: i64::try_from(total.as_nanos())?,
        },
        content_deltas(&text),
    ))
}

#[sqlx::test]
#[ignore = "PERF-003: needs PostgreSQL (TQA_DATABASE_URL), --release and an idle host; run explicitly"]
async fn perf_003_relay_latency(pool: PgPool) -> TestResult {
    if cfg!(debug_assertions) {
        return Err("PERF-003 must run with --release".into());
    }
    let environment = tqa_testkit::perf::environment()?;
    let reply: Vec<String> = (0..CHUNKS)
        .map(|chunk| format!("chunk-{chunk:02} "))
        .collect();
    let script = Script::new(reply.iter().cloned().map(Scripted::Text).collect());
    let server = FakeModelServer::start(
        vec![script; 2 * (WARMUPS + PAIRS)],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let body = json!({"model":"m","stream":true,"messages":[{"role":"user","content":"hello"}]});
    let result = Box::pin(support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let (owner, _) = f.admin().await?;
        let conversation = f.gateway.create_for_user(owner.id, "Fixture", "perf").await?;
        let token: SecretString = f
            .gateway
            .runtime_tokens()
            .mint(&conversation.id, &"perf-relay-sandbox".parse()?, std::net::Ipv4Addr::LOCALHOST.into())
            .await?;
        let direct = format!("{}/chat/completions", server.base_url());
        let relay = f.url("/relay/r/perf/chat/completions");
        let mut pairs = Vec::new();
        for index in 0..WARMUPS + PAIRS {
            let (mut direct_timing, mut relay_timing) = (None, None);
            for path in order(index) {
                let (timing, content) = match path {
                    Path::Direct => timed(f.client.post(&direct).json(&body)).await?,
                    Path::Relay => timed(f.client.post(&relay).bearer_auth(token.expose_secret()).json(&body)).await?,
                };
                // The oracle: both paths carried the same reply.
                assert_eq!(content, reply, "pair {index}, {path:?}: the content differs from the model's reply");
                match path {
                    Path::Direct => direct_timing = Some(timing),
                    Path::Relay => relay_timing = Some(timing),
                }
            }
            if index >= WARMUPS {
                pairs.push((direct_timing.ok_or("no direct timing")?, relay_timing.ok_or("no relay timing")?));
            }
        }
        let (added_total_ns, added_first_token_ns) = added(&pairs);
        let (total_p50, total_p99) = summary(&added_total_ns)?;
        let (first_token_p50, first_token_p99) = summary(&added_first_token_ns)?;
        let samples = |pick: fn(&Timing) -> i64, side: fn(&(Timing, Timing)) -> &Timing| -> Vec<i64> {
            pairs.iter().map(|pair| pick(side(pair))).collect()
        };
        let failures: Vec<String> = verdict(total_p50, first_token_p50).err().into_iter().collect();
        let mut environment = environment.clone();
        environment["load_after"] = std::fs::read_to_string("/proc/loadavg").unwrap_or_default().trim().into();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = tqa_testkit::perf::write_result(&root, "PERF-003", &environment, &json!({
            "warmup_pairs": WARMUPS, "measured_pairs": PAIRS, "chunks_per_reply": CHUNKS,
            "added_total_ms": {"p50": total_p50, "p99": total_p99, "target_p50": pairs::ADDED_TOTAL_P50_MAX_MS},
            "added_first_token_ms": {"p50": first_token_p50, "p99": first_token_p99,
                "target_p50": pairs::ADDED_FIRST_TOKEN_P50_MAX_MS},
            "samples_ns": {
                "direct_total": samples(|t| t.total_ns, |p| &p.0), "relay_total": samples(|t| t.total_ns, |p| &p.1),
                "direct_first_token": samples(|t| t.first_token_ns, |p| &p.0),
                "relay_first_token": samples(|t| t.first_token_ns, |p| &p.1),
            },
            "added": "relay minus direct per pair, order alternating; one client, one monotonic clock; warm-ups excluded",
        }), &failures)?;
        writeln!(
            std::io::stdout().lock(),
            "PERF-003 added p50/p99: total {total_p50:.3}/{total_p99:.3} ms, first token \
             {first_token_p50:.3}/{first_token_p99:.3} ms; {}",
            path.display()
        )?;
        failures.first().map_or(Ok(()), |failure| Err(failure.clone().into()))
    }))
    .await;
    server.shutdown().await?;
    result
}
