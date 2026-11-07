//! PERF-002 (TEST-PLAN §15.1, Gate 9): event pipeline throughput. The runtime sends 10,000 events a second through the
//! real link; the gateway stores them and streams them to a client. No loss; lag ≤ 100 ms p99.
#[path = "perf_event_pipeline/ledger.rs"]
mod ledger;
#[path = "support/fixture.rs"]
mod support;

use ledger::{Pacing, ROW_RATE, Receipts, burst_in_time, lag_summary, rate};
use serde_json::json;
use sqlx::PgPool;
use std::{io::Write as _, sync::Mutex, time::Instant};
use support::{TestResult, run};

/// The TEST-PLAN row's lag target (PERF-002): ≤ 100 ms p99, at its 10k events a second (`ledger::ROW_RATE`) or a
/// recorded variant's rate (`TQA_PERF_002_RATE`).
const LAG_P99_MAX_MS: f64 = 100.0;
/// §15.1: 5 warm-up iterations, then 50. An iteration is a one-second burst at the rate.
const WARMUPS: u64 = 5;
const BURSTS: u64 = 50;

#[sqlx::test]
#[ignore = "PERF-002: needs PostgreSQL (TQA_DATABASE_URL), --release and an idle host; run explicitly"]
async fn perf_002_event_pipeline_throughput(pool: PgPool) -> TestResult {
    if cfg!(debug_assertions) {
        return Err("PERF-002 must run with --release".into());
    }
    let environment = tqa_testkit::perf::environment()?;
    let rate = rate(std::env::var("TQA_PERF_002_RATE").ok().as_deref())?;
    let burst_events = rate;
    Box::pin(run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let created = f.conversation(owner.id).await?;
        let mut runtime = f.runtime(&created).await?;
        let mut stream = f.stream(&created.id, &cookie, 0).await?;
        let total = (WARMUPS + BURSTS) * burst_events;
        let pacing = Pacing::per_second(rate);
        let sent_at: Mutex<Vec<Option<Instant>>> = Mutex::new(vec![None; usize::try_from(total)?]);
        let stamp = |sequence: u64, at: Instant| -> TestResult {
            let mut table = sent_at.lock().map_err(|_| "send table poisoned")?;
            *table.get_mut(usize::try_from(sequence - 1)?).ok_or("sequence out of range")? = Some(at);
            Ok(())
        };
        let sender = async {
            let mut late = Vec::new();
            let mut send_secs = Vec::new();
            for burst in 0..WARMUPS + BURSTS {
                let start = Instant::now();
                for index in 0..burst_events {
                    let due = start + pacing.due(index);
                    if due > Instant::now() {
                        tokio::time::sleep_until(due.into()).await;
                    }
                    let sequence = burst * burst_events + index + 1;
                    stamp(sequence, Instant::now())?;
                    runtime.send_live(&support::frame(sequence, "perf")).await?;
                }
                let elapsed = start.elapsed();
                send_secs.push(elapsed.as_secs_f64());
                if let Err(reason) = burst_in_time(elapsed, burst_events, pacing.span(burst_events)) {
                    late.push(format!("burst {burst}: {reason}"));
                }
            }
            Ok::<_, Box<dyn std::error::Error>>((late, send_secs))
        };
        let reader = async {
            let mut receipts = Receipts::default();
            let mut lags_ns = Vec::new();
            while receipts.last() < total {
                let (sequence, _) = stream.next().await?;
                let Some(sequence) = sequence else { continue };
                let received = Instant::now();
                let sequence: u64 = sequence.to_string().parse()?;
                receipts.record(sequence)?;
                if sequence > WARMUPS * burst_events {
                    let sent = sent_at.lock().map_err(|_| "send table poisoned")?[usize::try_from(sequence - 1)?]
                        .ok_or("received before it was sent")?;
                    lags_ns.push(u64::try_from(received.duration_since(sent).as_nanos())?);
                }
            }
            receipts.complete(total)?;
            Ok::<_, Box<dyn std::error::Error>>(lags_ns)
        };
        let (sent, received) = tokio::join!(sender, reader);
        let ((late, send_secs), lags_ns) = (sent?, received?);
        // No loss in the store, and a client reading from 0 replays every event.
        let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM events WHERE conversation_id = $1")
            .bind(uuid::Uuid::parse_str(created.id.as_str())?)
            .fetch_one(&f.pool)
            .await?;
        let mut replay = f.stream(&created.id, &cookie, 0).await?;
        let mut replayed = Receipts::default();
        while replayed.last() < total {
            if let (Some(sequence), _) = replay.next().await? {
                replayed.record(sequence.to_string().parse()?)?;
            }
        }
        let (p50_ms, p99_ms) = lag_summary(&lags_ns)?;
        let burst_p99_ms = lags_ns
            .chunks(usize::try_from(burst_events)?)
            .map(|chunk| lag_summary(chunk).map(|(_, p99)| p99))
            .collect::<Result<Vec<_>, _>>()?;
        let mut failures: Vec<String> = late.iter().map(|burst| format!("the rate wasn't offered: {burst}")).collect();
        if stored != i64::try_from(total)? {
            failures.push(format!("{stored} of {total} events were stored"));
        }
        if p99_ms > LAG_P99_MAX_MS {
            failures.push(format!("lag p99 {p99_ms:.3} ms exceeds {LAG_P99_MAX_MS} ms"));
        }
        let mut environment = environment.clone();
        environment["load_after"] = std::fs::read_to_string("/proc/loadavg").unwrap_or_default().trim().into();
        // Whether a commit waits for the disk decides the lag, so the result names the database's durability.
        let settings: Vec<(String, String)> = sqlx::query_as(
            "SELECT name, setting FROM pg_settings WHERE name IN \
             ('fsync', 'synchronous_commit', 'full_page_writes', 'server_version') ORDER BY name",
        )
        .fetch_all(&f.pool)
        .await?;
        environment["database"] = settings
            .into_iter()
            .map(|(name, setting)| (name, setting.into()))
            .collect::<serde_json::Map<_, _>>()
            .into();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let utc = environment["utc"].as_str().ok_or("the environment's utc is not text")?;
        let raw = tqa_testkit::perf::write_raw(&root, "PERF-002", utc, "lag", &json!({
            "lag": "client receive minus runtime send per event, in receive order, nanoseconds; warm-ups excluded",
            "first_sequence": WARMUPS * burst_events + 1, "lags_ns": lags_ns,
        }))?;
        let path = tqa_testkit::perf::write_result(&root, "PERF-002", &environment, &json!({
            "rate_per_second": rate, "variant": if rate == ROW_RATE { "row" } else { "rate" }, "burst_events": burst_events, "warmup_bursts": WARMUPS, "measured_bursts": BURSTS,
            "events_sent": total, "events_stored": stored, "events_replayed": replayed.last(),
            "lag_ms": {"p50": p50_ms, "p99": p99_ms, "per_burst_p99": burst_p99_ms, "target_p99": LAG_P99_MAX_MS},
            "send_seconds_per_burst": send_secs, "late_bursts": late,
            "lag": "client receive minus runtime send, one monotonic clock; warm-up bursts excluded",
            "raw_files": [raw],
        }), &failures)?;
        writeln!(
            std::io::stdout().lock(),
            "PERF-002 lag p50={p50_ms:.3} ms p99={p99_ms:.3} ms; {total} events, {stored} stored; {}", path.display())?;
        if failures.is_empty() { Ok(()) } else { Err(format!("PERF-002: {}", failures.join("; ")).into()) }
    }))
    .await
}
