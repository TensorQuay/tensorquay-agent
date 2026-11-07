//! GW-I-208, the full-disk P0 (the 8 h soak on candidate 13h, 3 Oct 2026): a sandbox lost mid-run whose replacement can't be
//! created (a full disk fails the create) is retried while the disk stays full, and the run resumes in the
//! replacement once space is back. It never ends with neither a retry nor a terminal event (the run hung). Only a failed
//! create is retried: a replacement whose runtime dies at start ends the run failed, as before (candidate 14a).
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult, call};
use tqa_sandbox::SandboxBackend;
use tqa_types::{ConversationId, ErrorCode, Event, SandboxState, ServerFrame};

/// The next sandbox notice on the stream: its state and the recovery's countdown.
async fn notice(
    stream: &mut support::stream::Stream,
) -> Result<(SandboxState, Option<u32>), Box<dyn std::error::Error>> {
    for _ in 0..64 {
        if let (None, ServerFrame::SessionReady { sandbox, .. }) = stream.next().await? {
            return Ok((
                sandbox.state,
                sandbox.recovery.and_then(|recovery| recovery.retry_in_secs),
            ));
        }
    }
    Err("no sandbox notice".into())
}

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

/// The run's error code, read from the stream; a recovery countdown before it is a failure (only a failed create counts
/// down).
async fn run_error(
    stream: &mut support::stream::Stream,
) -> Result<ErrorCode, Box<dyn std::error::Error>> {
    loop {
        match stream.next().await? {
            (_, ServerFrame::Error { code, .. }) => return Ok(code),
            (_, ServerFrame::SessionReady { sandbox, .. }) => {
                let countdown = sandbox.recovery.and_then(|recovery| recovery.retry_in_secs);
                if countdown.is_some_and(|seconds| seconds > 0) {
                    return Err(format!(
                        "a countdown for a create that never failed: {countdown:?}"
                    )
                    .into());
                }
            }
            _ => {}
        }
    }
}

/// Whether a sandbox starts for `id` within 200 s of the idle clock: a slot held by a retried replacement stays held
/// far longer (its schedule runs to 600 s).
async fn starts(f: &Fixture, id: &ConversationId) -> bool {
    for _ in 0..40 {
        if f.gateway.sandboxes().ensure(id).await.is_ok() {
            return true;
        }
        f.clock.advance(Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// Moves the idle clock on by `seconds` in 5 s ticks, letting the worker act between them.
async fn advance(f: &Fixture, seconds: u64) {
    for _ in 0..seconds / 5 {
        f.clock.advance(Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// TEST: GW-I-208
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_208_a_replacement_that_cant_be_created_is_retried_and_the_run_resumes(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(&conversation.id, &cookie, 0).await?;
        runtime.send(&started()?).await?;
        assert_eq!(stream.durable().await?.0, 1.into());
        f.clock.freeze();
        f.clock.observed().await?;
        // The disk fills: the runtime is lost, and every create fails while it stays full.
        let lost = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        f.backend.fail_create.store(true, Ordering::SeqCst);
        f.backend.lose(&lost).await?;
        drop(runtime);
        let created = f.backend.creates.load(Ordering::SeqCst);
        // While it's full the replacement keeps being tried, and the task is never left with nothing to come.
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) <= created {
                f.clock.advance(Duration::from_secs(5));
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?;
        // The person sees the wait, never silence: after the failed create, a notice of when it is tried again.
        let mut counted = false;
        for _ in 0..16 {
            if let (_, Some(seconds)) = notice(&mut stream).await?
                && seconds > 0
            {
                counted = true;
                break;
            }
        }
        assert!(
            counted,
            "a notice with a countdown while the replacement is retried"
        );
        advance(f, 30).await;
        let tried = f.backend.creates.load(Ordering::SeqCst);
        assert!(
            tried > created + 1,
            "retried while the disk is full: {} tries",
            tried - created
        );
        // Space is back (100 s into the outage, as in the soak): the next try starts a sandbox and the run resumes.
        f.backend.fail_create.store(false, Ordering::SeqCst);
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.created(&conversation.id).is_none() {
                f.clock.advance(Duration::from_secs(5));
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| "no sandbox was created once space was back: the run hangs")?;
        f.forget_runtime(&conversation.id)?;
        let runtime = f.runtime(&conversation).await?;
        assert_eq!(
            runtime.config["last_stored_seq"], 1,
            "the run resumes in the replacement"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-208
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_208_a_replacement_whose_runtime_dies_at_start_ends_failed_not_retried(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(&conversation.id, &cookie, 0).await?;
        runtime.send(&started()?).await?;
        assert_eq!(stream.durable().await?.0, 1.into());
        f.clock.freeze();
        f.clock.observed().await?;
        // The sandbox is lost, and its replacement is created but its runtime exits at start (candidate 14a's
        // connector suites): that is no failed create, so nothing is retried.
        let lost = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        *f.backend.runtime_exit.lock().map_err(|_| "exit lock")? = Some(2);
        f.backend.lose(&lost).await?;
        drop(runtime);
        let created = f.backend.creates.load(Ordering::SeqCst);
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) <= created {
                f.clock.advance(Duration::from_secs(5));
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?;
        // The run ends with the runtime's own code ("the agent stopped while starting"), never a countdown.
        let code = tokio::time::timeout(support::DEADLINE, run_error(&mut stream))
            .await
            .map_err(|_| "the run never ended with an error")??;
        assert_eq!(code, ErrorCode::RuntimeStartFailed);
        // And it stays ended: no further create as the clock moves past the whole retry schedule.
        let ended = f.backend.creates.load(Ordering::SeqCst);
        advance(f, 700).await;
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            ended,
            "no retry of a start failure"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-208
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_208_a_deleted_tasks_sandbox_is_never_replaced_and_its_slot_frees(
    pool: PgPool,
) -> TestResult {
    let one_slot = "[sandbox]\nmax_live=1\n[admission]\nslots=1\nper_user=1";
    support::run_configured(pool, None, one_slot, async |f| {
        let (user, cookie) = f.admin().await?;
        let deleted = f.conversation(user.id).await?;
        let runtime = f.runtime(&deleted).await?;
        f.clock.freeze();
        f.clock.observed().await?;
        // Deleted while its sandbox is live (candidate 14a's perf_start deleted its warm-up tasks so).
        let path = format!("/api/v1/conversations/{}", deleted.id);
        assert_eq!(
            call(f, Method::DELETE, &path, &cookie, (None, None))
                .await?
                .0,
            204
        );
        // The delete revoked the runtime's token: the runtime goes, and its sandbox with it.
        drop(runtime);
        let sandbox = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        f.backend.lose(&sandbox).await?;
        let created = f.backend.creates.load(Ordering::SeqCst);
        // Its slot frees, so with one slot the next task's sandbox starts; no replacement was tried for the deleted one.
        let next = f.conversation(user.id).await?;
        assert!(
            starts(f, &next.id).await,
            "the deleted task still holds the only slot"
        );
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            created + 1,
            "one create, the next task's: none for the deleted task"
        );
        Ok(())
    })
    .await
}
