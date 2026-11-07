//! GW-I-114: a sandbox that keeps stopping unexpectedly (a full disk makes its runtime exit on every write) is replaced
//! with a growing delay, and the fifth loss in ten minutes retires it as failed with a retryable `sandbox_failed`; the
//! run stays unfinished, and the owner's next message starts a new sandbox. The owner sees the count and the delay.
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::TestResult;
use tqa_sandbox::SandboxBackend;
use tqa_types::{ErrorCode, Event, SandboxRecovery, SandboxState, ServerFrame};

fn lifecycle(sequence: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: sequence.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

/// The recovery on the next sandbox notice in `state`, skipping any other frame.
async fn notice(
    stream: &mut support::stream::Stream,
    state: SandboxState,
) -> Result<Option<SandboxRecovery>, Box<dyn std::error::Error>> {
    for _ in 0..64 {
        if let (None, ServerFrame::SessionReady { sandbox, .. }) = stream.next().await?
            && sandbox.state == state
        {
            return Ok(sandbox.recovery);
        }
    }
    Err(format!("no {state:?} notice").into())
}

async fn creates_reach(f: &support::Fixture, count: usize) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while f.backend.creates.load(Ordering::SeqCst) < count {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_114_a_sandbox_that_keeps_stopping_is_replaced_with_a_growing_delay_then_not_at_all(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(id, &cookie, 0).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "lossy".parse()?,
                },
            ))
            .await?;
        assert_eq!(stream.durable().await?.0, 1.into());
        f.clock.freeze();
        f.clock.observed().await?;
        // Four losses are replaced, after 0, 5, 15 and 45 seconds.
        for (loss, delay) in [(1_u32, 0_u64), (2, 5), (3, 15), (4, 45)] {
            let lost = f.backend.list().await?.pop().ok_or("sandbox missing")?;
            let created = f.backend.creates.load(Ordering::SeqCst);
            f.backend.lose(&lost).await?;
            drop(runtime);
            f.clock.observed().await?;
            let destroyed = notice(&mut stream, SandboxState::Destroyed).await?;
            assert_eq!(
                destroyed,
                Some(SandboxRecovery {
                    losses: loss,
                    retry_in_secs: Some(u32::try_from(delay)?)
                })
            );
            if delay > 0 {
                // Nothing starts before the delay has passed.
                f.clock.advance(Duration::from_secs(delay - 1));
                f.clock.observed().await?;
                f.clock.observed().await?;
                assert_eq!(
                    f.backend.creates.load(Ordering::SeqCst),
                    created,
                    "loss {loss} replaced early"
                );
                f.clock.advance(Duration::from_secs(1));
            }
            let creating = notice(&mut stream, SandboxState::Creating).await?;
            assert_eq!(
                creating,
                Some(SandboxRecovery {
                    losses: loss,
                    retry_in_secs: None
                })
            );
            creates_reach(f, created + 1).await?;
            f.forget_runtime(id)?;
            runtime = f.runtime(&conversation).await?;
            // The worker reads the clock again only once the replacement has started: lose it before that and the
            // start itself fails.
            f.clock.observed().await?;
            assert_eq!(
                runtime.config["last_stored_seq"], 1,
                "the replacement resumes from the history"
            );
        }
        // The fifth loss in the window is not replaced: failed, with a retryable code the owner can act on.
        let lost = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        let created = f.backend.creates.load(Ordering::SeqCst);
        f.backend.lose(&lost).await?;
        drop(runtime);
        f.clock.observed().await?;
        let mut refused = None;
        for _ in 0..64 {
            match stream.next().await?.1 {
                ServerFrame::Error {
                    code,
                    retryable,
                    message,
                    ..
                } => refused = Some((code, retryable, message)),
                ServerFrame::SessionReady { sandbox, .. }
                    if sandbox.state == SandboxState::Failed =>
                {
                    assert_eq!(
                        sandbox.recovery,
                        Some(SandboxRecovery {
                            losses: 5,
                            retry_in_secs: None
                        })
                    );
                    break;
                }
                _ => {}
            }
        }
        let (code, retryable, message) =
            refused.ok_or("no error notice before the failed state")?;
        assert_eq!((code, retryable), (ErrorCode::SandboxFailed, true));
        assert!(
            message.contains("5 times") && message.contains("send a message"),
            "{message}"
        );
        // The worker has ended, so nothing reads the clock any more: give a replacement real time to appear.
        f.clock.advance(Duration::from_mins(2));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            created,
            "no replacement after the limit"
        );
        assert!(f.backend.list().await?.is_empty());
        // The run is unfinished, not failed: nothing was written to its history.
        assert_eq!(f.gateway.events().read(id, 0.into(), 10).await?.len(), 1);
        // The owner's next message starts a new sandbox, with a fresh allowance.
        let (status, _) = support::call(
            f,
            reqwest::Method::POST,
            &format!("/api/v1/conversations/{id}/messages"),
            &cookie,
            (
                Some(serde_json::json!({"text": "go on"})),
                Some("resume-after-losses"),
            ),
        )
        .await?;
        assert_eq!(status, 202);
        creates_reach(f, created + 1).await?;
        f.forget_runtime(id)?;
        let resumed = f.runtime(&conversation).await?;
        assert_eq!(resumed.config["last_stored_seq"], 1);
        Ok(())
    })
    .await
}

// TEST: GW-I-227
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_227_a_replacement_whose_vm_host_dies_before_registering_is_started_once_more(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(id, &cookie, 0).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "blip".parse()?,
                },
            ))
            .await?;
        assert_eq!(stream.durable().await?.0, 1.into());
        f.clock.freeze();
        f.clock.observed().await?;
        let lost = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        let created = f.backend.creates.load(Ordering::SeqCst);
        // Mid-task: the sandbox is lost, and its replacement's VM host dies once before registering.
        f.backend.host_deaths.store(1, Ordering::SeqCst);
        f.backend.lose(&lost).await?;
        drop(runtime);
        f.clock.observed().await?;
        notice(&mut stream, SandboxState::Destroyed).await?;
        notice(&mut stream, SandboxState::Creating).await?;
        creates_reach(f, created + 2).await?;
        f.forget_runtime(id)?;
        let _runtime = f.runtime(&conversation).await?;
        f.clock.observed().await?;
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            created + 2,
            "the replacement started once more, no further"
        );
        assert_eq!(f.backend.host_deaths.load(Ordering::SeqCst), 0);
        let later = f.stream(id, &cookie, 0).await?;
        assert_ne!(
            later.initial,
            SandboxState::Failed,
            "no failed start mid-task for one blip"
        );
        Ok(())
    })
    .await
}
