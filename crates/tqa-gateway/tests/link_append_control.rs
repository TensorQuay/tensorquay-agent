//! GW-I-127: while the journal is slow to take a runtime frame, the link stays responsive and in order. It answers the
//! runtime's pings (the runtime probes the link with one after a failed model call and redials without a pong within
//! its own 5 s bound), and every frame that arrives meanwhile is journaled after the slow one, in the order sent. A
//! Close behind the slow frame never drops it: a runtime's last frame may be followed at once by its Close. A
//! heartbeat's workspace lease renewal, a store write too, doesn't hold the link up either.
#[path = "support/fixture.rs"]
mod support;

use serde_json::Value;
use sqlx::PgPool;
use std::time::{Duration, Instant};
use support::TestResult;
use tqa_types::ConversationId;

/// Released once a held table lock is: the test's own reads of that table wait for it too, and would time out under
/// a lock longer than `io_timeout`.
type Released = tokio::task::JoinHandle<Result<(), sqlx::Error>>;

/// Holds an exclusive lock on the events table for `duration` from another connection: a commit that stalls this long.
async fn hold_events(
    pool: &PgPool,
    duration: Duration,
) -> Result<Released, Box<dyn std::error::Error>> {
    hold(pool, "LOCK TABLE events IN ACCESS EXCLUSIVE MODE", duration).await
}

/// Takes the table lock `statement` from another connection and holds it for `duration`; returns once it is taken.
async fn hold(
    pool: &PgPool,
    statement: &'static str,
    duration: Duration,
) -> Result<Released, Box<dyn std::error::Error>> {
    let pool = pool.clone();
    let (locked, ready) = tokio::sync::oneshot::channel();
    let released = tokio::spawn(async move {
        let mut transaction = pool.begin().await?;
        sqlx::query(statement).execute(&mut *transaction).await?;
        let _ = locked.send(());
        // Real time on purpose: this stands in for a slow database, bounded by the duration.
        tokio::time::sleep(duration).await;
        transaction.commit().await?;
        Ok::<_, sqlx::Error>(())
    });
    ready.await?;
    Ok(released)
}

/// The journal's texts in sequence order, once it holds `count` frames, waiting up to `within`.
async fn journaled(
    fixture: &support::Fixture,
    id: &ConversationId,
    count: usize,
    within: Duration,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + within;
    loop {
        let frames = fixture.gateway.events().read(id, 0.into(), 1000).await?;
        if frames.len() >= count || Instant::now() > deadline {
            return frames
                .iter()
                .map(|frame| {
                    let value: Value = serde_json::to_value(frame)?;
                    Ok(value["event"]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned())
                })
                .collect();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Long enough for the gateway to have read the frame just sent and started its append.
const SETTLE: Duration = Duration::from_millis(300);

// TEST: GW-I-127
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_127_a_twenty_second_append_still_answers_a_ping_and_keeps_the_order(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let mut runtime = fixture.runtime(&created).await?;
        runtime.send(&support::frame(1, "first")).await?;
        assert_eq!(
            journaled(fixture, &created.id, 1, Duration::from_secs(10)).await?,
            ["first"]
        );

        // A commit that stalls 20 s: past one attempt (10 s), within the append budget (30 s).
        let released = hold_events(&fixture.pool, Duration::from_secs(20)).await?;
        runtime.send(&support::frame(2, "before the ping")).await?;
        tokio::time::sleep(SETTLE).await;
        runtime.send(&support::frame(3, "during the stall")).await?;
        let asked = Instant::now();
        // `roundtrip` also fails on any text frame, such as a `store_unavailable` error: the link stays open.
        runtime.roundtrip().await?;
        // A pong held behind the append would take the stall's 20 s; 10 s tells the two apart on any host (1 s was
        // close to a loaded one's).
        assert!(
            asked.elapsed() < Duration::from_secs(10),
            "the pong took {:?} while an append was pending",
            asked.elapsed()
        );
        runtime.send(&support::frame(4, "after the ping")).await?;

        // Once the stall ends, the frames are journaled once each, in the order sent.
        released.await??;
        assert_eq!(
            journaled(fixture, &created.id, 4, Duration::from_secs(40)).await?,
            [
                "first",
                "before the ping",
                "during the stall",
                "after the ping"
            ]
        );
        runtime.roundtrip().await?;
        Ok(())
    })
    .await
}

// TEST: GW-I-127
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_127_a_close_behind_a_slow_append_never_drops_it(pool: PgPool) -> TestResult {
    support::run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let mut runtime = fixture.runtime(&created).await?;
        runtime.send(&support::frame(1, "first")).await?;
        assert_eq!(
            journaled(fixture, &created.id, 1, Duration::from_secs(10)).await?,
            ["first"]
        );

        let released = hold_events(&fixture.pool, Duration::from_secs(4)).await?;
        runtime.send(&support::frame(2, "the last word")).await?;
        runtime.send(&support::frame(3, "held behind it")).await?;
        tokio::time::sleep(SETTLE).await;
        runtime.close().await?;
        released.await??;
        assert_eq!(
            journaled(fixture, &created.id, 3, Duration::from_secs(20)).await?,
            ["first", "the last word", "held behind it"]
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-127
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_127_a_slow_lease_renewal_still_answers_a_ping(pool: PgPool) -> TestResult {
    support::run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let mut runtime = fixture.runtime(&created).await?;
        let heartbeat = tqa_types::ServerFrame::Heartbeat {
            run_id: "run-renewing".parse()?,
        };
        // A heartbeat renews the lease; with the workspaces table locked that write takes 8 s (within `io_timeout`).
        runtime.send(&heartbeat).await?;
        runtime.roundtrip().await?;
        let released = hold(
            &fixture.pool,
            "LOCK TABLE workspaces IN ACCESS EXCLUSIVE MODE",
            Duration::from_secs(8),
        )
        .await?;
        runtime.send(&heartbeat).await?;
        tokio::time::sleep(SETTLE).await;
        let asked = Instant::now();
        runtime.roundtrip().await?;
        // Held behind the renewal it would take the lock's 8 s; 4 s tells the two apart (1 s was close to load's).
        assert!(
            asked.elapsed() < Duration::from_secs(4),
            "the pong took {:?} while a lease renewal was pending",
            asked.elapsed()
        );
        // The frame after the heartbeat still waits its turn, and the link stays open.
        runtime
            .send(&support::frame(1, "after the heartbeat"))
            .await?;
        released.await??;
        assert_eq!(
            journaled(fixture, &created.id, 1, Duration::from_secs(20)).await?,
            ["after the heartbeat"]
        );
        runtime.roundtrip().await?;
        Ok(())
    })
    .await
}
