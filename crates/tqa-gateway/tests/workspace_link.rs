//! Workspace requests on the runtime link (ADR-0030), with a fake runtime on a microVM install. GW-I-081: the bound
//! on pending requests, a runtime that never answers, a link drop, stray replies, and a client that leaves. GW-I-082:
//! who may start a sandbox for a files request. SEC-026: a reply for another request never affects this one.
#[path = "support/fixture.rs"]
mod support;

use std::time::Duration;

use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, workspace_link as link};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn firecracker(extra: &str) -> String {
    format!(
        "[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n{extra}",
        "b".repeat(64)
    )
}

fn listing(names: &[&str]) -> Value {
    let items: Vec<Value> = names
        .iter()
        .map(|name| link::item(name, "file", 1))
        .collect();
    json!({"kind": "listing", "items": items})
}

fn files(id: &tqa_types::ConversationId) -> String {
    format!("/api/v1/conversations/{id}/files")
}

fn code(answer: &link::Answer) -> Result<(u16, Value)> {
    Ok((answer.status, answer.json()?["error"]["code"].clone()))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_081_pending_requests_are_bounded_per_link(pool: PgPool) -> TestResult {
    let config = firecracker("[files]\nlink_requests=1\n");
    support::run_configured(pool, None, &config, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let first = link::get(f, &files(&conversation.id), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        // One request waits on the link: another is refused at once, retryable.
        let second = link::get(f, &files(&conversation.id), &cookie).await??;
        assert_eq!(code(&second)?, (429, json!("busy")));
        link::reply(&mut runtime, asked, &[listing(&["a"])]).await?;
        assert_eq!(first.await??.status, 200);
        // The slot is free again.
        let third = link::get(f, &files(&conversation.id), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(&mut runtime, asked, &[listing(&[])]).await?;
        assert_eq!(third.await??.status, 200);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_081_a_runtime_that_never_answers_times_out_and_is_told_to_stop(
    pool: PgPool,
) -> TestResult {
    let config = firecracker("[limits]\nio_timeout=5\n");
    support::run_configured(pool, None, &config, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let pending = link::get(f, &files(&conversation.id), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        assert_eq!(link::cancel(&mut runtime).await?, asked);
        let answer = pending.await??;
        assert_eq!(code(&answer)?, (504, json!("timeout")));
        assert_eq!(answer.json()?["error"]["retryable"], true);
        // The late reply is dropped and counted.
        let before = f.gateway.workspace_replies_dropped();
        link::reply(&mut runtime, asked, &[listing(&["late"])]).await?;
        runtime.roundtrip().await?;
        assert_eq!(f.gateway.workspace_replies_dropped(), before + 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_081_a_link_drop_fails_every_pending_request(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let first = link::get(f, &files(&conversation.id), &cookie);
        link::request(&mut runtime).await?;
        let second = link::get(f, &files(&conversation.id), &cookie);
        link::request(&mut runtime).await?;
        drop(runtime);
        for pending in [first, second] {
            assert_eq!(code(&pending.await??)?, (409, json!("sandbox_not_ready")));
        }
        Ok(())
    })
    .await
}

/// Sends `count` parts of `result` for request `asked`, none of them the last.
async fn unfinished(
    runtime: &mut support::runtime::Runtime,
    asked: u64,
    count: u32,
    result: &Value,
) -> TestResult {
    for part in 0..count {
        let frame = json!({"type": "workspace.reply", "request_id": asked, "part": part, "last": false, "result": result});
        runtime
            .send(&tqa_types::ServerFrame::parse(
                frame.to_string().as_bytes(),
            )?)
            .await?;
    }
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_081_parts_past_what_the_request_needs_end_it(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        // A listing fits in 8 parts: a ninth unfinished one ends the request, which the runtime is told to stop.
        let pending = link::get(f, &files(id), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        unfinished(
            &mut runtime,
            asked,
            9,
            &json!({"kind": "listing", "items": []}),
        )
        .await?;
        assert_eq!(link::cancel(&mut runtime).await?, asked);
        assert_eq!(code(&pending.await??)?, (502, json!("upstream_invalid")));
        // A read is one part: a first part that is not the last already ends it.
        let pending = link::get(
            f,
            &format!("/api/v1/conversations/{id}/files/content?path=a"),
            &cookie,
        );
        let (asked, _) = link::request(&mut runtime).await?;
        unfinished(&mut runtime, asked, 1, &link::content("aGVsbG8=", 5, true)).await?;
        assert_eq!(link::cancel(&mut runtime).await?, asked);
        assert_eq!(code(&pending.await??)?, (502, json!("upstream_invalid")));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_081_a_client_that_leaves_cancels_its_request(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let pending = link::get(f, &files(&conversation.id), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        pending.abort();
        assert_eq!(link::cancel(&mut runtime).await?, asked);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_026_replies_for_other_requests_never_affect_a_pending_one(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let other = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stranger = f.runtime(&other).await?;
        let pending = link::get(f, &files(&conversation.id), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        let before = f.gateway.workspace_replies_dropped();
        // Another link answers this request's id, and this link answers ids it was never asked.
        link::reply(&mut stranger, asked, &[listing(&["forged"])]).await?;
        // The other link is read by its own task, so nothing orders its drop against this request's answer: wait until
        // it is counted. This link's replies are then routed in the order they are sent (the strays before the real one).
        tokio::time::timeout(support::DEADLINE, async {
            while f.gateway.workspace_replies_dropped() < before + 1 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        link::reply(&mut runtime, asked + 1_000, &[listing(&["stray"])]).await?;
        link::reply(&mut runtime, 0, &[listing(&["stray"])]).await?;
        link::reply(&mut runtime, asked, &[listing(&["real"])]).await?;
        let page = pending.await??.json()?;
        assert_eq!(page["items"][0]["name"], "real", "{page}");
        assert_eq!(f.gateway.workspace_replies_dropped(), before + 3);
        // An oversized reply to a real request is refused, never shown.
        let pending = link::get(f, &files(&conversation.id), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        let too_many: Vec<String> = (0..201).map(|index| format!("f{index:03}")).collect();
        let names: Vec<&str> = too_many.iter().map(String::as_str).collect();
        link::reply(&mut runtime, asked, &[listing(&names)]).await?;
        assert_eq!(code(&pending.await??)?, (502, json!("upstream_invalid")));
        Ok(())
    })
    .await
}

/// The task's workspace id.
async fn workspace(f: &Fixture, id: &tqa_types::ConversationId) -> Result<uuid::Uuid> {
    Ok(
        sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id=$1")
            .bind(uuid::Uuid::parse_str(id.as_str())?)
            .fetch_one(&f.pool)
            .await?,
    )
}

/// Whether the task has a sandbox record: one was started for it.
async fn started(f: &Fixture, id: &tqa_types::ConversationId) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sandbox_instances WHERE conversation_id=$1)",
    )
    .bind(uuid::Uuid::parse_str(id.as_str())?)
    .fetch_one(&f.pool)
    .await?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_082_only_the_owner_starts_a_sandbox_for_files(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (owner, cookie) = f.admin().await?;
        let (reader, reader_cookie) = f.member("reader@example.invalid").await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        let workspace = workspace(f, id).await?;
        let shared = f
            .client
            .put(f.url(&format!(
                "/api/v1/workspaces/{workspace}/shares/{}",
                reader.id
            )))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .json(&json!({"access": "read"}))
            .send()
            .await?;
        assert_eq!(shared.status(), 200);
        // A read share with no live sandbox is refused, and nothing is started for someone else's task.
        let refused = link::get(f, &files(id), &reader_cookie).await??;
        assert_eq!(code(&refused)?, (409, json!("sandbox_not_ready")));
        assert!(!started(f, id).await?);
        // The owner's request starts the task's sandbox through admission and is served once it is ready.
        let pending = link::get(f, &files(id), &cookie);
        for _ in 0..100 {
            if f.backend.token(id).is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(started(f, id).await?);
        let mut runtime = f.runtime_for(id).await?;
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(&mut runtime, asked, &[listing(&["a"])]).await?;
        assert_eq!(pending.await??.status, 200);
        // With the owner's sandbox live, the read share is served by it.
        let pending = link::get(f, &files(id), &reader_cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(&mut runtime, asked, &[listing(&["a"])]).await?;
        assert_eq!(pending.await??.status, 200);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_082_the_lease_holder_answers_for_its_workspace_and_a_start_is_bounded(
    pool: PgPool,
) -> TestResult {
    let config = firecracker("[sandbox]\nready_seconds=1\n");
    support::run_configured(pool, None, &config, async |f| {
        let (owner, cookie) = f.admin().await?;
        let holder = f.conversation(owner.id).await?;
        let workspace = workspace(f, &holder.id).await?;
        let created = f
            .client
            .post(f.url("/api/v1/conversations"))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .header("idempotency-key", "second-task")
            .json(&json!({"title": "Second", "model_route": "local", "workspace_id": workspace}))
            .send()
            .await?;
        assert_eq!(created.status(), 201);
        let second: tqa_types::ConversationId = created.json::<Value>().await?["id"]
            .as_str()
            .ok_or("id")?
            .parse()?;
        // The holder's live sandbox mounts the workspace: it answers for the second task, which starts nothing.
        let mut runtime = f.runtime(&holder).await?;
        let pending = link::get(f, &files(&second), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(&mut runtime, asked, &[listing(&["shared"])]).await?;
        assert_eq!(pending.await??.json()?["items"][0]["name"], "shared");
        assert!(!started(f, &second).await?);
        // A started sandbox whose runtime never connects is not waited for past the ready bound.
        drop(runtime);
        let fresh = f.conversation(owner.id).await?;
        let refused = link::get(f, &files(&fresh.id), &cookie).await??;
        assert_eq!(code(&refused)?, (409, json!("sandbox_not_ready")));
        Ok(())
    })
    .await
}
