//! GW-I-054 (090, ADR-0028): shared secrets are administered by name through `/api/v1/admin/secrets`; values are
//! write-only, writes are versioned, need a signed-in session, may be retried with an Idempotency-Key, and are audited.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{TestResult, call};

fn detail(body: &Value) -> (Value, Value) {
    let detail = &body["error"]["details"][0];
    (detail["field"].clone(), detail["reason"].clone())
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_054_values_are_write_only_versioned_and_audited(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let path = "/api/v1/admin/secrets/provider";
        let (status, created) = call(f, Method::PUT, path, &cookie, (Some(json!({"value": " first "})), None)).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(
            created,
            json!({"ref": "secrets/provider", "updated_at": created["updated_at"], "version": 1, "seed": null,
                   "last_set_by": "admin", "routes": [], "rotated_at": created["updated_at"],
                   "rotate_after_days": null, "rotation_due": false, "consumers": []})
        );
        let stored = f.gateway.secret_store().get(&"secrets/provider".parse()?).await?.ok_or("stored")?;
        assert_eq!(stored.expose_secret(), " first ", "whitespace is kept exactly");
        let replace = |version: Value| Some(json!({"value": "second", "version": version}));
        assert_eq!(call(f, Method::PUT, path, &cookie, (replace(json!(1)), None)).await?.0, 200);
        for body in [replace(json!(1)), Some(json!({"value": "third"}))] {
            let (status, body) = call(f, Method::PUT, path, &cookie, (body, None)).await?;
            assert_eq!((status, &body["error"]["code"]), (409, &json!("conflict")), "a stale or missing version");
        }
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/secrets/absent", &cookie, (replace(json!(1)), None)).await?;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("conflict")), "a version for a secret that does not exist");
        let (status, list) = call(f, Method::GET, "/api/v1/admin/secrets", &cookie, (None, None)).await?;
        assert_eq!((status, &list["items"][0]["version"], &list["limit"]), (200, &json!(2), &json!(4096)));
        assert!(!list.to_string().contains("first") && !list.to_string().contains("second"));
        assert_eq!(call(f, Method::DELETE, &format!("{path}?version=1"), &cookie, (None, None)).await?.0, 409);
        assert_eq!(call(f, Method::DELETE, &format!("{path}?version=2"), &cookie, (None, None)).await?.0, 204);
        assert_eq!(call(f, Method::DELETE, &format!("{path}?version=2"), &cookie, (None, None)).await?.0, 404);
        let audit: Vec<(String, Option<i64>, Option<i64>, bool)> = sqlx::query_as(
            "SELECT action,before_version,after_version,value_hash_changed FROM admin_audit \
             WHERE actor_user_id=$1 AND item='secrets/provider' ORDER BY at, id",
        )
        .bind(admin.id)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(audit, vec![("create".into(), None, Some(1), true), ("replace".into(), Some(1), Some(2), true),
                               ("delete".into(), Some(2), None, true)]);
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_054_writes_need_an_administrator_session_and_valid_input(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let value = || Some(json!({"value": "v"}));
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/secrets/name",
            &member,
            (value(), None),
        )
        .await?;
        assert_eq!(status, 403);
        let (status, token) = call(
            f,
            Method::POST,
            "/api/v1/tokens",
            &cookie,
            (Some(json!({"name":"admin","expires_in_days":1})), None),
        )
        .await?;
        assert_eq!(status, 201);
        let bearer = |method: Method, path: &str| {
            f.client
                .request(method, f.url(path))
                .bearer_auth(token["token"].as_str().unwrap_or_default())
        };
        assert_eq!(
            bearer(Method::GET, "/api/v1/admin/secrets")
                .send()
                .await?
                .status(),
            200
        );
        let response = bearer(Method::PUT, "/api/v1/admin/secrets/name")
            .json(&json!({"value": "v"}))
            .send()
            .await?;
        assert_eq!(response.status(), 403);
        assert_eq!(
            response.json::<Value>().await?["error"]["code"],
            "session_required"
        );
        let long = "x".repeat(8193);
        for (name, body, field, reason) in [
            ("bad.name", json!({"value": "v"}), "name", "name_invalid"),
            (
                &"n".repeat(121),
                json!({"value": "v"}),
                "name",
                "name_invalid",
            ),
            ("git-abc", json!({"value": "v"}), "name", "reserved_prefix"),
            ("name", json!({"value": ""}), "value", "value_empty"),
            ("name", json!({"value": long}), "value", "value_too_long"),
            ("name", json!({"value": "a\u{0}b"}), "value", "value_nul"),
            (
                "name",
                json!({"value": "v", "extra": 1}),
                "body",
                "body_invalid",
            ),
            (
                "name",
                json!({"value": "v", "version": 0}),
                "version",
                "version_invalid",
            ),
        ] {
            let (status, body) = call(
                f,
                Method::PUT,
                &format!("/api/v1/admin/secrets/{name}"),
                &cookie,
                (Some(body), None),
            )
            .await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!(field), json!(reason))),
                "{name}"
            );
        }
        let (status, body) = call(
            f,
            Method::DELETE,
            "/api/v1/admin/secrets/name?force=1",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            (status, detail(&body)),
            (400, (json!("query"), json!("parameter_invalid")))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_054_used_limited_retried_and_private_secrets(pool: PgPool) -> TestResult {
    let registry = tqa_llm::Registry::from_toml(
        "[[routes]]\nname='priced'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='m'\nauth={kind='bearer'}\nkey_ref='secrets/in-use'",
    )?;
    support::run_configured(
        pool,
        Some(registry),
        "[secret_store]\nreferences=3\n",
        async |f| {
            let (_, cookie) = f.admin().await?;
            f.gateway
                .secret_store()
                .put(&"secrets/git-0123".parse()?, "private".into())
                .await?;
            let put = async |name: &str, key: Option<&'static str>, value: &str| {
                let path = format!("/api/v1/admin/secrets/{name}");
                call(
                    f,
                    Method::PUT,
                    &path,
                    &cookie,
                    (Some(json!({"value": value})), key),
                )
                .await
            };
            assert_eq!(put("in-use", None, "v").await?.0, 201);
            let (status, list) = call(
                f,
                Method::GET,
                "/api/v1/admin/secrets",
                &cookie,
                (None, None),
            )
            .await?;
            assert_eq!(
                (status, &list["items"]),
                (200, &json!([list["items"][0].clone()])),
                "git- references are never listed"
            );
            assert_eq!(list["items"][0]["routes"], json!(["priced"]));
            let (status, body) = call(
                f,
                Method::DELETE,
                "/api/v1/admin/secrets/in-use?version=1",
                &cookie,
                (None, None),
            )
            .await?;
            assert_eq!(
                (status, &body["error"]["code"]),
                (409, &json!("secret_in_use"))
            );
            let first = put("retried", Some("key-1"), "v").await?;
            assert_eq!(first.0, 201);
            assert_eq!(
                put("retried", Some("key-1"), "v").await?,
                first,
                "a retry replays before the version check"
            );
            assert_eq!(
                put("retried", Some("key-1"), "other").await?.0,
                422,
                "one key, one body"
            );
            let (status, body) = put("third", None, "v").await?;
            assert_eq!(
                (status, &body["error"]["code"]),
                (409, &json!("secret_limit"))
            );
            let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT body_hash FROM idempotency")
                .fetch_all(&f.pool)
                .await?;
            assert!(!stored.is_empty(), "the retry was recorded");
            Ok(())
        },
    )
    .await
}
