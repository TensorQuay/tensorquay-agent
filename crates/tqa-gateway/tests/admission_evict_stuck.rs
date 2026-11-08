//! GW-I-310 (a live check before 17.0, 18 Oct 2026): an eviction whose destroy fails doesn't stick. On a loaded engine
//! the evicted sandbox's destroy timed out, and so did the start-failure cleanup after it; the slot stayed with its
//! sandbox unconfirmed, the remembered eviction kept "in flight" for as long as that slot existed, and no other sandbox
//! was evicted for anyone: a waiting task got in only when an unrelated sandbox idled out, 6.5 minutes later, and the
//! failed one's row stayed `destroying` with its container long gone. Now the failed destroy is retried in the
//! background, a sandbox found gone is released, and meanwhile the next pass evicts another idle sandbox.
#[path = "support/fixture.rs"]
mod support;

use serde_json::json;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, Event, QueueReason, SandboxState, ServerFrame};
use uuid::Uuid;

/// One slot short of three idle sandboxes, and an idle window far beyond the test: only an eviction frees a slot.
const CONFIG: &str = "[sandbox]\nidle_seconds=3600\nmax_live=4\n[admission]\nslots=2\nper_user=2";

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

/// A conversation of `owner` with a live sandbox whose one run has ended: idle, so evictable.
async fn idle(
    f: &Fixture,
    owner: uuid::Uuid,
    run: &str,
) -> Result<(tqa_gateway::Conversation, support::runtime::Runtime), Box<dyn std::error::Error>> {
    let conversation = f.conversation(owner).await?;
    let mut runtime = f.runtime(&conversation).await?;
    runtime
        .send(&lifecycle(
            1,
            Event::RunStarted {
                run_id: run.parse()?,
            },
        ))
        .await?;
    runtime
        .send(&lifecycle(
            2,
            Event::RunCompleted {
                run_id: run.parse()?,
            },
        ))
        .await?;
    f.until_sequence(&conversation.id, 2).await?;
    Ok((conversation, runtime))
}

/// A Desktop template, and `conversation` bound to it.
async fn desktop(f: &Fixture, conversation: &ConversationId) -> TestResult {
    sqlx::query("INSERT INTO templates (name,display_name,image,capabilities,is_default,version,last_set_by) VALUES ('desktop','Desktop',$1,'{shell,browser,desktop}',false,1,'admin') ON CONFLICT (name) DO NOTHING")
        .bind(format!("sha256:{}", "4".repeat(64)))
        .execute(&f.pool)
        .await?;
    sqlx::query("UPDATE conversations SET template='desktop' WHERE id=$1")
        .bind(Uuid::parse_str(conversation.as_str())?)
        .execute(&f.pool)
        .await?;
    Ok(())
}

async fn message(f: &Fixture, id: &ConversationId, cookie: &str) -> TestResult {
    let accepted = f
        .action(
            &format!("/api/v1/conversations/{id}/messages"),
            cookie,
            Some("waiter"),
            &json!({"text": "work"}),
        )
        .await?;
    assert_eq!(accepted.status(), 202);
    Ok(())
}

/// The sandbox rows still being destroyed.
async fn destroying(f: &Fixture) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM sandbox_instances WHERE phase = 'destroying'")
            .fetch_one(&f.pool)
            .await?,
    )
}

// TEST: GW-I-310
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_310_a_failed_eviction_doesnt_stick_the_waiter_starts_and_nothing_is_left_destroying(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (owner, cookie) = f.admin().await?;
        // Two idle sandboxes fill both slots; the first is the least recently active, so the first to go.
        let (first, _first_runtime) = idle(f, owner.id, "first").await?;
        let (_second, _second_runtime) = idle(f, owner.id, "second").await?;
        // The idle clock moves only when the test says, so a teardown's background retries come on its advances.
        f.clock.freeze();
        // A loaded engine: the next two destroys time out, yet each removal completes after (the 16o case).
        f.backend.destroy_lies.store(true, Ordering::SeqCst);
        f.backend.fail_destroys.store(2, Ordering::SeqCst);
        let waiter = f.conversation(owner.id).await?;
        message(f, &waiter.id, &cookie).await?;
        // A few seconds of the idle clock at a time, never near the idle window: only evictions and their retries
        // can make room. Before the fix nothing more happened after the two failed destroys.
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.created(&waiter.id).is_none() {
                f.clock.advance(Duration::from_secs(5));
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| "the waiter never started: the failed eviction stuck")?;
        assert!(
            !f.backend.list_conversations().await?.contains(&first.id),
            "the first victim is gone from the engine"
        );
        // Its row and slot are released once its removal is found complete: no phantom 'destroying' row.
        tokio::time::timeout(support::DEADLINE, async {
            while destroying(f).await.unwrap_or(1) > 0 {
                f.clock.advance(Duration::from_secs(5));
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| "a failed eviction's row stayed 'destroying'")?;
        Ok(())
    })
    .await
}

// TEST: GW-I-310
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_310_a_desktop_waiter_reads_waiting_for_a_desktop_through_a_failed_eviction_and_then_starts(
    pool: PgPool,
) -> TestResult {
    let config =
        "[sandbox]\nidle_seconds=3600\n[admission]\nslots=4\nper_user=4\n[desktop]\nmax_sessions=1";
    support::run_configured(pool, None, config, async |f| {
        let (owner, cookie) = f.admin().await?;
        // One Desktop, finished, its run-end reset clean: it may give way (GW-I-305).
        let first = f.conversation(owner.id).await?;
        desktop(f, &first.id).await?;
        let mut runtime = f.runtime(&first).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "r-1".parse()?,
                },
            ))
            .await?;
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "r-1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&first.id, 2).await?;
        runtime
            .send(&ServerFrame::ScreenReset {
                ok: true,
                ready: true,
            })
            .await?;
        f.clock.freeze();
        f.backend.destroy_lies.store(true, Ordering::SeqCst);
        f.backend.fail_destroys.store(2, Ordering::SeqCst);
        let second = f.conversation(owner.id).await?;
        desktop(f, &second.id).await?;
        message(f, &second.id, &cookie).await?;
        // The eviction is asked for, and its destroy fails.
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.destroys.load(Ordering::SeqCst) < 1 {
                f.gateway.sandboxes().admission_pass_now().await?;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .await??;
        f.gateway.sandboxes().admission_pass_now().await?;
        // Waiting for a free desktop, said as such: never "starting" for an eviction that failed.
        let stream = f.stream(&second.id, &cookie, 0).await?;
        assert_eq!(
            (stream.initial, stream.initial_reason),
            (SandboxState::Queued, Some(QueueReason::DesktopSessions))
        );
        // The failed destroy is retried; the removal is found complete, the desktop released, and the waiter starts.
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.created(&second.id).is_none() {
                f.clock.advance(Duration::from_secs(5));
                f.gateway.sandboxes().admission_pass_now().await?;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .await
        .map_err(|_| "the desktop waiter never started: the failed eviction stuck")??;
        assert_eq!(destroying(f).await?, 0, "no row left destroying");
        Ok(())
    })
    .await
}
