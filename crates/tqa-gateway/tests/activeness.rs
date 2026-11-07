//! GW-I-035: only accepted actions and run lifecycle events affect the idle policy.
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult};
use tqa_sandbox::SandboxBackend;
use tqa_types::{Event, SandboxState, ServerFrame};

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_035_background_processes_and_pongs_do_not_keep_an_idle_sandbox(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        // Resource accounting represents background work without a run.started event.
        f.backend.background_pids.store(3, Ordering::SeqCst);
        let handle = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        assert!(f.backend.stats(&handle).await?.pids >= 3);
        settle(f).await?;
        for _ in 0..19 {
            advance(f, 30).await?;
            runtime.pong().await?;
            f.clock.observed().await?;
            assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        }
        advance(f, 29).await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        advance(f, 1).await?;
        retired(f).await
    })
    .await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_035_model_wait_without_event_traffic_keeps_a_run_busy(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "waiting-model".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&conversation.id, 1).await?;
        settle(f).await?;
        // Advance policy time only: no pongs, heartbeats, output or accepted actions.
        advance(f, 1200).await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        assert_eq!(f.backend.list().await?.len(), 1);
        assert_eq!(
            f.gateway
                .events()
                .read(&conversation.id, 0.into(), 10)
                .await?
                .len(),
            1
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_035_redial_replays_a_lost_terminal_and_starts_the_idle_window(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let run_id = "terminal-replay".parse()?;
        runtime
            .send(&lifecycle(1, Event::RunStarted { run_id }))
            .await?;
        f.until_sequence(&conversation.id, 1).await?;
        settle(f).await?;
        let terminal = lifecycle(
            2,
            Event::RunCompleted {
                run_id: "terminal-replay".parse()?,
            },
        );
        drop(runtime);
        advance(f, 1200).await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        let mut redial = f.runtime(&conversation).await?;
        assert_eq!(redial.config["last_stored_seq"], 1);
        redial.send(&terminal).await?;
        f.until_sequence(&conversation.id, 2).await?;
        f.clock.observed().await?;
        f.clock.observed().await?;
        advance(f, 599).await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        advance(f, 1).await?;
        retired(f).await
    })
    .await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_035_client_sandbox_state_follows_the_run_lifecycle(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(id, &cookie, 0).await?;
        assert_eq!(stream.initial, SandboxState::Ready);
        let run_id = "client-state";
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: run_id.parse()?,
                },
            ))
            .await?;
        assert_eq!(stream.durable().await?.0, 1.into());
        assert_eq!(
            stream.state().await.map_err(|e| format!("busy: {e}"))?,
            SandboxState::Busy
        );
        assert_eq!(f.stream(id, &cookie, 1).await?.initial, SandboxState::Busy);
        // Freeze policy time before the terminal event so its idle window starts at a known instant.
        settle(f).await.map_err(|e| format!("settle: {e}"))?;
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: run_id.parse()?,
                },
            ))
            .await?;
        assert_eq!(stream.durable().await?.0, 2.into());
        assert_eq!(
            stream.state().await.map_err(|e| format!("idle: {e}"))?,
            SandboxState::Idle
        );
        assert_eq!(f.stream(id, &cookie, 2).await?.initial, SandboxState::Idle);
        f.clock.observed().await?;
        f.clock.observed().await?;
        advance(f, 599).await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        advance(f, 1).await.map_err(|e| format!("advance: {e}"))?;
        retired(f).await.map_err(|e| format!("retire: {e}"))?;
        assert_eq!(
            stream
                .state()
                .await
                .map_err(|e| format!("destroyed: {e}"))?,
            SandboxState::Destroyed
        );
        assert_eq!(
            f.stream(id, &cookie, 2).await?.initial,
            SandboxState::Destroyed
        );
        Ok(())
    })
    .await
}

fn lifecycle(sequence: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: sequence.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

async fn settle(f: &Fixture) -> TestResult {
    f.clock.freeze();
    f.clock.observed().await?;
    f.clock.observed().await
}

async fn advance(f: &Fixture, seconds: u64) -> TestResult {
    f.clock.advance(Duration::from_secs(seconds));
    f.clock.observed().await
}

async fn retired(f: &Fixture) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while f.backend.destroys.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}
