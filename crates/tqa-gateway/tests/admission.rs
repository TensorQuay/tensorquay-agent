//! GW-I-034: slots, eviction of the least recently active idle sandbox, pins, and a queue fair across users.
#[path = "support/fixture.rs"]
mod support;

use serde_json::json;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, Event, SandboxState, ServerFrame};

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
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
    assert_eq!(
        accepted.status(),
        202,
        "accepted and queued, never 429 for a full host"
    );
    Ok(())
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

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_034_idle_is_evicted_busy_and_pinned_never_and_the_queue_is_fair(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=2", async |f| {
        let (a, a_cookie) = f.admin().await?;
        let (b, b_cookie) = f.member("b@example.invalid").await?;
        let (mut busy_runtime, busy, pinned) =
            Box::pin(evict_the_idle_sandbox(f, a.id, b.id, &b_cookie)).await?;
        // B pins its sandbox; with one busy and one pinned sandbox nothing is evictable.
        Box::pin(pin(f, &pinned.id, &b_cookie)).await?;
        // B queues first, then A.
        let b_waiting = f.conversation(b.id).await?;
        let a_waiting = f.conversation(a.id).await?;
        message(f, &b_waiting.id, &b_cookie, "b-waiting").await?;
        message(f, &a_waiting.id, &a_cookie, "a-waiting").await?;
        let b_stream = f.stream(&b_waiting.id, &b_cookie, 0).await?;
        let a_stream = f.stream(&a_waiting.id, &a_cookie, 0).await?;
        // Each owner has one live sandbox: the earlier request is next.
        assert_eq!(
            (b_stream.initial, b_stream.initial_queue),
            (SandboxState::Queued, Some(0))
        );
        assert_eq!(
            (a_stream.initial, a_stream.initial_queue),
            (SandboxState::Queued, Some(1))
        );
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            1,
            "busy and pinned are never evicted"
        );
        // A's run ends and its sandbox retires after the idle window; the pinned one does not.
        f.clock.freeze();
        f.clock.observed().await?;
        busy_runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "busy".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&busy.id, 2).await?;
        f.clock.observed().await?;
        f.clock.observed().await?;
        f.clock.advance(Duration::from_mins(10));
        f.clock.observed().await?;
        until(f, |f| f.backend.created(&a_waiting.id).is_some()).await?;
        // A now has no live sandbox and B has one: A is served first although B asked earlier.
        assert!(
            f.backend.created(&b_waiting.id).is_none(),
            "fair across users, not FIFO"
        );
        let live = f.backend.list_conversations().await?;
        assert!(
            live.contains(&pinned.id),
            "the pinned sandbox survived the idle window"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_034_after_replay_the_stream_ends_with_the_current_queued_snapshot(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=1", async |f| {
        let (a, cookie) = f.admin().await?;
        let busy = f.conversation(a.id).await?;
        let mut busy_runtime = f.runtime(&busy).await?;
        busy_runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "busy".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&busy.id, 1).await?;
        // Earlier work left an idle `sandbox.state` in the history of the conversation that now waits.
        let waiting = f.conversation(a.id).await?;
        f.gateway
            .events()
            .append(&waiting.id, support::frame(1, "earlier work"))
            .await?;
        f.gateway
            .events()
            .append(
                &waiting.id,
                lifecycle(
                    2,
                    Event::SandboxState {
                        state: SandboxState::Idle,
                    },
                ),
            )
            .await?;
        message(f, &waiting.id, &cookie, "follow-up").await?;
        let mut stream = f.stream(&waiting.id, &cookie, 0).await?;
        assert_eq!(
            (stream.initial, stream.initial_queue),
            (SandboxState::Queued, Some(0))
        );
        assert_eq!(stream.durable().await?.0, 1.into());
        assert_eq!(stream.durable().await?.0, 2.into());
        // The last frame before live traffic is the current snapshot, not the replayed idle state.
        let (sequence, snapshot) = stream.next().await?;
        assert_eq!(sequence, None);
        assert!(
            matches!(
                &snapshot,
                ServerFrame::SessionReady { sandbox, queue: Some(queue), .. }
                    if sandbox.state == SandboxState::Queued && queue.ahead() == 0
            ),
            "{snapshot:?}"
        );
        Ok(())
    })
    .await
}

/// A has one busy and one idle warm sandbox; B's first message evicts the idle one, never the busy one.
async fn evict_the_idle_sandbox(
    f: &Fixture,
    a: uuid::Uuid,
    b: uuid::Uuid,
    b_cookie: &str,
) -> Result<
    (
        support::runtime::Runtime,
        tqa_gateway::Conversation,
        tqa_gateway::Conversation,
    ),
    Box<dyn std::error::Error>,
> {
    // A: one busy sandbox, then one idle warm sandbox.
    let busy = f.conversation(a).await?;
    let mut busy_runtime = f.runtime(&busy).await?;
    busy_runtime
        .send(&lifecycle(
            1,
            Event::RunStarted {
                run_id: "busy".parse()?,
            },
        ))
        .await?;
    f.until_sequence(&busy.id, 1).await?;
    let idle = f.conversation(a).await?;
    let _idle_runtime = f.runtime(&idle).await?;
    // B needs a slot: the idle warm sandbox is evicted, the busy one is not.
    let pinned = f.conversation(b).await?;
    message(f, &pinned.id, b_cookie, "b-first").await?;
    until(f, |f| f.backend.created(&pinned.id).is_some()).await?;
    assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 1);
    assert!(f.backend.created(&busy.id).is_some() && f.backend.mounts(&busy.id).is_some());
    let live: Vec<_> = f.backend.list_conversations().await?;
    assert!(
        live.contains(&busy.id) && !live.contains(&idle.id),
        "the idle sandbox was evicted: {live:?}"
    );
    let mut pinned_runtime = f.runtime(&pinned).await?;
    assert!(matches!(
        pinned_runtime.action().await?,
        tqa_types::ClientFrame::UserMessage { .. }
    ));
    Ok((busy_runtime, busy, pinned))
}

async fn pin(f: &Fixture, id: &ConversationId, cookie: &str) -> TestResult {
    let pin = f
        .client
        .patch(f.url(&format!("/api/v1/conversations/{id}")))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", "pin")
        .json(&json!({"pinned": true}))
        .send()
        .await?;
    assert_eq!(pin.status(), 200);
    assert_eq!(pin.json::<serde_json::Value>().await?["pinned"], true);
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_034_a_pinned_sandbox_outlives_the_idle_window_until_its_maximum_lifetime(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        f.clock.freeze();
        let _runtime = f.runtime(&conversation).await?;
        Box::pin(pin(f, &conversation.id, &cookie)).await?;
        f.clock.observed().await?;
        f.clock.observed().await?;
        // Well past the 600 s idle window, before the 28800 s default lifetime.
        f.clock.advance(Duration::from_secs(28_799));
        f.clock.observed().await?;
        f.clock.observed().await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        f.clock.advance(Duration::from_secs(1));
        until(f, |f| f.backend.destroys.load(Ordering::SeqCst) == 1).await
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_034_one_slot_one_user_the_second_conversation_starts_once_the_first_is_idle(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=1", async |f| {
        let (user, cookie) = f.admin().await?;
        let first = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&first).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "first".parse()?,
                },
            ))
            .await?;
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "first".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&first.id, 2).await?;
        // The owner is at its cap; its own finished sandbox is evicted without waiting for the idle window.
        let second = f.conversation(user.id).await?;
        message(f, &second.id, &cookie, "second").await?;
        until(f, |f| f.backend.created(&second.id).is_some()).await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 1);
        assert!(!f.backend.list_conversations().await?.contains(&first.id));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_034_an_owner_whose_own_sandbox_is_pinned_stays_queued(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=1", async |f| {
        let (user, cookie) = f.admin().await?;
        let first = f.conversation(user.id).await?;
        let _runtime = f.runtime(&first).await?;
        Box::pin(pin(f, &first.id, &cookie)).await?;
        let second = f.conversation(user.id).await?;
        message(f, &second.id, &cookie, "second").await?;
        for _ in 0..3 {
            f.gateway.sandboxes().admission_pass_now().await?;
        }
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            0,
            "a pinned sandbox is never evicted"
        );
        assert!(f.backend.created(&second.id).is_none());
        assert_eq!(
            f.stream(&second.id, &cookie, 0).await?.initial,
            SandboxState::Queued
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_034_an_open_stream_sees_creating_when_its_queued_conversation_is_admitted(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=1", async |f| {
        let (a, _) = f.admin().await?;
        let (b, b_cookie) = f.member("b@example.invalid").await?;
        let busy = f.conversation(a.id).await?;
        let mut busy_runtime = f.runtime(&busy).await?;
        busy_runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "busy".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&busy.id, 1).await?;
        let waiting = f.conversation(b.id).await?;
        message(f, &waiting.id, &b_cookie, "waiting").await?;
        let mut stream = f.stream(&waiting.id, &b_cookie, 0).await?;
        assert_eq!(stream.initial, SandboxState::Queued);
        // The busy run ends; with work waiting, the admission task evicts the idle sandbox at once (no idle
        // window, no clock) and gives the slot to the waiting conversation.
        busy_runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "busy".parse()?,
                },
            ))
            .await?;
        until(f, |f| f.backend.created(&waiting.id).is_some()).await?;
        let mut runtime = f.runtime(&waiting).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "waiting".parse()?,
                },
            ))
            .await?;
        // After any queue positions: `creating` (no position), then the registered state, then the run.
        let mut states = Vec::new();
        while states.len() < 3 {
            let (_, frame) = stream.next().await?;
            if let ServerFrame::SessionReady { sandbox, queue, .. } = frame
                && sandbox.state != SandboxState::Queued
            {
                states.push((sandbox.state, queue.is_some()));
            }
        }
        assert_eq!(
            states,
            [
                (SandboxState::Creating, false),
                (SandboxState::Ready, false),
                (SandboxState::Busy, false)
            ]
        );
        f.until_sequence(&waiting.id, 1).await?;
        Ok(())
    })
    .await
}
