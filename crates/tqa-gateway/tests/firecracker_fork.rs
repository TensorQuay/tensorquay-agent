//! GW-I-064: on a microVM install a fork copies the workspace's block image (ARCHITECTURE §4.6) without mounting it:
//! byte for byte, sparse, private, sized by its allocation; a workspace never started forks empty; a fork taken while
//! a sandbox runs is marked inconsistent; one over the quota fails and leaves nothing.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    io::{Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
};
use support::{Fixture, TestResult};
use tqa_types::ConversationId;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn firecracker(extra: &str) -> String {
    format!(
        "[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n{extra}",
        "b".repeat(64)
    )
}

async fn call(
    f: &Fixture,
    cookie: &str,
    method: Method,
    path: &str,
    body: Value,
) -> Result<(u16, Value)> {
    let response = f
        .client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", uuid::Uuid::now_v7().to_string())
        .json(&body)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await.unwrap_or(Value::Null)))
}

async fn workspace(f: &Fixture, cookie: &str) -> Result<String> {
    let (status, body) = call(
        f,
        cookie,
        Method::POST,
        "/api/v1/workspaces",
        json!({"name":"Source"}),
    )
    .await?;
    assert_eq!(status, 201, "{body}");
    Ok(body["id"].as_str().ok_or("id")?.to_owned())
}

fn image(f: &Fixture, workspace: &str) -> PathBuf {
    f.config_storage_root()
        .join("workspaces")
        .join(format!("{workspace}.ext4"))
}

/// A 32 MiB sparse image with 2 MiB of data, as a started workspace leaves behind.
fn write_image(path: &PathBuf) -> TestResult {
    let mut file = std::fs::File::create_new(path)?;
    file.set_len(32 << 20)?;
    file.seek(SeekFrom::Start(8 << 20))?;
    file.write_all(&vec![7; 2 << 20])?;
    file.sync_all()?;
    Ok(())
}

/// Forks `source` and reads the fork until it leaves `forking` (bounded); returns the 202 body and the settled one.
async fn fork(f: &Fixture, cookie: &str, source: &str) -> Result<(Value, Value)> {
    let path = format!("/api/v1/workspaces/{source}/fork");
    let (status, started) = call(f, cookie, Method::POST, &path, json!({"name":"Fork"})).await?;
    assert_eq!(status, 202, "{started}");
    let id = started["id"].as_str().ok_or("fork id")?.to_owned();
    let settled = tokio::time::timeout(support::DEADLINE, async {
        loop {
            let path = format!("/api/v1/workspaces/{id}");
            let (_, body) = call(f, cookie, Method::GET, &path, Value::Null).await?;
            if body["state"] != "forking" {
                return Ok::<Value, Box<dyn std::error::Error>>(body);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await??;
    Ok((started, settled))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_064_a_fork_copies_the_image_without_a_mount(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (_, cookie) = f.admin().await?;
        let source = workspace(f, &cookie).await?;
        write_image(&image(f, &source))?;
        let (started, settled) = fork(f, &cookie, &source).await?;
        assert_eq!(started["fork"], json!({"from":source,"consistent":true}));
        assert_eq!(settled["state"], "ready", "{settled}");
        let copy = image(f, settled["id"].as_str().ok_or("id")?);
        assert_eq!(std::fs::read(&copy)?, std::fs::read(image(f, &source))?);
        let metadata = std::fs::metadata(&copy)?;
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            settled["size_bytes"],
            json!(metadata.blocks() * 512),
            "sized by its allocation"
        );
        assert!(metadata.blocks() * 512 < 8 << 20, "sparse");
        assert!(
            !copy.with_extension("").exists(),
            "no workspace directory on the host"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_064_a_workspace_never_started_forks_empty(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (_, cookie) = f.admin().await?;
        let source = workspace(f, &cookie).await?;
        let (_, settled) = fork(f, &cookie, &source).await?;
        assert_eq!(settled["state"], "ready", "{settled}");
        assert_eq!(settled["size_bytes"], json!(0));
        let copy = image(f, settled["id"].as_str().ok_or("id")?);
        assert!(
            !copy.exists() && !copy.with_extension("").exists(),
            "nothing on the host"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_064_a_fork_of_a_running_workspace_is_marked_inconsistent(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (_, cookie) = f.admin().await?;
        let source = workspace(f, &cookie).await?;
        write_image(&image(f, &source))?;
        let body = json!({"title":"Task","model_route":"local","workspace_id":source});
        let (status, task) = call(f, &cookie, Method::POST, "/api/v1/conversations", body).await?;
        assert_eq!(status, 201, "{task}");
        let id: ConversationId = task["id"].as_str().ok_or("task id")?.parse()?;
        f.gateway.sandboxes().ensure(&id).await?;
        let (started, settled) = fork(f, &cookie, &source).await?;
        assert_eq!(
            started["fork"]["consistent"], false,
            "the source had a live sandbox"
        );
        assert_eq!(settled["state"], "ready", "{settled}");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_064_a_fork_over_the_quota_fails_and_leaves_nothing(pool: PgPool) -> TestResult {
    let config = firecracker("[workspaces]\nquota_bytes=1048576\n");
    support::run_configured(pool, None, &config, async |f| {
        let (_, cookie) = f.admin().await?;
        let source = workspace(f, &cookie).await?;
        write_image(&image(f, &source))?;
        let (_, settled) = fork(f, &cookie, &source).await?;
        assert_eq!(settled["state"], "failed", "{settled}");
        let copy = image(f, settled["id"].as_str().ok_or("id")?);
        assert!(
            !copy.exists() && !copy.with_extension("").exists(),
            "nothing left behind"
        );
        let left: Vec<_> = std::fs::read_dir(copy.parent().ok_or("parent")?)?
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
            .collect();
        assert_eq!(left.len(), 1, "only the source image: {left:?}");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_064_a_source_image_that_is_a_link_is_never_followed(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (_, cookie) = f.admin().await?;
        let source = workspace(f, &cookie).await?;
        let elsewhere = tempfile::tempdir()?;
        let target = elsewhere.path().join("other.ext4");
        write_image(&target)?;
        std::os::unix::fs::symlink(&target, image(f, &source))?;
        let (_, settled) = fork(f, &cookie, &source).await?;
        assert_eq!(settled["state"], "failed", "{settled}");
        assert!(!image(f, settled["id"].as_str().ok_or("id")?).exists());
        Ok(())
    })
    .await
}
