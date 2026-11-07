//! GW-I-136 (ADR-0037 §7, the second release): a profile's `repositories` and a workspace's `git_url` are removed. A
//! request that still sends either is refused naming the field, with what to do instead, never ignored: ignoring
//! `git_url` would give a task an empty workspace where its clone was expected. Tasks clone and push with plain git and
//! a key under Keys.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn post(f: &Fixture, cookie: &str, path: &str, body: Value) -> Result<(u16, Value)> {
    call(f, Method::POST, path, cookie, (Some(body), None)).await
}

/// The refusal's status, code, field and reason, after checking its message says what to do.
fn refused(status: u16, body: &Value) -> (u16, Value, Value, Value) {
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.ends_with("was removed in this release: clone with plain git and a key under Keys"),
        "{body}"
    );
    let detail = &body["error"]["details"][0];
    (
        status,
        body["error"]["code"].clone(),
        detail["field"].clone(),
        detail["reason"].clone(),
    )
}

// TEST: GW-I-136
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_136_a_profile_that_still_sets_repositories_is_refused_by_name(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        for (repositories, shared) in [(true, true), (false, true), (true, false)] {
            let body = json!({"name": "release", "display_name": "Release", "shared": shared,
                "repositories": repositories});
            let (status, body) = post(f, &admin, "/api/v1/profiles", body).await?;
            assert_eq!(
                refused(status, &body),
                (
                    400,
                    json!("invalid_request"),
                    json!("repositories"),
                    json!("removed")
                ),
                "{repositories} {shared}"
            );
        }
        // Without it, the same profile is made, and its read has no such field.
        let body = json!({"name": "release", "display_name": "Release", "shared": true});
        let (status, created) = post(f, &admin, "/api/v1/profiles", body).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(created.get("repositories"), None, "{created}");
        Ok(())
    })
    .await
}

// TEST: GW-I-136
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_136_a_workspace_from_a_git_url_is_refused_by_name(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let body =
            json!({"name": "Report", "git_url": "https://git.example.invalid/team/report.git"});
        let (status, body) = post(f, &member, "/api/v1/workspaces", body).await?;
        assert_eq!(
            refused(status, &body),
            (
                400,
                json!("invalid_request"),
                json!("git_url"),
                json!("removed")
            )
        );
        let listed: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(listed, 0, "nothing made");
        // An empty workspace is made as before, and its read has no source.
        let (status, created) =
            post(f, &member, "/api/v1/workspaces", json!({"name": "Report"})).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(created.get("source_url"), None, "{created}");
        Ok(())
    })
    .await
}
