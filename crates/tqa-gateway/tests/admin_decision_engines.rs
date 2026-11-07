//! GW-I-105 (the 11c parts, ADR-0013 point 6): administrators manage decision engines through
//! `/api/v1/admin/decision/engines`. Writes are versioned, validated as the seed is (file-only fields refused), need a
//! secret that allows the engine, and are audited, every exposure change as its own `exposure` row with the old and new
//! exposure; members are refused. A change applies at once. An engine a profile or an active conversation names can't
//! be deleted (disable it instead); a deleted seeded engine stays deleted while its seed entry is unchanged.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SEED: &str = "[[decision_engines]]\nname = \"seeded\"\nurl = \"https://seeded.example\"\n";

fn engine(exposure: &str, version: Option<i64>) -> Value {
    let mut body = json!({"url": "https://engine.example", "auth": {"scheme": "bearer"},
        "key_ref": "secrets/engine", "timeout_ms": 500, "exposure": exposure});
    if let Some(version) = version {
        body["version"] = json!(version);
    }
    body
}

async fn put(f: &Fixture, cookie: &str, name: &str, body: Value) -> Result<(u16, Value)> {
    call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/decision/engines/{name}"),
        cookie,
        (Some(body), None),
    )
    .await
}

/// The engine's audit rows, oldest first: action and detail.
async fn audit(f: &Fixture, name: &str) -> Result<Vec<(String, Option<String>)>> {
    Ok(sqlx::query_as(
        "SELECT action, detail FROM admin_audit WHERE area='decision_engines' AND item=$1 AND actor_kind='admin' ORDER BY at, action",
    )
    .bind(name)
    .fetch_all(&f.pool)
    .await?)
}

async fn create_secret(f: &Fixture, cookie: &str) -> TestResult {
    let (status, body) = call(
        f,
        Method::PUT,
        "/api/v1/admin/secrets/engine",
        cookie,
        (Some(json!({"value": "synthetic"})), None),
    )
    .await?;
    assert_eq!(status, 201, "{body}");
    Ok(())
}

async fn allow(f: &Fixture, cookie: &str) -> TestResult {
    let (status, body) = call(
        f,
        Method::PUT,
        "/api/v1/admin/secrets/engine/consumers",
        cookie,
        (
            Some(json!({"consumers": [{"kind": "engine", "name": "router"}], "version": 1})),
            None,
        ),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(())
}

// TEST: GW-I-105
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_105_engines_are_versioned_bound_and_every_exposure_change_audited(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        // The secret must allow the engine first.
        create_secret(f, &cookie).await?;
        let (status, body) = put(f, &cookie, "router", engine("none", None)).await?;
        assert_eq!(status, 409, "{body}");
        assert_eq!(body["error"]["code"], "secret_not_allowed");
        allow(f, &cookie).await?;
        let mut named = engine("none", None);
        named["display_name"] = json!("Policy engine");
        let (status, created) = put(f, &cookie, "router", named).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(created["display_name"], "Policy engine");
        assert_eq!(
            (
                &created["version"],
                &created["key_missing"],
                &created["seed"]
            ),
            (&json!(1), &json!(false), &Value::Null)
        );
        let (status, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/decision/engines",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        assert_eq!(listed["items"][0]["name"], "router");
        // Widening and narrowing are each their own audit row; a replace without an exposure change is not.
        assert_eq!(
            put(f, &cookie, "router", engine("full", Some(1))).await?.0,
            200
        );
        assert_eq!(
            put(f, &cookie, "router", engine("calls", Some(2))).await?.0,
            200
        );
        let mut slower = engine("calls", Some(3));
        slower["timeout_ms"] = json!(900);
        assert_eq!(put(f, &cookie, "router", slower).await?.0, 200);
        let rows = audit(f, "router").await?;
        let exposures: Vec<_> = rows
            .iter()
            .filter(|(action, _)| action == "exposure")
            .map(|(_, detail)| detail.clone())
            .collect();
        assert_eq!(
            exposures,
            [
                Some("none->full".to_owned()),
                Some("full->calls".to_owned())
            ]
        );
        assert_eq!(
            rows.iter()
                .filter(|(action, _)| action == "replace")
                .count(),
            3
        );
        // A stale version, a file-only field and a member are refused.
        let (status, body) = put(f, &cookie, "router", engine("none", Some(1))).await?;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("conflict")));
        let mut plain = engine("none", Some(4));
        plain["insecure_http"] = json!(true);
        let (status, body) = put(f, &cookie, "router", plain).await?;
        assert_eq!(status, 400, "{body}");
        assert_eq!(body["error"]["details"][0]["reason"], "file_only");
        let (_, member) = f.member("member@example.invalid").await?;
        assert_eq!(
            put(f, &member, "other", json!({"url": "https://x.example"}))
                .await?
                .0,
            403
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-105
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_105_an_install_holds_at_most_16_engines(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        for index in 0..16 {
            let (status, body) = put(
                f,
                &cookie,
                &format!("e{index}"),
                json!({"url": "https://e.example"}),
            )
            .await?;
            assert_eq!(status, 201, "{body}");
        }
        let (status, body) = put(f, &cookie, "e16", json!({"url": "https://e.example"})).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("decision_engine_limit"))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-105
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_105_a_change_applies_at_once_and_an_engine_in_use_is_disabled_not_deleted(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_restarting_with(pool, None, SEED, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let profile = json!({"name":"p","display_name":"P","skills":"all","mcp_servers":[],"connectors":[],
            "rules":[],"limits":{},"decision_engine":"seeded"});
        sqlx::query("UPDATE conversations SET profile=$2, profile_id=$3, profile_version=1 WHERE id=$1::uuid")
            .bind(conversation.id.as_str())
            .bind(&profile)
            .bind(uuid::Uuid::now_v7())
            .execute(&f.pool)
            .await?;
        // In use: deleting is refused with what uses it.
        let path = "/api/v1/admin/decision/engines/seeded";
        let (status, body) = call(f, Method::DELETE, &format!("{path}?version=1"), &cookie, (None, None)).await?;
        assert_eq!(status, 409, "{body}");
        assert_eq!(body["error"]["code"], "decision_engine_in_use");
        assert_eq!(body["error"]["in_use"]["conversations"], 1);
        // Disabling is the soft path, and it applies to the next request.
        let (status, body) = put(
            f,
            &cookie,
            "seeded",
            json!({"url": "https://seeded.example", "enabled": false, "version": 1}),
        )
        .await?;
        assert_eq!(status, 200, "{body}");
        let refused = f
            .action(
                &format!("/api/v1/conversations/{}/messages", conversation.id),
                &cookie,
                Some("after-disable"),
                &json!({"text": "start"}),
            )
            .await?;
        assert_eq!(refused.status(), 409);
        // Once nothing names it, it can be deleted, and an unchanged seed doesn't bring it back.
        sqlx::query("UPDATE conversations SET profile=NULL, profile_id=NULL, profile_version=NULL WHERE id=$1::uuid")
            .bind(conversation.id.as_str())
            .execute(&f.pool)
            .await?;
        let (status, body) = call(f, Method::DELETE, &format!("{path}?version=2"), &cookie, (None, None)).await?;
        assert_eq!(status, 204, "{body}");
        f.restart().await?;
        let (_, listed) = call(f, Method::GET, "/api/v1/admin/decision/engines", &cookie, (None, None)).await?;
        assert_eq!(listed["items"], json!([]), "a deleted seeded engine stays deleted");
        Ok(())
    }))
    .await
}
