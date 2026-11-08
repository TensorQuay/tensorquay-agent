//! GW-I-030: a workspace over its quota refuses new runs, and retention deletes only long-unused workspaces
//! that no active task is bound to (and nothing at all by default). SBX-C-015's retention clause: expired workspaces
//! are deleted and logged (the ages are set in the store, standing in for a fake clock).
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::ConversationId;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn workspace(f: &Fixture, cookie: &str, name: &str) -> Result<String> {
    let created: Value = call(f, cookie, Method::POST, "/api/v1/workspaces")
        .json(&json!({"name":name}))
        .send()
        .await?
        .json()
        .await?;
    Ok(created["id"].as_str().ok_or("id")?.to_owned())
}

async fn task(f: &Fixture, cookie: &str, key: &str, workspace: &str) -> Result<ConversationId> {
    let created: Value = call(f, cookie, Method::POST, "/api/v1/conversations")
        .header("idempotency-key", key)
        .json(&json!({"title":"Task","model_route":"local","workspace_id":workspace}))
        .send()
        .await?
        .json()
        .await?;
    Ok(created["id"].as_str().ok_or("id")?.parse()?)
}

async fn retire_all(f: &Fixture) -> TestResult {
    f.clock.freeze();
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            let live: i64 = sqlx::query_scalar("SELECT count(*) FROM sandbox_instances")
                .fetch_one(&f.pool)
                .await?;
            if live == 0 {
                return Ok::<(), Box<dyn std::error::Error>>(());
            }
            f.clock.advance(std::time::Duration::from_mins(10));
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await?
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_a_workspace_over_its_quota_refuses_new_runs(pool: PgPool) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[workspaces]\nquota_bytes=1048576\n",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let id = workspace(f, &cookie, "Growing").await?;
            let conversation = task(f, &cookie, "grow", &id).await?;
            let _runtime = f.runtime_for(&conversation).await?;
            let directory = f.backend.mounted(&conversation).ok_or("mounted")?;
            std::fs::write(directory.join("big.bin"), vec![1_u8; 2 * 1024 * 1024])?;
            // The size is measured when the sandbox lets go of the workspace.
            retire_all(f).await?;
            // Measured in the background after the release: wait (bounded) for it.
            let view = tokio::time::timeout(support::DEADLINE, async {
                loop {
                    let view: Value =
                        call(f, &cookie, Method::GET, &format!("/api/v1/workspaces/{id}"))
                            .send()
                            .await?
                            .json()
                            .await?;
                    if !view["size_bytes"].is_null() {
                        return Ok::<Value, Box<dyn std::error::Error>>(view);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await??;
            assert_eq!(view["size_bytes"], 2 * 1024 * 1024);
            assert!(view["size_measured_at"].is_string());
            let refused = f
                .action(
                    &format!("/api/v1/conversations/{conversation}/messages"),
                    &cookie,
                    Some("over-quota"),
                    &json!({"text":"keep going"}),
                )
                .await?;
            assert_eq!(refused.status(), 409);
            let body: Value = refused.json().await?;
            assert_eq!(body["error"]["code"], "workspace_quota");
            let live: i64 = sqlx::query_scalar("SELECT count(*) FROM sandbox_instances")
                .fetch_one(&f.pool)
                .await?;
            assert_eq!(live, 0, "no sandbox starts on a workspace over its quota");
            // Back under the quota (files removed and measured again), runs start again.
            std::fs::remove_file(directory.join("big.bin"))?;
            sqlx::query("UPDATE workspaces SET size_bytes=0 WHERE id=$1::uuid")
                .bind(&id)
                .execute(&f.pool)
                .await?;
            let accepted = f
                .action(
                    &format!("/api/v1/conversations/{conversation}/messages"),
                    &cookie,
                    Some("under-quota"),
                    &json!({"text":"continue"}),
                )
                .await?;
            assert_eq!(accepted.status(), 202);
            Ok(())
        },
    )
    .await
}

async fn age(f: &Fixture, workspace: &str, days: i32) -> TestResult {
    sqlx::query("UPDATE workspaces SET last_used_at=CURRENT_TIMESTAMP-make_interval(days=>$2) WHERE id=$1::uuid")
        .bind(workspace)
        .bind(days)
        .execute(&f.pool)
        .await?;
    Ok(())
}

async fn state(f: &Fixture, workspace: &str) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM workspaces WHERE id=$1::uuid")
            .bind(workspace)
            .fetch_one(&f.pool)
            .await?,
    )
}

// TEST: SBX-C-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_retention_deletes_only_long_unused_workspaces_without_active_tasks(
    pool: PgPool,
) -> TestResult {
    let capture = log_capture::Capture::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .finish(),
    )?;
    support::run_configured(pool, None, "[workspaces]\nretention_days=30\n", async |f| {
        let (_, cookie) = f.admin().await?;
        let stale = workspace(f, &cookie, "Stale").await?;
        let bound = workspace(f, &cookie, "Bound").await?;
        let recent = workspace(f, &cookie, "Recent").await?;
        let conversation = task(f, &cookie, "bound-task", &bound).await?;
        let _runtime = f.runtime_for(&conversation).await?;
        let root = f
            .backend
            .mounted(&conversation)
            .ok_or("mounted")?
            .parent()
            .ok_or("root")?
            .to_path_buf();
        retire_all(f).await?;
        age(f, &stale, 31).await?;
        age(f, &bound, 31).await?;
        age(f, &recent, 29).await?;
        // Held, so the deleted row can be seen before the purge removes it.
        f.gateway.purge_hold(true);
        f.gateway.workspace_retention_now().await?;
        assert_eq!(state(f, &stale).await?, "deleted");
        assert!(
            !root.join(&stale).exists(),
            "the stale workspace's files are removed"
        );
        let audited: Vec<(String, Option<uuid::Uuid>, String)> = sqlx::query_as(
            "SELECT actor_kind, actor_user_id, action FROM admin_audit WHERE area='workspaces' AND item=$1",
        )
        .bind(&stale)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(audited, [("system".to_owned(), None, "delete".to_owned())], "retention's delete is the system's");
        f.gateway.purge_now().await?;
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE id=$1::uuid")
            .bind(&stale)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(rows, 0, "the purge removes the retired workspace's row");
        assert_eq!(
            state(f, &bound).await?,
            "ready",
            "an active task keeps its workspace"
        );
        assert_eq!(state(f, &recent).await?, "ready");
        let deleted = capture
            .text()?
            .lines()
            .filter(|line| line.contains("unused workspace deleted by retention"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(deleted.len(), 1, "one deletion, logged once: {deleted:?}");
        assert!(
            deleted[0].contains("INFO") && deleted[0].contains(&stale),
            "logged with the workspace id: {deleted:?}"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_retention_is_off_by_default(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let old = workspace(f, &cookie, "Old").await?;
        age(f, &old, 3650).await?;
        f.gateway.workspace_retention_now().await?;
        assert_eq!(
            state(f, &old).await?,
            "ready",
            "nobody loses files by surprise"
        );
        Ok(())
    })
    .await
}
