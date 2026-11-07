//! GW-I-160 (the R2 soak on candidate 13b): a sandbox that stops unexpectedly is removed before it is replaced. When
//! that removal fails (an engine stalled or erring under load), the person's task never fails for it: the removal is
//! retried in the background (after 5, 15 and 45 seconds) while the task shows it is starting, nothing starts beside the
//! old sandbox until it is gone, and the run then resumes in the replacement. Only when every retry fails does the owner
//! see a retryable `sandbox_failed`.
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult};
use tqa_sandbox::SandboxBackend;
use tqa_types::{ErrorCode, Event, SandboxState, ServerFrame};

fn started() -> Result<ServerFrame, Box<dyn std::error::Error>> {
    Ok(ServerFrame::Event {
        seq: 1.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: Event::RunStarted {
            run_id: "lossy".parse()?,
        },
    })
}

/// The frames up to the next sandbox notice in `state`, which is the last.
/// The frames the stream sends until it is quiet for a second.
async fn until_quiet(
    stream: &mut support::stream::Stream,
) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let mut seen = Vec::new();
    while let Ok(next) = tokio::time::timeout(Duration::from_secs(1), stream.next()).await {
        seen.push(next?.1);
    }
    Ok(seen)
}

async fn until_state(
    stream: &mut support::stream::Stream,
    state: SandboxState,
) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let mut seen = Vec::new();
    for _ in 0..64 {
        let Ok(next) = tokio::time::timeout(Duration::from_secs(3), stream.next()).await else {
            return Err(format!("no {state:?} notice; the stream sent: {seen:?}").into());
        };
        let (_, frame) = next?;
        let reached =
            matches!(&frame, ServerFrame::SessionReady { sandbox, .. } if sandbox.state == state);
        seen.push(frame);
        if reached {
            return Ok(seen);
        }
    }
    Err(format!("no {state:?} notice: {seen:?}").into())
}

fn failed(frames: &[ServerFrame]) -> bool {
    frames.iter().any(|frame| {
        matches!(frame, ServerFrame::Error { code: ErrorCode::SandboxFailed, .. })
            || matches!(frame, ServerFrame::SessionReady { sandbox, .. } if sandbox.state == SandboxState::Failed)
    })
}

/// A busy task whose sandbox is then lost while its removal fails `failing` times: gone from the engine, or `stopped`
/// (still listed, as a killed container is); returns the stream and the sandboxes created before the loss.
async fn lose_with_failing_removal(
    f: &Fixture,
    failing: usize,
    stopped: bool,
) -> Result<(support::stream::Stream, usize, tqa_gateway::Conversation), Box<dyn std::error::Error>>
{
    let (user, cookie) = f.admin().await?;
    let conversation = f.conversation(user.id).await?;
    let mut runtime = f.runtime(&conversation).await?;
    let mut stream = f.stream(&conversation.id, &cookie, 0).await?;
    runtime.send(&started()?).await?;
    assert_eq!(stream.durable().await?.0, 1.into());
    f.clock.freeze();
    f.clock.observed().await?;
    let lost = f.backend.list().await?.pop().ok_or("sandbox missing")?;
    let created = f.backend.creates.load(Ordering::SeqCst);
    f.backend.fail_destroys.store(failing, Ordering::SeqCst);
    if stopped {
        f.backend.fail_address.store(true, Ordering::SeqCst);
    } else {
        f.backend.lose(&lost).await?;
    }
    drop(runtime);
    f.clock.observed().await?;
    Ok((stream, created, conversation))
}

// TEST: GW-I-160
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_160_a_failed_removal_of_a_lost_sandbox_is_retried_while_the_task_starts(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        // `lose` removes the container itself, so the gateway's own removal fails once, then succeeds.
        let (mut stream, created, conversation) = lose_with_failing_removal(f, 1, false).await?;
        let starting = until_state(&mut stream, SandboxState::Creating).await?;
        assert!(
            !failed(&starting),
            "the task starts, never fails: {starting:?}"
        );
        // Nothing starts beside the old sandbox until its removal succeeds.
        f.clock.advance(Duration::from_secs(4));
        f.clock.observed().await?;
        assert_eq!(f.backend.creates.load(Ordering::SeqCst), created);
        f.clock.advance(Duration::from_secs(1));
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) == created {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        f.forget_runtime(&conversation.id)?;
        let runtime = f.runtime(&conversation).await?;
        assert_eq!(
            runtime.config["last_stored_seq"], 1,
            "the run resumes in the replacement"
        );
        let after = until_state(&mut stream, SandboxState::Busy).await?;
        assert!(!failed(&after), "{after:?}");
        Ok(())
    })
    .await
}

/// Moves the idle clock on by `seconds` in 5 s ticks, letting the background retry run between them.
async fn advance(f: &Fixture, seconds: u64) {
    for _ in 0..seconds / 5 {
        f.clock.advance(Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// TEST: GW-I-160
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_160_an_engine_outage_longer_than_a_minute_recovers_while_the_task_starts(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        // The R2 soak on candidate 13d: an engine erring for about 150 s. A killed container stays listed, and its
        // removal fails at once and after 5, 20 and 65 s; the retry at 155 s removes it.
        let (mut stream, created, conversation) = lose_with_failing_removal(f, 4, true).await?;
        let starting = until_state(&mut stream, SandboxState::Creating).await?;
        assert!(!failed(&starting), "{starting:?}");
        advance(f, 150).await;
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            created,
            "nothing beside it yet"
        );
        // The engine is back before the retry at 155 s: the removal succeeds and the replacement starts.
        f.backend.fail_address.store(false, Ordering::SeqCst);
        advance(f, 10).await;
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) == created {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        f.forget_runtime(&conversation.id)?;
        let runtime = f.runtime(&conversation).await?;
        assert_eq!(
            runtime.config["last_stored_seq"], 1,
            "the run resumes in the replacement"
        );
        let after = until_state(&mut stream, SandboxState::Busy).await?;
        assert!(!failed(&after), "never shown as failed: {after:?}");
        Ok(())
    })
    .await
}

// TEST: GW-I-160
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_160_only_a_removal_still_failing_after_ten_minutes_is_shown_as_failed(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        // A killed container stays listed and the engine never answers: the task shows it is starting for the whole
        // ten minutes, and only then fails, retryably.
        let (mut stream, created, _) = lose_with_failing_removal(f, usize::MAX, true).await?;
        let starting = until_state(&mut stream, SandboxState::Creating).await?;
        assert!(!failed(&starting), "{starting:?}");
        advance(f, 595).await;
        let quiet = until_quiet(&mut stream).await?;
        assert!(!failed(&quiet), "not failed before the cap: {quiet:?}");
        advance(f, 10).await;
        let refused = until_state(&mut stream, SandboxState::Failed).await?;
        assert!(
            refused.iter().any(|frame| matches!(
                frame,
                ServerFrame::Error {
                    code: ErrorCode::SandboxFailed,
                    retryable: true,
                    ..
                }
            )),
            "{refused:?}"
        );
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            created,
            "nothing started beside it"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-160
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_160_a_slow_attempt_never_moves_the_later_ones(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        // The schedule counts from the first failure (5, 20, 65 … 600 s), not from the end of each attempt: under load
        // an attempt that takes long must not push the ten-minute cap later. Dues it overran collapse into one attempt.
        let (mut stream, _, _) = lose_with_failing_removal(f, usize::MAX, true).await?;
        until_state(&mut stream, SandboxState::Creating).await?;
        let before = f.backend.destroys.load(Ordering::SeqCst);
        f.backend.hold_destroy.store(true, Ordering::SeqCst);
        f.clock.advance(Duration::from_secs(5));
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.destroys.load(Ordering::SeqCst) == before {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        // The attempt at 5 s takes until 105 s.
        f.clock.advance(Duration::from_secs(100));
        f.backend.hold_destroy.store(false, Ordering::SeqCst);
        f.backend.release_destroy.notify_waiters();
        // The attempts due at 20 and 65 s are overdue: they collapse into one attempt now, with the clock still at
        // 105 s (one, not a burst against a stalled engine).
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.destroys.load(Ordering::SeqCst) < before + 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| "the overdue attempt didn't run")?;
        f.clock.observed().await?;
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            before + 2,
            "one attempt for both"
        );
        // The next is still the one due at 155 s.
        f.clock.advance(Duration::from_secs(45));
        f.clock.observed().await?;
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            before + 2,
            "nothing before 155 s"
        );
        f.clock.advance(Duration::from_secs(5));
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.destroys.load(Ordering::SeqCst) < before + 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| "the attempt at 155 s didn't run")?;
        Ok(())
    })
    .await
}
