//! GW-I-158: a task whose only wait is its owner's own idle sandbox retiring to make room is queued with the
//! reason `own_sandbox`, so the page says it is starting, not waiting for a free slot. A wait behind another person's
//! work stays `slot`. The idle sandbox's destroy is held, so the retirement is caught while it is in flight. GW-I-309:
//! a sandbox whose turn its runtime took, with its run not yet started, isn't evicted for that work until the run ends.
#[path = "support/fixture.rs"]
mod support;

use serde_json::json;
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, Event, QueueReason, SandboxState, ServerFrame};

fn started(run: &str) -> Result<ServerFrame, Box<dyn std::error::Error>> {
    Ok(ServerFrame::Event {
        seq: 1.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: Event::RunStarted {
            run_id: run.parse()?,
        },
    })
}

async fn message(f: &Fixture, id: &ConversationId, cookie: &str, key: &str) -> TestResult {
    let accepted = f
        .action(
            &format!("/api/v1/conversations/{id}/messages"),
            cookie,
            Some(key),
            &json!({"text":"work"}),
        )
        .await?;
    assert_eq!(accepted.status(), 202);
    Ok(())
}

/// The waiting task's queue reason, once admission has run and the idle sandbox's retirement is under way.
async fn reason(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
) -> Result<Option<QueueReason>, Box<dyn std::error::Error>> {
    f.gateway.sandboxes().admission_pass_now().await?;
    let stream = f.stream(id, cookie, 0).await?;
    assert_eq!(stream.initial, SandboxState::Queued);
    Ok(stream.initial_reason)
}

async fn until(condition: impl Fn() -> bool) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

fn release(f: &Fixture) {
    f.backend.hold_destroy.store(false, Ordering::SeqCst);
    f.backend.release_destroy.notify_waiters();
}

// TEST: GW-I-158
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_158_an_owner_at_its_cap_waits_only_for_its_own_idle_sandbox(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=4\nper_user=1", async |f| {
        let (user, cookie) = f.admin().await?;
        let idle = f.conversation(user.id).await?;
        let _runtime = f.runtime(&idle).await?;
        f.backend.hold_destroy.store(true, Ordering::SeqCst);
        let next = f.conversation(user.id).await?;
        message(f, &next.id, &cookie, "next").await?;
        assert_eq!(
            reason(f, &next.id, &cookie).await?,
            Some(QueueReason::OwnSandbox)
        );
        release(f);
        until(|| f.backend.created(&next.id).is_some()).await
    })
    .await
}

// TEST: GW-I-158
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_158_a_full_host_whose_evicted_sandbox_is_the_same_persons_says_own_sandbox(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=2\nper_user=2", async |f| {
        let (a, a_cookie) = f.admin().await?;
        let (b, _) = f.member("b@example.invalid").await?;
        // The host is full: B's busy sandbox and A's idle one. Only A's can go.
        let busy = f.conversation(b.id).await?;
        let mut busy_runtime = f.runtime(&busy).await?;
        busy_runtime.send(&started("busy")?).await?;
        f.until_sequence(&busy.id, 1).await?;
        let idle = f.conversation(a.id).await?;
        let _runtime = f.runtime(&idle).await?;
        f.backend.hold_destroy.store(true, Ordering::SeqCst);
        let next = f.conversation(a.id).await?;
        message(f, &next.id, &a_cookie, "next").await?;
        assert_eq!(
            reason(f, &next.id, &a_cookie).await?,
            Some(QueueReason::OwnSandbox)
        );
        release(f);
        until(|| f.backend.created(&next.id).is_some()).await
    })
    .await
}

// TEST: GW-I-158
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_158_a_wait_behind_another_persons_work_stays_a_slot(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=1\nper_user=1", async |f| {
        let (a, a_cookie) = f.admin().await?;
        let (b, _) = f.member("b@example.invalid").await?;
        let busy = f.conversation(b.id).await?;
        let mut busy_runtime = f.runtime(&busy).await?;
        busy_runtime.send(&started("busy")?).await?;
        f.until_sequence(&busy.id, 1).await?;
        let next = f.conversation(a.id).await?;
        message(f, &next.id, &a_cookie, "next").await?;
        assert_eq!(
            reason(f, &next.id, &a_cookie).await?,
            Some(QueueReason::Slot)
        );
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        Ok(())
    })
    .await
}

fn frame(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

// TEST: GW-I-309
// A live check before 17.0 (18 Oct 2026): a sandbox retired between its runtime taking a turn and starting its run
// strands the turn. Eviction for queued work is such a retirement: the sandbox stays until that run has ended.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_309_a_sandbox_whose_turn_was_taken_isnt_evicted_until_its_run_ends(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=4\nper_user=1", async |f| {
        let (user, cookie) = f.admin().await?;
        let busy = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&busy).await?;
        message(f, &busy.id, &cookie, "busy").await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        // The runtime journals the message; its run hasn't started yet.
        runtime
            .send(&frame(
                1,
                Event::MessageUser {
                    text: "work".into(),
                    attachments: Vec::new(),
                    lead_brief: None,
                    team_wake: None,
                },
            ))
            .await?;
        f.until_sequence(&busy.id, 1).await?;
        let next = f.conversation(user.id).await?;
        message(f, &next.id, &cookie, "next").await?;
        for _ in 0..10 {
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(
            f.backend.list_conversations().await?.contains(&busy.id),
            "evicted while its turn was taken but its run hadn't started"
        );
        assert!(f.backend.created(&next.id).is_none(), "the next task waits");
        // The run starts and ends: the idle sandbox makes room.
        runtime
            .send(&frame(
                2,
                Event::RunStarted {
                    run_id: "r-1".parse()?,
                },
            ))
            .await?;
        runtime
            .send(&frame(
                3,
                Event::RunCompleted {
                    run_id: "r-1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&busy.id, 3).await?;
        let started = std::time::Instant::now();
        while f.backend.created(&next.id).is_none() {
            assert!(
                started.elapsed() < support::DEADLINE,
                "the next task never started"
            );
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Ok(())
    })
    .await
}
