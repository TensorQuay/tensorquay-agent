//! GW-I-004: creating a conversation is one owner-scoped, atomic idempotent write.
#[path = "support/fixture.rs"]
mod support;

use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

const ROUTE: &str = "/api/v1/conversations";

async fn create(
    f: &Fixture,
    cookie: &str,
    key: Option<&str>,
    title: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    f.action(
        ROUTE,
        cookie,
        key,
        &json!({"title": title, "model_route": "local"}),
    )
    .await
}

async fn count(f: &Fixture, cookie: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let value: Value = f.get(ROUTE, cookie).await?.json().await?;
    Ok(value["items"].as_array().ok_or("missing items")?.len())
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_004_create_replays_the_entire_response_and_rejects_a_changed_body(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let first = create(f, &cookie, Some("create-one"), "One task").await?;
        assert_eq!(first.status(), 201);
        let first: Value = first.json().await?;
        let second = create(f, &cookie, Some("create-one"), "One task").await?;
        assert_eq!(second.status(), 201);
        assert_eq!(second.json::<Value>().await?, first);
        assert_eq!(count(f, &cookie).await?, 1);
        let changed = create(f, &cookie, Some("create-one"), "Another task").await?;
        assert_eq!(changed.status(), 422);
        assert_eq!(
            changed.json::<Value>().await?["error"]["code"],
            "idempotency_key_reused"
        );
        assert_eq!(count(f, &cookie).await?, 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_004_concurrent_create_and_owner_scoping(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, other) = f.member("other@example.invalid").await?;
        let (one, two) = tokio::try_join!(
            create(f, &cookie, Some("shared-key"), "Concurrent task"),
            create(f, &cookie, Some("shared-key"), "Concurrent task")
        )?;
        assert_eq!(one.status(), 201);
        assert_eq!(two.status(), 201);
        let one: Value = one.json().await?;
        assert_eq!(two.json::<Value>().await?, one);
        assert_eq!(count(f, &cookie).await?, 1);
        let foreign = create(f, &other, Some("shared-key"), "Concurrent task").await?;
        assert_eq!(foreign.status(), 201);
        assert_ne!(foreign.json::<Value>().await?["id"], one["id"]);
        assert_eq!(count(f, &other).await?, 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_004_missing_malformed_and_cross_route_keys_do_not_create(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        for key in [None, Some(""), Some("has spaces"), Some(&"a".repeat(129))] {
            assert_eq!(create(f, &cookie, key, "Invalid key").await?.status(), 400);
        }
        let duplicated = f
            .client
            .post(f.url(ROUTE))
            .header("cookie", &cookie)
            .header("origin", f.origin())
            .header("idempotency-key", "one")
            .header("idempotency-key", "two")
            .json(&json!({"title": "Duplicate key", "model_route": "local"}))
            .send()
            .await?;
        assert_eq!(duplicated.status(), 400);
        assert_eq!(count(f, &cookie).await?, 0);
        let record: Value = create(f, &cookie, Some("route-key"), "Task")
            .await?
            .json()
            .await?;
        let id = record["id"].as_str().ok_or("missing id")?;
        let response = f
            .action(
                &format!("{ROUTE}/{id}/messages"),
                &cookie,
                Some("route-key"),
                &json!({"text":"hello"}),
            )
            .await?;
        assert_eq!(response.status(), 409);
        assert_eq!(
            response.json::<Value>().await?["error"]["code"],
            "idempotency_route"
        );
        assert_eq!(count(f, &cookie).await?, 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_004_create_rolls_back_when_the_key_record_cannot_commit(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe("ALTER TABLE idempotency ADD CONSTRAINT reject_fixture_key CHECK (key <> 'rejected-key')")).execute(&f.pool).await?;
        assert_eq!(create(f, &cookie, Some("rejected-key"), "Must roll back").await?.status(), 503);
        assert_eq!(count(f, &cookie).await?, 0);
        sqlx::raw_sql(sqlx::AssertSqlSafe("ALTER TABLE idempotency DROP CONSTRAINT reject_fixture_key")).execute(&f.pool).await?;
        let response = create(f, &cookie, Some("rejected-key"), "Must roll back").await?;
        assert_eq!(response.status(), 201);
        assert_eq!(count(f, &cookie).await?, 1);
        Ok(())
    }).await
}
