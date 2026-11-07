//! GW-I-068 (ADR-0031 decision 7, the ADR-0028 pattern): connectors are administered through
//! `/api/v1/admin/connectors`: versions, idempotent retries, audit rows without values, `key_missing`, the shared name
//! namespace, the file-only fields, the connector limit, administrator access, and the `[[connectors]]` seed under the
//! seed-change rule.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn detail(body: &Value) -> (Value, Value) {
    let detail = &body["error"]["details"][0];
    (detail["field"].clone(), detail["reason"].clone())
}

fn connector(url: &str) -> Value {
    json!({"url": url, "auth": {"scheme": "bearer"}})
}

const STDIO: &str = "[[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\n";

async fn put(f: &Fixture, cookie: &str, name: &str, body: Value) -> Result<(u16, Value)> {
    call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/connectors/{name}"),
        cookie,
        (Some(body), None),
    )
    .await
}

async fn listed(f: &Fixture, cookie: &str) -> Result<Vec<Value>> {
    let (status, body) = call(
        f,
        Method::GET,
        "/api/v1/admin/connectors",
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"].as_array().ok_or("items")?.clone())
}

// TEST: GW-I-068
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_068_connectors_are_versioned_retried_audited_and_live_at_once(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let path = "/api/v1/admin/connectors/tracker";
        let mut body = connector("https://mcp.tracker.example/mcp");
        body["display_name"] = json!("Issue tracker");
        body["secret_ref"] = json!("secrets/tracker");
        let (status, created) = call(
            f,
            Method::PUT,
            path,
            &cookie,
            (Some(body.clone()), Some("create")),
        )
        .await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(
            (
                &created["name"],
                &created["display_name"],
                &created["version"],
                &created["seed"],
                &created["last_set_by"]
            ),
            (
                &json!("tracker"),
                &json!("Issue tracker"),
                &json!(1),
                &json!(null),
                &json!("admin")
            )
        );
        assert_eq!(
            (created.get("class"), &created["key_missing"]),
            (None, &json!(true))
        );
        let (status, replayed) = call(
            f,
            Method::PUT,
            path,
            &cookie,
            (Some(body.clone()), Some("create")),
        )
        .await?;
        assert_eq!(
            (status, &replayed),
            (201, &created),
            "a retry replays the stored response"
        );
        // The secret gets a value: key_missing clears, and no value ever appears.
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/secrets/tracker",
            &cookie,
            (Some(json!({"value": "synthetic-tracker"})), None),
        )
        .await?;
        assert_eq!(status, 201);
        let items = listed(f, &cookie).await?;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["key_missing"], false);
        assert!(!serde_json::to_string(&items)?.contains("synthetic-tracker"));
        // Replace the version read; a stale version is a conflict.
        let mut replace = connector("https://mcp.tracker.example/v2/mcp");
        replace["version"] = json!(1);
        let (status, replaced) = call(
            f,
            Method::PUT,
            path,
            &cookie,
            (Some(replace.clone()), Some("replace")),
        )
        .await?;
        assert_eq!(
            (status, &replaced["version"], &replaced["url"]),
            (200, &json!(2), &json!("https://mcp.tracker.example/v2/mcp"))
        );
        let (status, stale) = call(
            f,
            Method::PUT,
            path,
            &cookie,
            (Some(replace), Some("stale")),
        )
        .await?;
        assert_eq!((status, &stale["error"]["code"]), (409, &json!("conflict")));
        let (status, exists) =
            call(f, Method::PUT, path, &cookie, (Some(body), Some("exists"))).await?;
        assert_eq!(
            (status, &exists["error"]["code"]),
            (409, &json!("conflict")),
            "a create never replaces"
        );
        // Delete the version read.
        let (status, _) = call(
            f,
            Method::DELETE,
            &format!("{path}?version=1"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 409);
        let (status, _) = call(
            f,
            Method::DELETE,
            &format!("{path}?version=2"),
            &cookie,
            (None, Some("delete")),
        )
        .await?;
        assert_eq!(status, 204);
        assert!(listed(f, &cookie).await?.is_empty());
        let (status, _) = call(
            f,
            Method::DELETE,
            &format!("{path}?version=2"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 404);
        let audit: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT action,before_version,after_version FROM admin_audit \
             WHERE area='connectors' AND actor_user_id=$1 AND item='tracker' ORDER BY at, id",
        )
        .bind(admin.id)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(
            audit,
            vec![
                ("create".into(), None, Some(1)),
                ("replace".into(), Some(1), Some(2)),
                ("delete".into(), Some(2), None)
            ]
        );
        // Members cannot administer connectors.
        let (_, member) = f.member("member@example.invalid").await?;
        let (status, _) = call(
            f,
            Method::GET,
            "/api/v1/admin/connectors",
            &member,
            (None, None),
        )
        .await?;
        assert_eq!(status, 403);
        Ok(())
    })
    .await
}

// TEST: GW-I-068
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_068_the_api_refuses_what_the_validator_refuses(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, STDIO, async |f| {
        let (_, cookie) = f.admin().await?;
        let (status, body) = put(f, &cookie, "docs", connector("https://docs.example/mcp")).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("name_taken")),
            "a stdio server holds the name"
        );
        let (status, body) = put(f, &cookie, "Bad", connector("https://mcp.example/mcp")).await?;
        assert_eq!(
            (status, detail(&body)),
            (400, (json!("name"), json!("name_invalid")))
        );
        let mut insecure = connector("http://mcp.internal/mcp");
        insecure["insecure_http"] = json!(true);
        let (status, body) = put(f, &cookie, "local", insecure).await?;
        assert_eq!(
            (status, detail(&body)),
            (400, (json!("insecure_http"), json!("file_only")))
        );
        let (status, body) = put(
            f,
            &cookie,
            "leaky",
            connector("https://mcp.example/mcp?token=1"),
        )
        .await?;
        assert_eq!(
            (status, detail(&body)),
            (400, (json!("url"), json!("auth_query_invalid")))
        );
        let mut tools = connector("https://mcp.example/mcp");
        tools["tools"] = json!({"allow": [], "deny": []});
        let (status, body) = put(f, &cookie, "tools", tools).await?;
        assert_eq!(
            (status, detail(&body)),
            (400, (json!("tools"), json!("tool_filter_invalid")))
        );
        for bad in [
            json!({"name": "x", "url": "https://mcp.example/mcp"}),
            json!({"url": "https://mcp.example/mcp", "extra": 1}),
            json!([1]),
        ] {
            let (status, body) = put(f, &cookie, "body", bad).await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!("body"), json!("body_invalid")))
            );
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-068
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_068_an_install_holds_at_most_64_connectors(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        for index in 0..64 {
            let path = format!("/api/v1/admin/connectors/c{index}");
            let (status, _) = call(
                f,
                Method::PUT,
                &path,
                &cookie,
                (Some(connector("https://mcp.example/mcp")), None),
            )
            .await?;
            assert_eq!(status, 201, "{index}");
        }
        let (status, body) = call(
            f,
            Method::PUT,
            "/api/v1/admin/connectors/c64",
            &cookie,
            (Some(connector("https://mcp.example/mcp")), None),
        )
        .await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("connector_limit"))
        );
        // Replacing one of the 64 is still allowed.
        let mut replace = connector("https://mcp.example/v2");
        replace["version"] = json!(1);
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/connectors/c0",
            &cookie,
            (Some(replace), None),
        )
        .await?;
        assert_eq!(status, 200);
        Ok(())
    })
    .await
}

async fn provenance(pool: &PgPool, name: &str) -> Result<Option<(i64, String, Option<String>)>> {
    let row = sqlx::query("SELECT version,last_set_by,seed_state FROM connectors WHERE name=$1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(match row {
        Some(row) => Some((row.try_get(0)?, row.try_get(1)?, row.try_get(2)?)),
        None => None,
    })
}

fn seed(url: &str) -> String {
    format!("[[connectors]]\nname='tracker'\nurl='{url}'\nauth={{scheme='bearer'}}\n")
}

// TEST: GW-I-068
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_068_the_seed_applies_only_when_it_changed_and_a_deleted_seed_stays_deleted(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, &seed("https://one.example/mcp"), async |f| {
        let (_, cookie) = f.admin().await?;
        assert_eq!(
            provenance(&f.pool, "tracker").await?,
            Some((1, "seed".into(), Some("current".into())))
        );
        // An administrator's change survives a restart with the same seed.
        let mut change = connector("https://admin.example/mcp");
        change["version"] = json!(1);
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/connectors/tracker",
            &cookie,
            (Some(change), None),
        )
        .await?;
        assert_eq!(status, 200);
        f.restart().await?;
        let items = listed(f, &cookie).await?;
        assert_eq!(
            (&items[0]["url"], &items[0]["last_set_by"]),
            (&json!("https://admin.example/mcp"), &json!("admin"))
        );
        // An edited seed wins at the next start.
        f.reconfigure(&seed("https://two.example/mcp"))?;
        f.restart().await?;
        assert_eq!(
            provenance(&f.pool, "tracker").await?,
            Some((3, "seed".into(), Some("current".into())))
        );
        // A deleted seeded connector stays deleted while the seed is unchanged, and returns when it changes.
        let (status, _) = call(
            f,
            Method::DELETE,
            "/api/v1/admin/connectors/tracker?version=3",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 204);
        f.restart().await?;
        assert_eq!(provenance(&f.pool, "tracker").await?, None);
        f.reconfigure(&seed("https://three.example/mcp"))?;
        f.restart().await?;
        assert_eq!(
            provenance(&f.pool, "tracker").await?,
            Some((1, "seed".into(), Some("current".into())))
        );
        // Removed from the file: kept and marked.
        f.reconfigure("")?;
        f.restart().await?;
        assert_eq!(
            provenance(&f.pool, "tracker").await?,
            Some((1, "seed".into(), Some("removed".into())))
        );
        Ok(())
    })
    .await
}
