//! GW-I-003/004 and GW-S-021: ownership applies to metadata, event streams and actions.

#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_004_creation_binds_a_known_route_and_a_delete_takes_the_runtime_token(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let wrong = fixture
            .action(
                "/api/v1/conversations",
                &cookie,
                Some("create-draft"),
                &json!({"title":"Draft", "model_route":"missing"}),
            )
            .await?;
        assert_eq!(wrong.status(), 403);
        let response = fixture
            .action(
                "/api/v1/conversations",
                &cookie,
                Some("create-draft"),
                &json!({"title":"Draft", "model_route":"local"}),
            )
            .await?;
        assert_eq!(response.status(), 201);
        let record: serde_json::Value = response.json().await?;
        assert_eq!(record["backend"], "docker");
        assert_eq!(record["owner_id"], owner.id.to_string());
        assert!(record.get("runtime_token").is_none());
        assert!(record.get("workspace_dir").is_none());
        let id = record["id"]
            .as_str()
            .ok_or("missing conversation id")?
            .to_owned();
        let route = format!("/api/v1/conversations/{id}");
        let response = fixture
            .client
            .delete(fixture.url(&route))
            .header("origin", fixture.origin())
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(response.status(), 204);
        let count = sqlx::query_scalar!(
            "SELECT count(*) FROM runtime_tokens WHERE conversation_id=$1",
            uuid::Uuid::parse_str(id.as_str())?
        )
        .fetch_one(&fixture.pool)
        .await?;
        assert_eq!(count, Some(0));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_003_other_members_and_administrators_cannot_read_or_act_on_owned_events(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (_, admin_cookie) = fixture.admin().await?;
        let (owner, owner_cookie) = fixture.member("owner@example.invalid").await?;
        let (_, other_cookie) = fixture.member("other@example.invalid").await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        for cookie in [&admin_cookie, &other_cookie] {
            assert_eq!(
                fixture
                    .get(&format!("/api/v1/conversations/{id}/events"), cookie)
                    .await?
                    .status(),
                403
            );
            assert_eq!(
                fixture
                    .get(
                        &format!("/api/v1/conversations/{id}/events?stream=1&protocol=1"),
                        cookie
                    )
                    .await?
                    .status(),
                403
            );
            let response = fixture
                .action(
                    &format!("/api/v1/conversations/{id}/messages"),
                    cookie,
                    Some("action-one"),
                    &json!({"text":"hello", "attachments":[]}),
                )
                .await?;
            assert_eq!(response.status(), 403);
        }
        assert_eq!(
            fixture
                .get("/api/v1/conversations?all=true", &other_cookie)
                .await?
                .status(),
            403
        );
        let response = fixture
            .get("/api/v1/conversations?all=true", &admin_cookie)
            .await?;
        assert_eq!(response.status(), 200);
        let response = fixture
            .action(
                &format!("/api/v1/conversations/{id}/messages"),
                &owner_cookie,
                None,
                &json!({"text":"hello", "attachments":[]}),
            )
            .await?;
        assert_eq!(response.status(), 400);
        Ok(())
    })
    .await
}
