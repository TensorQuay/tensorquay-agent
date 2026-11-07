//! GW-I-010: socket liveness never extends the 600-second idle window.
#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::TestResult;
use tqa_types::{Event, ServerFrame};

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_010_connected_pongs_do_not_prevent_default_idle_retirement(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        f.clock.freeze();
        // Observe a complete policy pass after construction or replay has settled.
        f.clock.observed().await?;
        f.clock.observed().await?;
        for _ in 0..19 {
            f.clock.advance(Duration::from_secs(30));
            runtime.pong().await?;
            f.clock.observed().await?;
            assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        }
        f.clock.advance(Duration::from_secs(29));
        f.clock.observed().await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        f.clock.advance(Duration::from_secs(1));
        until_retired(f).await?;
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_010_active_run_survives_and_terminal_event_starts_a_fresh_window(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let run_id: tqa_types::RunId = "run-idle-clock".parse()?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: run_id.clone(),
                },
            ))
            .await?;
        f.until_sequence(&conversation.id, 1).await?;
        f.clock.freeze();
        // Observe a complete policy pass after construction or replay has settled.
        f.clock.observed().await?;
        f.clock.observed().await?;
        for _ in 0..24 {
            f.clock.advance(Duration::from_secs(30));
            runtime.pong().await?;
            f.clock.observed().await?;
            runtime
                .send(&ServerFrame::Heartbeat {
                    run_id: run_id.clone(),
                })
                .await?;
            runtime.pong().await?;
            f.clock.observed().await?;
            assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        }
        runtime
            .send(&lifecycle(2, Event::RunCompleted { run_id }))
            .await?;
        f.until_sequence(&conversation.id, 2).await?;
        runtime.roundtrip().await?;
        f.clock.observed().await?;
        f.clock.observed().await?;
        for _ in 0..19 {
            f.clock.advance(Duration::from_secs(30));
            runtime.pong().await?;
            f.clock.observed().await?;
            assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        }
        f.clock.advance(Duration::from_secs(29));
        f.clock.observed().await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        f.clock.advance(Duration::from_secs(1));
        until_retired(f).await?;
        Ok(())
    })
    .await
}

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

async fn until_retired(f: &support::Fixture) -> TestResult {
    let deadline = std::time::Instant::now() + support::DEADLINE;
    while f.backend.destroys.load(Ordering::SeqCst) == 0 {
        if std::time::Instant::now() >= deadline {
            return Err("idle runtime was not retired".into());
        }
        tokio::task::yield_now().await;
    }
    Ok(())
}
