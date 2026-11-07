//! GW-I-115 and GW-I-116: a sandbox whose teardown fails at its lifetime (a busy container engine times the destroy
//! out) never strands its run. The teardown is retried in the background after 5, 15 and 45 seconds, and the run's
//! replacement starts once it succeeds. If it never does, the owner is told with a retryable `sandbox_failed`, and the
//! sandbox is kept, never forgotten: the next message tries the teardown again first and starts a new sandbox only once
//! the old one is gone, never beside it.

#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, ErrorCode, Event, SandboxState, ServerFrame};

/// A one-minute lifetime and a one-day idle window, so only the lifetime acts.
const CONFIG: &str = "[sandbox]\nmax_lifetime_seconds=60\nidle_seconds=86400";
/// The lifetime plus the busy grace (`manager::idle::LIFETIME_GRACE`), and one step past it.
const PAST_THE_GRACE: [Duration; 3] = [
    Duration::from_secs(59),
    Duration::from_mins(2),
    Duration::from_secs(1),
];

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

/// Starts a busy run and takes it past its lifetime and grace, with the next `failures` destroys failing.
async fn busy_past_its_lifetime(
    f: &Fixture,
    failures: usize,
) -> Result<
    (tqa_gateway::Conversation, String, support::runtime::Runtime),
    Box<dyn std::error::Error>,
> {
    let (user, cookie) = f.admin().await?;
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
    f.clock.observed().await?;
    f.clock.observed().await?;
    f.backend.fail_destroys.store(failures, Ordering::SeqCst);
    for (step, by) in PAST_THE_GRACE.iter().enumerate() {
        f.clock.advance(*by);
        if step + 1 < PAST_THE_GRACE.len() {
            f.clock.observed().await?;
            f.clock.observed().await?;
        }
    }
    until(f, |f| f.backend.destroys.load(Ordering::SeqCst) >= 1).await?;
    Ok((conversation, cookie, runtime))
}

/// Advances the idle clock a minute at a time until the backend has seen `destroys` destroys.
async fn until_destroys(f: &Fixture, destroys: usize) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while f.backend.destroys.load(Ordering::SeqCst) < destroys {
            f.clock.advance(Duration::from_mins(1));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    Ok(())
}

/// The next error notice on the stream, skipping anything else.
async fn error_notice(
    stream: &mut support::stream::Stream,
) -> Result<(ErrorCode, bool), Box<dyn std::error::Error>> {
    for _ in 0..64 {
        if let (
            None,
            ServerFrame::Error {
                code, retryable, ..
            },
        ) = stream.next().await?
        {
            return Ok((code, retryable));
        }
    }
    Err("no error notice".into())
}

async fn message(
    f: &Fixture,
    cookie: &str,
    id: &ConversationId,
    key: &str,
) -> Result<u16, Box<dyn std::error::Error>> {
    let (status, _) = support::call(
        f,
        reqwest::Method::POST,
        &format!("/api/v1/conversations/{id}/messages"),
        cookie,
        (Some(serde_json::json!({"text": "go on"})), Some(key)),
    )
    .await?;
    Ok(status)
}

// TEST: GW-I-115
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_115_a_busy_sandbox_whose_teardown_times_out_is_retried_and_its_run_resumes(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        // The lifetime's destroy and the first background retry time out; the second retry succeeds.
        let (conversation, _, runtime) = busy_past_its_lifetime(f, 2).await?;
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            1,
            "nothing starts beside a sandbox not yet gone"
        );
        until_destroys(f, 3).await?;
        until(f, |f| f.backend.creates.load(Ordering::SeqCst) == 2).await?;
        drop(runtime);
        f.forget_runtime(&conversation.id)?;
        let replacement = f.runtime(&conversation).await?;
        assert_eq!(
            replacement.config["last_stored_seq"], 1,
            "the run resumes from its journal"
        );
        assert_eq!(
            f.backend.list_conversations().await?.len(),
            1,
            "one sandbox, never two"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-116
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_116_a_teardown_never_confirmed_is_reported_and_the_next_message_tries_it_first(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (conversation, cookie, runtime) = busy_past_its_lifetime(f, usize::MAX).await?;
        let id = conversation.id.clone();
        let mut stream = f.stream(&id, &cookie, 1).await?;
        // The lifetime's attempt and every retry up to ten minutes fail: the owner is told, and nothing else starts.
        // A minute a tick passes some retry times at once, and those collapse into one attempt, so the count of
        // attempts depends on the ticks; the last at ten minutes is always made.
        for _ in 0..11 {
            f.clock.advance(Duration::from_mins(1));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(error_notice(&mut stream).await?, (ErrorCode::SandboxFailed, true));
        assert!(f.backend.destroys.load(Ordering::SeqCst) >= 2, "the lifetime's attempt and the last retry");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(f.backend.creates.load(Ordering::SeqCst), 1, "never a sandbox beside one not confirmed gone");
        drop(runtime);
        // The next message tries the teardown again first; still failing, it is refused visibly, never left waiting.
        let destroys = f.backend.destroys.load(Ordering::SeqCst);
        assert_eq!(message(f, &cookie, &id, "still-stuck").await?, 202);
        until(f, |f| f.backend.destroys.load(Ordering::SeqCst) > destroys).await?;
        assert_eq!(error_notice(&mut stream).await?, (ErrorCode::SandboxFailed, true));
        let queued: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE conversation_id=$1::uuid AND execution_status='queued'",
        )
        .bind(id.as_str())
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(queued, 0, "the message is failed, not left queued");
        assert_eq!(f.backend.creates.load(Ordering::SeqCst), 1);
        // Once the engine answers again, the next message's teardown succeeds and only then a new sandbox starts.
        f.backend.fail_destroys.store(0, Ordering::SeqCst);
        assert_eq!(message(f, &cookie, &id, "engine-back").await?, 202);
        until(f, |f| f.backend.creates.load(Ordering::SeqCst) == 2).await?;
        f.forget_runtime(&id)?;
        let resumed = f.runtime(&conversation).await?;
        assert_eq!(resumed.config["last_stored_seq"], 1);
        assert_eq!(f.backend.list_conversations().await?.len(), 1, "one sandbox, never two");
        let state = loop {
            if let (None, ServerFrame::SessionReady { sandbox, .. }) = stream.next().await?
                && sandbox.state != SandboxState::Failed
                && sandbox.state != SandboxState::Destroyed
            {
                break sandbox.state;
            }
        };
        assert_ne!(state, SandboxState::Failed);
        Ok(())
    })
    .await
}
