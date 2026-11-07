//! ADR-0039 slice 5: Sign out everywhere, the gateway's side.
//! - GW-I-222 (sign-out): `POST .../screen/sign-out` answers `202` and records the outcome as a screen notice: with no
//!   runtime linked at once (`screen.signed_out`, nothing to clear); with one linked, after asking it
//!   (`screen.sign_out`) and on its answer, `screen.signed_out` when it emptied the profile, `screen.sign_out_failed`
//!   with `not_cleared` when it couldn't, `unconfirmed` when it didn't answer within 10 s. A retry with the same key
//!   records nothing more. Refused `409 no_desktop`, `409 run_active` while a run is active, `403` for another person's
//!   task and `404` for a missing one.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_types::{ClientFrame, ConversationId, Event, ServerFrame, SignOutFailure};
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

async fn sign_out(
    f: &Fixture,
    id: &str,
    cookie: &str,
    key: &str,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    call(
        f,
        Method::POST,
        &format!("/api/v1/conversations/{id}/screen/sign-out"),
        cookie,
        (Some(json!({})), Some(key)),
    )
    .await
}

/// The task's notices once `count` are stored, bounded past the gateway's 30 s wait for the runtime's answer (which
/// comes once the browser has started again, GW-U-204).
async fn notices(
    f: &Fixture,
    id: &str,
    cookie: &str,
    count: usize,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    tokio::time::timeout(support::DEADLINE * 4, async {
        loop {
            let (_, body) = call(
                f,
                Method::GET,
                &format!("/api/v1/conversations/{id}/screen/notices"),
                cookie,
                (None, None),
            )
            .await?;
            let items = body["items"].as_array().cloned().unwrap_or_default();
            if items.len() >= count {
                return Ok::<_, Box<dyn std::error::Error>>(items);
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await?
}

fn kinds(items: &[Value]) -> Vec<(Value, Value)> {
    items
        .iter()
        .map(|item| (item["kind"].clone(), item["reason"].clone()))
        .collect()
}

// TEST: GW-I-222
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_222_with_no_runtime_linked_the_sign_out_is_done_at_once(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let (status, body) = sign_out(f, task.id.as_str(), &cookie, "first").await?;
        assert_eq!(status, 202, "{body}");
        let items = notices(f, task.id.as_str(), &cookie, 1).await?;
        assert_eq!(kinds(&items), [(json!("screen.signed_out"), Value::Null)]);
        // A retry with the same key answers the same and records nothing more.
        let (status, _) = sign_out(f, task.id.as_str(), &cookie, "first").await?;
        assert_eq!(status, 202);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(notices(f, task.id.as_str(), &cookie, 1).await?.len(), 1);
        Ok(())
    })
    .await
}

// TEST: GW-I-222
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_222_a_linked_runtime_is_asked_and_its_answer_is_the_notice(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        let (status, _) = sign_out(f, task.id.as_str(), &cookie, "ok").await?;
        assert_eq!(status, 202);
        assert_eq!(runtime.action().await?, ClientFrame::ScreenSignOut {});
        runtime
            .send(&ServerFrame::ScreenSignOutDone {
                ok: true,
                reason: None,
                ready: true,
            })
            .await?;
        let items = notices(f, task.id.as_str(), &cookie, 1).await?;
        assert_eq!(kinds(&items), [(json!("screen.signed_out"), Value::Null)]);
        let (status, _) = sign_out(f, task.id.as_str(), &cookie, "busy").await?;
        assert_eq!(status, 202);
        assert_eq!(runtime.action().await?, ClientFrame::ScreenSignOut {});
        runtime
            .send(&ServerFrame::ScreenSignOutDone {
                ok: false,
                reason: Some(SignOutFailure::ProfileInUse),
                ready: true,
            })
            .await?;
        let items = notices(f, task.id.as_str(), &cookie, 2).await?;
        assert_eq!(
            kinds(&items)[1],
            (json!("screen.sign_out_failed"), json!("not_cleared"))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-222
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_222_a_runtime_that_never_answers_leaves_it_unconfirmed(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        let (status, _) = sign_out(f, task.id.as_str(), &cookie, "silent").await?;
        assert_eq!(status, 202);
        assert_eq!(runtime.action().await?, ClientFrame::ScreenSignOut {});
        // No answer: after 30 s the notice says it couldn't be confirmed.
        let items = notices(f, task.id.as_str(), &cookie, 1).await?;
        assert_eq!(
            kinds(&items),
            [(json!("screen.sign_out_failed"), json!("unconfirmed"))]
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-222
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_222_a_sign_out_is_refused_without_a_desktop_during_a_run_and_on_others_tasks(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (_, other_cookie) = f.member("other@example.invalid").await?;
        let plain = f.conversation(owner.id).await?;
        let (status, body) = sign_out(f, plain.id.as_str(), &cookie, "plain").await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("no_desktop")),
            "{body}"
        );
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 1.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-10-05T12:00:00Z".into(),
                event: Event::RunStarted {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&task.id, 1).await?;
        let (status, body) = sign_out(f, task.id.as_str(), &cookie, "running").await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("run_active")),
            "{body}"
        );
        let (status, body) = sign_out(f, task.id.as_str(), &other_cookie, "other").await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (403, &json!("forbidden")),
            "{body}"
        );
        let (status, _) = sign_out(f, &Uuid::now_v7().to_string(), &cookie, "missing").await?;
        assert_eq!(status, 404);
        Ok(())
    })
    .await
}
