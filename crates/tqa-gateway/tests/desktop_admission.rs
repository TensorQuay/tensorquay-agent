//! ADR-0039 slice 2: the Desktop in admission and in its run limits.
//! - GW-I-224: with `desktop.max_sessions` at 1 and a desktop sandbox live, another desktop task waits queued with the
//!   reason `desktop_sessions` while a task on another environment still starts; it starts once the cap rises, and
//!   once the live Desktop idles out. A Desktop whose run-end reset this gateway hasn't seen isn't retired for it: a
//!   sign-in may be in its browser (GW-I-305 has it give way once its profile is known empty). A desktop task's
//!   `session.config` carries 500 steps, another task's the install's own.
//! - GW-I-228: a desktop task's `session.config` carries the `computer` tool's caps (200 screenshots and 1,000 actions
//!   by default), another task's none; an administrator's change reaches the next session.
#[path = "support/fixture.rs"]
mod support;

use std::sync::atomic::Ordering;

use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_types::{ConversationId, QueueReason, SandboxState};
use uuid::Uuid;

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

async fn until(condition: impl Fn() -> bool) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

// TEST: GW-I-224
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_224_a_desktop_past_the_cap_waits_for_a_free_desktop(pool: PgPool) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[admission]\nslots=4\nper_user=4\n[desktop]\nmax_sessions=1",
        async |f| {
            let (user, cookie) = f.admin().await?;
            let first = f.conversation(user.id).await?;
            desktop(f, &first.id).await?;
            let _first = f.runtime(&first).await?;
            let second = f.conversation(user.id).await?;
            desktop(f, &second.id).await?;
            message(f, &second.id, &cookie, "second").await?;
            let plain = f.conversation(user.id).await?;
            message(f, &plain.id, &cookie, "plain").await?;
            f.gateway.sandboxes().admission_pass_now().await?;
            let stream = f.stream(&second.id, &cookie, 0).await?;
            assert_eq!(
                (stream.initial, stream.initial_reason),
                (SandboxState::Queued, Some(QueueReason::DesktopSessions))
            );
            // Another environment isn't held back by the Desktop's cap.
            until(|| f.backend.created(&plain.id).is_some()).await?;
            assert!(f.backend.created(&second.id).is_none());
            // The cap raised: the second Desktop starts on the next pass.
            let (status, body) = call(
                f,
                Method::PUT,
                "/api/v1/admin/limits/desktop.max_sessions",
                &cookie,
                (Some(json!({"value": 2, "version": 1})), None),
            )
            .await?;
            assert_eq!(status, 200, "{body}");
            f.gateway.sandboxes().admission_pass_now().await?;
            until(|| f.backend.created(&second.id).is_some()).await
        },
    )
    .await
}

// TEST: GW-I-224
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_224_a_waiting_desktop_starts_once_another_desktop_ends(pool: PgPool) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[admission]\nslots=4\nper_user=4\n[desktop]\nmax_sessions=1",
        async |f| {
            let (user, cookie) = f.admin().await?;
            let first = f.conversation(user.id).await?;
            desktop(f, &first.id).await?;
            let _first = f.runtime(&first).await?;
            let second = f.conversation(user.id).await?;
            desktop(f, &second.id).await?;
            message(f, &second.id, &cookie, "second").await?;
            // An idle Desktop whose run-end reset this gateway hasn't seen isn't retired for the waiter: a sign-in may be
            // in its browser (ADR-0039 §10, amended 17 Oct 2026; GW-I-305).
            f.gateway.sandboxes().admission_pass_now().await?;
            f.gateway.sandboxes().admission_pass_now().await?;
            assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
            assert!(f.backend.created(&second.id).is_none());
            // The first Desktop idles out: its sandbox ends, and the waiter starts on the next pass.
            let held = Uuid::parse_str(first.id.as_str())?;
            f.clock.freeze();
            tokio::time::timeout(support::DEADLINE, async {
                loop {
                    let live: i64 = sqlx::query_scalar(
                        "SELECT count(*) FROM sandbox_instances WHERE conversation_id=$1",
                    )
                    .bind(held)
                    .fetch_one(&f.pool)
                    .await?;
                    if live == 0 {
                        return Ok::<(), Box<dyn std::error::Error>>(());
                    }
                    f.clock.advance(std::time::Duration::from_mins(10));
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await??;
            f.gateway.sandboxes().admission_pass_now().await?;
            until(|| f.backend.created(&second.id).is_some()).await
        },
    )
    .await
}

// TEST: GW-I-224
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_224_a_desktop_task_gets_500_steps(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let plain = f.conversation(user.id).await?;
        let runtime = f.runtime(&plain).await?;
        assert_eq!(runtime.config["limits"]["max_steps"], json!(100));
        let on_desktop = f.conversation(user.id).await?;
        desktop(f, &on_desktop.id).await?;
        let runtime = f.runtime(&on_desktop).await?;
        assert_eq!(runtime.config["limits"]["max_steps"], json!(500));
        Ok(())
    })
    .await
}

// TEST: GW-I-228
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_228_a_desktop_session_carries_the_computer_caps(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let plain = f.conversation(user.id).await?;
        let runtime = f.runtime(&plain).await?;
        assert!(
            runtime
                .config
                .get("computer_limits")
                .is_none_or(serde_json::Value::is_null)
        );
        let on_desktop = f.conversation(user.id).await?;
        desktop(f, &on_desktop.id).await?;
        let runtime = f.runtime(&on_desktop).await?;
        assert_eq!(
            runtime.config["computer_limits"],
            json!({"max_screenshots_per_run": 200, "max_actions_per_run": 1000})
        );
        let (status, body) = call(
            f,
            Method::PUT,
            "/api/v1/admin/limits/computer.max_actions_per_run",
            &cookie,
            (Some(json!({"value": 50, "version": 1})), None),
        )
        .await?;
        assert_eq!(status, 200, "{body}");
        let next = f.conversation(user.id).await?;
        desktop(f, &next.id).await?;
        let runtime = f.runtime(&next).await?;
        assert_eq!(
            runtime.config["computer_limits"],
            json!({"max_screenshots_per_run": 200, "max_actions_per_run": 50})
        );
        Ok(())
    })
    .await
}
