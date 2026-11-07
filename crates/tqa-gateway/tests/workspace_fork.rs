//! GW-I-030 and SEC-014 (066c): a fork is an owned, independent copy that never follows links, is marked when taken
//! from a running workspace, and fails within the quota instead of filling the disk.
#[path = "support/fixture.rs"]
mod support;

use reqwest::{Method, Response};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use support::{Fixture, TestResult};
use tqa_types::ConversationId;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn json(response: Response) -> Result<(u16, Value)> {
    let status = response.status().as_u16();
    let text = response.text().await?;
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

async fn workspace(f: &Fixture, cookie: &str, name: &str) -> Result<String> {
    let (status, body) = json(
        call(f, cookie, Method::POST, "/api/v1/workspaces")
            .json(&json!({"name":name}))
            .send()
            .await?,
    )
    .await?;
    assert_eq!(status, 201, "{body}");
    Ok(body["id"].as_str().ok_or("id")?.to_owned())
}

async fn task(f: &Fixture, cookie: &str, key: &str, workspace: &str) -> Result<(u16, Value)> {
    json(
        call(f, cookie, Method::POST, "/api/v1/conversations")
            .header("idempotency-key", key)
            .json(&json!({"title":"Task","model_route":"local","workspace_id":workspace}))
            .send()
            .await?,
    )
    .await
}

async fn fork(f: &Fixture, cookie: &str, source: &str, name: &str) -> Result<(u16, Value)> {
    json(
        call(
            f,
            cookie,
            Method::POST,
            &format!("/api/v1/workspaces/{source}/fork"),
        )
        .json(&json!({"name":name}))
        .send()
        .await?,
    )
    .await
}

/// Reads the workspace until it leaves `forking` (bounded).
async fn settled(f: &Fixture, cookie: &str, id: &str) -> Result<Value> {
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            let (status, body) = json(
                call(f, cookie, Method::GET, &format!("/api/v1/workspaces/{id}"))
                    .send()
                    .await?,
            )
            .await?;
            assert_eq!(status, 200, "{body}");
            if body["state"] != "forking" {
                return Ok::<Value, Box<dyn std::error::Error>>(body);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await?
}

/// A task on `workspace` with a live, connected sandbox (idle retirement needs the link); returns the host
/// directory the sandbox mounts and the runtime, which the caller keeps alive.
async fn running(
    f: &Fixture,
    cookie: &str,
    key: &str,
    workspace: &str,
) -> Result<(PathBuf, support::runtime::Runtime)> {
    let (status, body) = task(f, cookie, key, workspace).await?;
    assert_eq!(status, 201, "{body}");
    let id: ConversationId = body["id"].as_str().ok_or("id")?.parse()?;
    let runtime = f.runtime_for(&id).await?;
    Ok((f.backend.mounted(&id).ok_or("mounted")?, runtime))
}

/// Retires every sandbox through the idle window on the injected clock, so no lease is held.
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

fn sibling(directory: &Path, workspace: &str) -> Result<PathBuf> {
    Ok(directory.parent().ok_or("workspaces root")?.join(workspace))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_a_fork_is_an_independent_copy_owned_by_the_forker(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, owner) = f.admin().await?;
        let (reader, cookie) = f.member("reader@example.invalid").await?;
        let source = workspace(f, &owner, "Source").await?;
        let (directory, _runtime) = running(f, &owner, "source-task", &source).await?;
        std::fs::write(directory.join("a.txt"), "alpha")?;
        std::fs::create_dir(directory.join("nested"))?;
        std::fs::write(directory.join("nested/b.txt"), "beta")?;
        let outside = tempfile::tempdir()?;
        std::fs::write(
            outside.path().join("outside.txt"),
            "not part of the workspace",
        )?;
        std::os::unix::fs::symlink(outside.path().join("outside.txt"), directory.join("link"))?;
        retire_all(f).await?;
        let share = call(
            f,
            &owner,
            Method::PUT,
            &format!("/api/v1/workspaces/{source}/shares/{}", reader.id),
        )
        .json(&json!({"access":"read"}))
        .send()
        .await?;
        assert_eq!(share.status(), 200);
        // A read share may fork; the fork belongs to the forker.
        let (status, started) = fork(f, &cookie, &source, "Copy").await?;
        assert_eq!(status, 202, "{started}");
        assert_eq!(
            (&started["owner_id"], &started["access"]),
            (&json!(reader.id), &json!("owner"))
        );
        assert_eq!(started["fork"], json!({"from":source,"consistent":true}));
        let copy = started["id"].as_str().ok_or("id")?.to_owned();
        let done = settled(f, &cookie, &copy).await?;
        assert_eq!(done["state"], "ready", "{done}");
        assert_eq!(done["size_bytes"], 9, "regular file bytes only");
        let forked = sibling(&directory, &copy)?;
        assert_eq!(std::fs::read_to_string(forked.join("a.txt"))?, "alpha");
        assert_eq!(
            std::fs::read_to_string(forked.join("nested/b.txt"))?,
            "beta"
        );
        // A link is copied as a link to the same target, never followed.
        assert!(
            std::fs::symlink_metadata(forked.join("link"))?
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(forked.join("link"))?,
            outside.path().join("outside.txt")
        );
        // Independent: changing the copy leaves the source alone.
        std::fs::write(forked.join("a.txt"), "changed")?;
        assert_eq!(std::fs::read_to_string(directory.join("a.txt"))?, "alpha");
        // The forker's copy is not shared back with the source's owner.
        let (status, _) = json(
            call(
                f,
                &owner,
                Method::GET,
                &format!("/api/v1/workspaces/{copy}"),
            )
            .send()
            .await?,
        )
        .await?;
        assert_eq!(status, 403);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_a_fork_taken_while_a_sandbox_runs_is_marked_inconsistent(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, owner) = f.admin().await?;
        let source = workspace(f, &owner, "Busy").await?;
        let (directory, _runtime) = running(f, &owner, "busy-task", &source).await?;
        std::fs::write(directory.join("work.txt"), "in progress")?;
        let (status, started) = fork(f, &owner, &source, "Snapshot").await?;
        assert_eq!(status, 202, "{started}");
        assert_eq!(
            started["fork"]["consistent"], false,
            "the source had a live sandbox"
        );
        let copy = started["id"].as_str().ok_or("id")?.to_owned();
        assert_eq!(settled(f, &owner, &copy).await?["state"], "ready");
        assert_eq!(
            std::fs::read_to_string(sibling(&directory, &copy)?.join("work.txt"))?,
            "in progress"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_014_a_fork_needs_access_and_a_ready_source(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        let source = workspace(f, &admin, "Private").await?;
        // No share, no fork (administrators included: their role gives metadata only).
        assert_eq!(fork(f, &stranger, &source, "Mine").await?.0, 403);
        let (_, member) = f.member("owner@example.invalid").await?;
        let owned = workspace(f, &member, "Member's").await?;
        assert_eq!(fork(f, &admin, &owned, "Admin copy").await?.0, 403);
        assert_eq!(fork(f, &admin, &source, "").await?.0, 400);
        // A workspace still being forked (or failed) can neither be forked nor bound nor run on.
        sqlx::query("UPDATE workspaces SET state='forking' WHERE id=$1::uuid")
            .bind(&source)
            .execute(&f.pool)
            .await?;
        let (status, body) = fork(f, &admin, &source, "Too early").await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("workspace_not_ready"))
        );
        let (status, body) = task(f, &admin, "early-task", &source).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("workspace_not_ready"))
        );
        sqlx::query("UPDATE workspaces SET state='failed' WHERE id=$1::uuid")
            .bind(&source)
            .execute(&f.pool)
            .await?;
        let (status, body) = task(f, &admin, "failed-task", &source).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("workspace_not_ready"))
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_a_fork_over_the_quota_fails_and_leaves_nothing_behind(
    pool: PgPool,
) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[workspaces]\nquota_bytes=1048576\n",
        async |f| {
            let (_, owner) = f.admin().await?;
            let source = workspace(f, &owner, "Large").await?;
            let (directory, _runtime) = running(f, &owner, "large-task", &source).await?;
            std::fs::write(directory.join("big.bin"), vec![7_u8; 1_572_864])?;
            retire_all(f).await?;
            let (status, started) = fork(f, &owner, &source, "Too big").await?;
            assert_eq!(status, 202, "{started}");
            let copy = started["id"].as_str().ok_or("id")?.to_owned();
            assert_eq!(settled(f, &owner, &copy).await?["state"], "failed");
            assert!(
                !sibling(&directory, &copy)?.join("big.bin").exists(),
                "a failed fork keeps no partial copy"
            );
            Ok(())
        },
    )
    .await
}
