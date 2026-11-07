//! GW-I-066 (ARCHITECTURE §4.4, reaper rule 1): the max lifetime is a real cap. A busy sandbox is not retired before it;
//! at the cap it takes no new work and gets a grace period, then is retired while busy and replaced at once so its run
//! resumes; a run that ends within the grace retires as idle.

#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult};
use tqa_types::{Event, ServerFrame};

/// A one-minute lifetime and a one-day idle window, so only the lifetime acts.
const CONFIG: &str = "[sandbox]\nmax_lifetime_seconds=60\nidle_seconds=86400";
const LIFETIME: Duration = Duration::from_mins(1);
/// The gateway's grace for a busy sandbox at its lifetime (`manager::idle::LIFETIME_GRACE`).
const GRACE: Duration = Duration::from_mins(2);

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

async fn until(f: &Fixture, condition: impl Fn(&Fixture) -> bool) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while !condition(f) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

/// Waits until the sandbox's worker is running: its lifetime is measured from then (it reads the clock each pass).
async fn worker_running(f: &Fixture) -> TestResult {
    f.clock.observed().await?;
    f.clock.observed().await
}

/// Moves the fake idle clock and waits until the worker has read it twice (one whole pass).
async fn advance(f: &Fixture, by: Duration) -> TestResult {
    f.clock.advance(by);
    f.clock.observed().await?;
    f.clock.observed().await
}

// TEST: GW-I-066
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_066_a_busy_sandbox_is_retired_after_the_grace_and_its_run_resumes_in_a_new_one(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        f.clock.freeze();
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "long".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&conversation.id, 1).await?;
        worker_running(f).await?;
        advance(f, LIFETIME.saturating_sub(Duration::from_secs(1))).await?;
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            0,
            "never before the lifetime"
        );
        advance(f, GRACE).await?;
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            0,
            "a busy sandbox gets the grace"
        );
        // The step past the grace retires it: move the clock and wait for the retirement itself (a retired sandbox's
        // worker stops reading the clock).
        f.clock.advance(Duration::from_secs(1));
        until(f, |f| f.backend.destroys.load(Ordering::SeqCst) == 1).await?;
        until(f, |f| f.backend.creates.load(Ordering::SeqCst) == 2).await?;
        // The replacement's runtime registers and is handed the conversation's journal to resume from.
        drop(runtime);
        f.forget_runtime(&conversation.id)?;
        let replacement = f.runtime(&conversation).await?;
        assert_eq!(
            replacement.config["last_stored_seq"], 1,
            "the run's journal survives"
        );
        // One retirement at the lifetime is counted with its fixed reason.
        let metrics = {
            use tower::ServiceExt;
            let response = f
                .gateway
                .metrics_router()
                .oneshot(axum::http::Request::get("/metrics").body(axum::body::Body::empty())?)
                .await?;
            String::from_utf8(
                http_body_util::BodyExt::collect(response.into_body())
                    .await?
                    .to_bytes()
                    .to_vec(),
            )?
        };
        assert!(
            metrics.contains("tqa_sandbox_retired_total{reason=\"lifetime\"} 1"),
            "the lifetime retirement is counted: {metrics}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-066
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_066_a_run_that_ends_within_the_grace_retires_the_sandbox_without_a_replacement(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        f.clock.freeze();
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "short".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&conversation.id, 1).await?;
        worker_running(f).await?;
        advance(f, LIFETIME + Duration::from_secs(30)).await?;
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            0,
            "within the grace"
        );
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "short".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&conversation.id, 2).await?;
        f.clock.advance(Duration::from_secs(1));
        until(f, |f| f.backend.destroys.load(Ordering::SeqCst) == 1).await?;
        // Retirement ends when the instance record is gone; no replacement follows without work.
        let uuid = uuid::Uuid::parse_str(conversation.id.as_str())?;
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let live: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM sandbox_instances WHERE conversation_id=$1",
                )
                .bind(uuid)
                .fetch_one(&f.pool)
                .await?;
                if live == 0 {
                    return Ok::<_, sqlx::Error>(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await??;
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            1,
            "no replacement without work"
        );
        Ok(())
    })
    .await
}
