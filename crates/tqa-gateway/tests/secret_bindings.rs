//! A shared secret is used only by the consumers it allows (ADR-0033).
//! - SEC-032: an admin write naming an unbound secret is refused `409 secret_not_allowed`; a consumer edited into the
//!   store around the API fails closed when it reads; a file's route or connector binds its secret (the seed).
//! - GW-I-096: an upgrade binds each secret to exactly the consumers that used it, so nothing stops working; a secret
//!   created afterwards allows none until bound; bindings are versioned and audited.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const CONNECTORS: &str = "[[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\n\
    auth={scheme='bearer'}\nsecret_ref='secrets/tracker-shared'\n";

fn route(key_ref: &str) -> Value {
    json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "m",
           "auth": {"kind": "bearer"}, "key_ref": key_ref})
}

fn connector(secret_ref: &str) -> Value {
    json!({"url": "https://mcp.other.example/mcp", "auth": {"scheme": "bearer"}, "secret_ref": secret_ref})
}

async fn secret(f: &Fixture, cookie: &str, name: &str) -> Result<()> {
    let (status, body) = call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/secrets/{name}"),
        cookie,
        (Some(json!({"value": "synthetic"})), None),
    )
    .await?;
    assert_eq!(status, 201, "{body}");
    Ok(())
}

async fn bind(
    f: &Fixture,
    cookie: &str,
    name: &str,
    consumers: Value,
    version: i64,
) -> Result<(u16, Value)> {
    call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/secrets/{name}/consumers"),
        cookie,
        (
            Some(json!({"consumers": consumers, "version": version})),
            None,
        ),
    )
    .await
}

fn code(body: &Value) -> Value {
    body["error"]["code"].clone()
}

// TEST: SEC-032
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_032_a_route_or_connector_may_use_only_a_secret_bound_to_it(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, CONNECTORS, async |f| {
        let (_, cookie) = f.admin().await?;
        secret(f, &cookie, "provider").await?;
        // Unbound: refused on write, route and connector alike.
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/routes/keyed", &cookie, (Some(route("secrets/provider")), None)).await?;
        assert_eq!((status, code(&body)), (409, json!("secret_not_allowed")), "{body}");
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/connectors/other", &cookie, (Some(connector("secrets/provider")), None)).await?;
        assert_eq!((status, code(&body)), (409, json!("secret_not_allowed")), "{body}");
        // Bound: accepted.
        let (status, item) = bind(f, &cookie, "provider", json!([{"kind": "route", "name": "keyed"}, {"kind": "connector", "name": "other"}]), 1).await?;
        assert_eq!(status, 200, "{item}");
        assert_eq!(item["consumers"], json!([{"kind": "connector", "name": "other"}, {"kind": "route", "name": "keyed"}]));
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/routes/keyed", &cookie, (Some(route("secrets/provider")), None)).await?;
        assert_eq!(status, 201, "{body}");
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/connectors/other", &cookie, (Some(connector("secrets/provider")), None)).await?;
        assert_eq!(status, 201, "{body}");
        // A secret that does not exist yet is bound to the route that names it (nothing to protect yet), and an
        // administrator who later sets its value sees the binding.
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/routes/early", &cookie, (Some(route("secrets/later")), None)).await?;
        assert_eq!(status, 201, "{body}");
        let (status, created) = call(
            f,
            Method::PUT,
            "/api/v1/admin/secrets/later",
            &cookie,
            (Some(json!({"value": "synthetic"})), None),
        )
        .await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(created["consumers"], json!([{"kind": "route", "name": "early"}]));
        // The file's connector bound its secret when it was seeded.
        let seeded: Vec<(String, String)> = sqlx::query_as(
            "SELECT consumer, bound_by FROM secret_consumers WHERE ref='secrets/tracker-shared'",
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(seeded, [("tracker".to_owned(), "seed".to_owned())]);
        let (bob, _) = f.member("bob@example.invalid").await?;
        secret(f, &cookie, "tracker-shared").await?;
        assert!(f.gateway.connector_credential(bob.id, "tracker").await?.is_some());
        // Around the API: the file's connector pointed at a secret nobody bound. Its read fails closed.
        secret(f, &cookie, "elsewhere").await?;
        sqlx::query("UPDATE connectors SET document = jsonb_set(document, '{secret_ref}', '\"secrets/elsewhere\"') WHERE name='tracker'")
            .execute(&f.pool)
            .await?;
        sqlx::query("UPDATE settings_generation SET generation = generation + 1 WHERE id = 1")
            .execute(&f.pool)
            .await?;
        f.restart().await?;
        let refused = f.gateway.connector_credential(bob.id, "tracker").await;
        assert!(
            matches!(refused, Err(tqa_gateway::GatewayError::SecretUnavailable)),
            "a read for an unbound consumer fails closed: {:?}",
            refused.as_ref().err()
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-096
#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_096_an_upgrade_binds_each_secret_to_the_consumers_that_used_it(
    pool: PgPool,
) -> TestResult {
    // The schema as it was before this change (every migration older than the secret consumers one, since later ones
    // build on it), with a route and a connector already using secrets.
    let all = sqlx::migrate!("./migrations");
    let change = all
        .migrations
        .iter()
        .find(|migration| migration.description.contains("secret consumers"))
        .map(|migration| migration.version)
        .ok_or("no secret consumers migration")?;
    let before = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.migrations
                .iter()
                .filter(|migration| migration.version < change)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    before.run(&pool).await?;
    sqlx::query(
        "INSERT INTO model_routes (name,document) VALUES ('old', $1), ('free', $2)",
    )
    .bind(json!({"name": "old", "protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "m",
                 "auth": {"kind": "bearer"}, "key_ref": "secrets/provider"}))
    .bind(json!({"name": "free", "protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "m",
                 "auth": {"kind": "none"}}))
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO connectors (name,document) VALUES ('legacy', $1)")
        .bind(
            json!({"url": "https://mcp.legacy.example/mcp", "auth": {"scheme": "bearer"},
                     "secret_ref": "secrets/provider"}),
        )
        .execute(&pool)
        .await?;
    all.run(&pool).await?;
    let bound: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT ref, consumer_kind, consumer, bound_by FROM secret_consumers ORDER BY consumer_kind, consumer",
    )
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        bound,
        [
            (
                "secrets/provider".into(),
                "connector".into(),
                "legacy".into(),
                "upgrade".into()
            ),
            (
                "secrets/provider".into(),
                "route".into(),
                "old".into(),
                "upgrade".into()
            ),
        ]
    );
    Ok(())
}

// TEST: GW-I-096
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_096_a_new_secret_allows_none_and_bindings_are_versioned_and_audited(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        secret(f, &cookie, "fresh").await?;
        let (status, listed) = call(f, Method::GET, "/api/v1/admin/secrets", &cookie, (None, None)).await?;
        assert_eq!(status, 200);
        let item = listed["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|item| item["ref"] == "secrets/fresh")
            .cloned()
            .ok_or("listed")?;
        assert_eq!(item["consumers"], json!([]), "a new secret allows none");
        assert_eq!(bind(f, &cookie, "fresh", json!([{"kind": "route", "name": "a"}]), 7).await?.0, 409);
        let (status, item) = bind(f, &cookie, "fresh", json!([{"kind": "route", "name": "a"}]), 1).await?;
        assert_eq!((status, &item["version"]), (200, &json!(2)), "{item}");
        let audited: (String, bool) = sqlx::query_as(
            "SELECT action, value_hash_changed FROM admin_audit WHERE item='secrets/fresh' ORDER BY at DESC, id DESC LIMIT 1",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(audited, ("replace".to_owned(), false));
        let (status, item) = bind(f, &cookie, "fresh", json!([]), 2).await?;
        assert_eq!((status, &item["consumers"]), (200, &json!([])));
        Ok(())
    })
    .await
}
