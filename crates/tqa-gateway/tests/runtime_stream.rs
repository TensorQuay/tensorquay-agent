//! GW-I-006/007 and PROTO-U-004/006: fake runtime traffic through real HTTP and `WebSocket` transports.

#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_two_streams_receive_persisted_frames_and_actions_reach_the_runtime(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        let mut runtime = fixture.runtime(&created).await?;
        let mut first = fixture.stream(id, &cookie, 0).await?;
        let mut second = fixture.stream(id, &cookie, 0).await?;
        runtime.send(&support::frame(1, "first")).await?;
        assert_eq!(first.durable().await?.0, 1.into());
        assert_eq!(second.durable().await?.0, 1.into());
        assert_eq!(fixture.gateway.events().read(id, 0.into(), 10).await?.len(), 1);
        let action = fixture.action(&format!("/api/v1/conversations/{id}/messages"), &cookie, Some("first-message"), &json!({"text":"hello", "attachments":[]})).await?;
        assert_eq!(action.status(), 202);
        assert!(matches!(runtime.action().await?, tqa_types::ClientFrame::UserMessage { text, .. } if text == "hello"));
        drop(first);
        drop(second);
        runtime.send(&support::frame(2, "after disconnect")).await?;
        fixture.until_sequence(id, 2).await?;
        let mut replay = fixture.stream(id, &cookie, 1).await?;
        assert_eq!(replay.durable().await?.0, 2.into());
        Ok(())
    }).await
}

/// What the client stream showed after a runtime frame was sent.
enum Seen {
    Durable(u64),
    /// The documented store-outage notice (GW-E-020): the stream stays open and the runtime must redial.
    StoreOutage,
}

async fn seen(stream: &mut support::stream::Stream) -> Result<Seen, Box<dyn std::error::Error>> {
    loop {
        match stream.next().await? {
            (Some(sequence), _) => return Ok(Seen::Durable(sequence.to_string().parse()?)),
            (
                None,
                tqa_types::ServerFrame::Error {
                    code: tqa_types::ErrorCode::StoreUnavailable,
                    ..
                },
            ) => {
                return Ok(Seen::StoreOutage);
            }
            (None, tqa_types::ServerFrame::SessionReady { .. }) => {}
            (None, frame) => return Err(format!("unexpected unsequenced frame: {frame:?}").into()),
        }
    }
}

/// Streams `total` runtime frames to a client that reconnects after each sequence in `cuts`, and follows the documented
/// recovery when a store stall outlasts the append budget: the runtime redials and resumes after the gateway's last
/// stored sequence, and the open client stream receives every frame exactly once, in order. With `stall`, one
/// connection holds the events table for that long just before sequence `stall.0`.
async fn stream_frames(
    fixture: &support::Fixture,
    total: u64,
    cuts: &std::collections::BTreeSet<u64>,
    stall: Option<(u64, std::time::Duration)>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let (owner, cookie) = fixture.admin().await?;
    let created = fixture.conversation(owner.id).await?;
    let mut runtime = fixture.runtime(&created).await?;
    let mut stream = fixture.stream(&created.id, &cookie, 0).await?;
    let (mut delivered, mut outages) = (0_u64, 0_usize);
    while delivered < total {
        let sequence = delivered + 1;
        if let Some((at, duration)) = stall.filter(|(at, _)| *at == sequence && outages == 0) {
            hold_events(&fixture.pool, duration).await?;
            assert_eq!(at, sequence);
        }
        // A send can race the gateway closing the link; the stream then shows the outage.
        let _ = runtime.send(&support::frame(sequence, "streamed")).await;
        match seen(&mut stream).await? {
            Seen::Durable(seen) => {
                assert_eq!(seen, sequence, "frames arrive once and in order");
                delivered = seen;
                if cuts.contains(&seen) {
                    drop(stream);
                    stream = fixture.stream(&created.id, &cookie, seen).await?;
                }
            }
            Seen::StoreOutage => {
                outages += 1;
                assert!(outages <= 5, "the store kept failing");
                runtime = redial(fixture, &created).await?;
                let stored = runtime.config["last_stored_seq"]
                    .as_u64()
                    .ok_or("last_stored_seq")?;
                delivered = committed_tail(&mut stream, delivered, stored).await?;
            }
        }
    }
    let events = fixture
        .gateway
        .events()
        .read(&created.id, 0.into(), 1000)
        .await?;
    assert_eq!(u64::try_from(events.len())?, total);
    Ok(outages)
}

/// Redials like the runtime does after an outage: a registered runtime retries with backoff, each attempt bounded
/// by the gateway's I/O timeout, because registration itself needs the store and may fail again while the stall lasts.
async fn redial(
    fixture: &support::Fixture,
    created: &tqa_gateway::Conversation,
) -> Result<support::runtime::Runtime, Box<dyn std::error::Error>> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match fixture.runtime(created).await {
            Ok(runtime) => return Ok(runtime),
            Err(error) if attempts >= REDIALS => {
                return Err(format!("{attempts} redials failed, last: {error}").into());
            }
            Err(_) => {}
        }
    }
}

/// Attempts before a redial counts as failed: at the default 10 s each, a stall of about a minute (at the stall tests'
/// 2 s, about 12 s). A longer one is a database outage, which GW-E-020 covers.
const REDIALS: u32 = 6;

/// A frame committed before the outage still reaches the open stream: reads it, in order, up to `stored`.
async fn committed_tail(
    stream: &mut support::stream::Stream,
    mut delivered: u64,
    stored: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    while delivered < stored {
        // A redial that failed while the store was still stalled published another notice; it changes nothing.
        let Seen::Durable(seen) = seen(stream).await? else {
            continue;
        };
        assert_eq!(seen, delivered + 1, "frames arrive once and in order");
        delivered = seen;
    }
    Ok(delivered)
}

/// Holds an exclusive lock on the events table for `duration` from another connection, emulating a stalled commit.
async fn hold_events(pool: &PgPool, duration: std::time::Duration) -> TestResult {
    let pool = pool.clone();
    let (locked, ready) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut transaction = pool.begin().await?;
        sqlx::query("LOCK TABLE events IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *transaction)
            .await?;
        let _ = locked.send(());
        // Real time on purpose: this stands in for a slow database, bounded by the duration.
        tokio::time::sleep(duration).await;
        transaction.commit().await?;
        Ok::<_, sqlx::Error>(())
    });
    Ok(ready.await?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_007_one_thousand_frames_replay_exactly_after_twenty_seeded_disconnects(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        use rand::{Rng, SeedableRng};
        let mut random = rand::rngs::StdRng::seed_from_u64(7);
        let mut cuts = std::collections::BTreeSet::new();
        while cuts.len() < 20 {
            cuts.insert(random.random_range(1_u64..1000));
        }
        stream_frames(fixture, 1000, &cuts, None).await?;
        Ok(())
    })
    .await
}

/// A short `io_timeout` for the stall tests: one append attempt is 2 s, and a frame's attempts together 6 s.
const STALL_CONFIG: &str = "[limits]\nio_timeout=2\n";

/// GW-I-007 under a store stall longer than one append attempt but within the append budget (a slow commit under
/// host I/O pressure): the attempt that ran into it is retried, and no link closes.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_007_a_store_stall_within_the_append_budget_closes_nothing(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, STALL_CONFIG, async |fixture| {
        let cuts = std::collections::BTreeSet::from([20, 60]);
        let stall = (50, std::time::Duration::from_secs(4));
        let outages = stream_frames(fixture, 100, &cuts, Some(stall)).await?;
        assert_eq!(outages, 0, "a stall within the budget closes no link");
        Ok(())
    })
    .await
}

/// GW-I-007 under a store stall longer than the append budget: the documented outage recovery keeps the stream exact.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_007_a_store_stall_during_streaming_recovers_every_frame_once(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, STALL_CONFIG, async |fixture| {
        let cuts = std::collections::BTreeSet::from([20, 60]);
        let stall = (50, std::time::Duration::from_secs(9));
        let outages = stream_frames(fixture, 100, &cuts, Some(stall)).await?;
        assert_eq!(outages, 1, "the stall caused exactly one outage");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn proto_u_004_gap_closes_the_link_and_redial_accepts_the_correct_sequence(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let mut runtime = fixture.runtime(&created).await?;
        runtime.send(&support::frame(2, "gap")).await?;
        assert_eq!(runtime.error().await?, tqa_types::ErrorCode::SeqGap);
        runtime.closed().await?;
        let mut next = fixture.runtime(&created).await?;
        next.send(&support::frame(1, "correct")).await?;
        fixture.until_sequence(&created.id, 1).await?;
        let _replacement = fixture.runtime(&created).await?;
        assert_eq!(next.error().await?, tqa_types::ErrorCode::Restarting);
        next.closed().await?;
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn proto_u_006_stream_rejects_unsupported_versions_and_foreign_origins(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        let response = fixture
            .get(
                &format!("/api/v1/conversations/{id}/events?stream=1&protocol=2"),
                &cookie,
            )
            .await?;
        assert_eq!(response.status(), 400);
        assert_eq!(
            response.json::<serde_json::Value>().await?["error"]["code"],
            "unsupported_protocol"
        );
        for header in [
            b"18446744073709551616".as_slice(),
            b"-1".as_slice(),
            &[0xff],
        ] {
            let response = fixture
                .client
                .get(fixture.url(&format!(
                    "/api/v1/conversations/{id}/events?stream=1&protocol=1"
                )))
                .header("cookie", &cookie)
                .header(
                    "last-event-id",
                    reqwest::header::HeaderValue::from_bytes(header)?,
                )
                .send()
                .await?;
            assert_eq!(response.status(), 400);
        }
        let response = fixture
            .client
            .get(fixture.url(&format!(
                "/api/v1/conversations/{id}/events?stream=1&protocol=1"
            )))
            .header("cookie", &cookie)
            .header("origin", "https://foreign.invalid")
            .send()
            .await?;
        assert_eq!(response.status(), 403);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_heartbeat_updates_supervisor_liveness_without_persistence_or_fanout(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let id = &conversation.id;
        let mut runtime = fixture.runtime(&conversation).await?;
        let mut stream = fixture.stream(id, &cookie, 0).await?;
        runtime
            .send(&tqa_types::ServerFrame::Heartbeat {
                run_id: "running".parse()?,
            })
            .await?;
        runtime.send(&support::frame(1, "after heartbeat")).await?;
        assert_eq!(stream.durable().await?.0, 1.into());
        assert_eq!(
            fixture
                .gateway
                .last_heartbeat(id)?
                .ok_or("missing liveness")?
                .run_id
                .as_str(),
            "running"
        );
        assert_eq!(
            fixture.gateway.events().read(id, 0.into(), 10).await?.len(),
            1
        );
        Ok(())
    })
    .await
}
