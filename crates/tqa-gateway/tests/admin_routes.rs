//! GW-I-060 and GW-U-031 (ADR-0028): model routes are administered through `/api/v1/admin/routes` with the file's own
//! validator: versions, idempotent retries, audit rows without values, `key_missing`, fallbacks, the models file export,
//! administrator access (session or token), and a change that applies from the next request.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{TestResult, call};

fn detail(body: &Value) -> (Value, Value) {
    let detail = &body["error"]["details"][0];
    (detail["field"].clone(), detail["reason"].clone())
}

/// A valid route body (every field but the name).
fn route(model: &str) -> Value {
    json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": model, "auth": {"kind": "none"}})
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_060_routes_are_versioned_retried_audited_and_live_at_once(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let path = "/api/v1/admin/routes/big";
        let mut body = route("big-1");
        body["display_name"] = json!("Big model");
        let (status, created) = call(
            f,
            Method::PUT,
            path,
            &cookie,
            (Some(body.clone()), Some("create-big")),
        )
        .await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(
            (
                &created["name"],
                &created["model"],
                &created["display_name"],
                &created["version"],
                &created["seed"],
                &created["last_set_by"]
            ),
            (
                &json!("big"),
                &json!("big-1"),
                &json!("Big model"),
                &json!(1),
                &json!(null),
                &json!("admin")
            )
        );
        assert_eq!(
            (
                &created["fallbacks"],
                &created["fallback_of"],
                &created["key_missing"]
            ),
            (&json!([]), &json!([]), &json!(false))
        );
        assert_eq!(
            created["probe"],
            json!({"state": "never", "probed_at": null, "observed": null})
        );
        let (status, replayed) = call(
            f,
            Method::PUT,
            path,
            &cookie,
            (Some(body), Some("create-big")),
        )
        .await?;
        assert_eq!(
            (status, &replayed),
            (201, &created),
            "a retry replays instead of answering 409"
        );
        // Live at once: the next /models read has it, with its label and no verification yet.
        let models: Value = f.get("/api/v1/models", &cookie).await?.json().await?;
        let listed = models["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|item| item["name"] == "big")
            .cloned()
            .ok_or("big")?;
        assert_eq!(
            (&listed["display_name"], &listed["verified"]),
            (&json!("Big model"), &json!(null))
        );
        let mut replace = route("big-2");
        replace["version"] = json!(1);
        assert_eq!(
            call(f, Method::PUT, path, &cookie, (Some(replace.clone()), None))
                .await?
                .0,
            200
        );
        for stale in [replace.clone(), route("big-3")] {
            let (status, refused) =
                call(f, Method::PUT, path, &cookie, (Some(stale), None)).await?;
            assert_eq!(
                (status, &refused["error"]["code"]),
                (409, &json!("conflict")),
                "a stale or missing version"
            );
        }
        let (status, list) = call(
            f,
            Method::GET,
            "/api/v1/admin/routes",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        let big = list["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|item| item["name"] == "big")
            .cloned()
            .ok_or("big")?;
        assert_eq!(
            (&big["model"], &big["display_name"], &big["version"]),
            (&json!("big-2"), &json!(null), &json!(2))
        );
        assert!(list["generation"].as_i64().ok_or("generation")? >= 2);
        assert_eq!(
            call(
                f,
                Method::DELETE,
                &format!("{path}?version=1"),
                &cookie,
                (None, None)
            )
            .await?
            .0,
            409
        );
        for attempt in ["first", "retry"] {
            let delete = format!("{path}?version=2");
            let (status, _) = call(
                f,
                Method::DELETE,
                &delete,
                &cookie,
                (None, Some("delete-big")),
            )
            .await?;
            assert_eq!(status, 204, "the {attempt} answer is the stored one");
        }
        assert_eq!(
            call(
                f,
                Method::DELETE,
                &format!("{path}?version=2"),
                &cookie,
                (None, None)
            )
            .await?
            .0,
            404
        );
        let models: Value = f.get("/api/v1/models", &cookie).await?.json().await?;
        assert!(
            !models.to_string().contains("\"big\""),
            "gone from the next read"
        );
        let audit: Vec<(String, Option<i64>, Option<i64>, bool)> = sqlx::query_as(
            "SELECT action,before_version,after_version,value_hash_changed FROM admin_audit \
             WHERE area='routes' AND actor_user_id=$1 AND item='big' ORDER BY at, id",
        )
        .bind(admin.id)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(
            audit,
            vec![
                ("create".into(), None, Some(1), true),
                ("replace".into(), Some(1), Some(2), true),
                ("delete".into(), Some(2), None, true)
            ]
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_060_fallbacks_key_refs_export_and_access(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let mut keyed = route("keyed");
        keyed["auth"] = json!({"kind": "bearer"});
        keyed["key_ref"] = json!("secrets/absent");
        let (status, body) = call(
            f,
            Method::PUT,
            "/api/v1/admin/routes/keyed",
            &cookie,
            (Some(keyed), None),
        )
        .await?;
        assert_eq!(
            (status, &body["key_missing"]),
            (201, &json!(true)),
            "a key_ref with no value is flagged, never an error"
        );
        let mut primary = route("primary");
        primary["fallbacks"] = json!(["keyed"]);
        assert_eq!(
            call(
                f,
                Method::PUT,
                "/api/v1/admin/routes/primary",
                &cookie,
                (Some(primary), None)
            )
            .await?
            .0,
            201
        );
        let (_, list) = call(
            f,
            Method::GET,
            "/api/v1/admin/routes",
            &cookie,
            (None, None),
        )
        .await?;
        let keyed_item = list["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|item| item["name"] == "keyed")
            .cloned()
            .ok_or("keyed")?;
        assert_eq!(keyed_item["fallback_of"], json!(["primary"]));
        let (status, body) = call(
            f,
            Method::DELETE,
            "/api/v1/admin/routes/keyed?version=1",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("route_is_fallback"))
        );
        let export = f
            .client
            .get(f.url("/api/v1/admin/routes/export"))
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(export.status(), 200);
        let text = export.text().await?;
        let exported = tqa_llm::Registry::from_toml(&text)?;
        assert_eq!(
            exported.names().collect::<Vec<_>>(),
            ["keyed", "local", "primary"],
            "the export loads as a models file"
        );
        assert!(!text.contains("display_name"));
        assert_eq!(
            call(
                f,
                Method::GET,
                "/api/v1/admin/routes",
                &member,
                (None, None)
            )
            .await?
            .0,
            403
        );
        assert_eq!(
            call(
                f,
                Method::PUT,
                "/api/v1/admin/routes/member",
                &member,
                (Some(route("m")), None)
            )
            .await?
            .0,
            403
        );
        let (_, token) = call(
            f,
            Method::POST,
            "/api/v1/tokens",
            &cookie,
            (Some(json!({"name":"admin","expires_in_days":1})), None),
        )
        .await?;
        let response = f
            .client
            .put(f.url("/api/v1/admin/routes/by-token"))
            .bearer_auth(token["token"].as_str().unwrap_or_default())
            .json(&route("token"))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            201,
            "an administrator's API token may change routes"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_031_the_api_refuses_a_route_for_the_reason_the_file_does(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let with = |changes: Value| {
            let mut body = route("model");
            if let (Value::Object(body), Value::Object(changes)) = (&mut body, changes) {
                body.extend(changes);
            }
            body
        };
        // Each body breaks one rule; the file with the same record must fail for the same reason.
        let cases = [
            (with(json!({"model": ""})), "model"),
            (
                with(json!({"base_url": "ftp://models.example"})),
                "base_url",
            ),
            (with(json!({"auth": {"kind": "bearer"}})), "key_ref"),
            (with(json!({"context_window": 0})), "limits"),
            (
                with(json!({"extra_body": {"model": "other"}})),
                "extra_body",
            ),
            (
                with(json!({"extra_headers": {"authorization": "Bearer x"}})),
                "extra_headers",
            ),
            (with(json!({"quirks": ["unknown"]})), "quirks"),
            (with(json!({"fallbacks": ["nowhere"]})), "fallbacks"),
            (with(json!({"fallbacks": ["case"]})), "fallbacks"),
        ];
        for (body, field) in cases {
            let (status, refused) = call(
                f,
                Method::PUT,
                "/api/v1/admin/routes/case",
                &cookie,
                (Some(body.clone()), None),
            )
            .await?;
            let (got_field, reason) = detail(&refused);
            assert_eq!((status, got_field), (400, json!(field)), "{body} {refused}");
            let mut record = body.clone();
            record["name"] = json!("case");
            let file = toml::to_string(&serde_json::from_value::<toml::Value>(
                json!({"routes": [record]}),
            )?)?;
            let error = tqa_llm::Registry::from_toml(&file)
                .err()
                .ok_or("the file accepts it")?;
            assert_eq!(
                json!(error.reason().ok_or("a typed reason")?.code()),
                reason,
                "{file}"
            );
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_060_names_display_names_versions_and_bodies_are_bounded(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let with = |changes: Value| {
            let mut body = route("model");
            if let (Value::Object(body), Value::Object(changes)) = (&mut body, changes) {
                body.extend(changes);
            }
            body
        };
        let ok = "/api/v1/admin/routes/ok".to_owned();
        let cases = [
            (
                format!("/api/v1/admin/routes/{}", "n".repeat(129)),
                route("m"),
                ("name", "route_name_invalid"),
            ),
            (
                "/api/v1/admin/routes/bad%01name".to_owned(),
                route("m"),
                ("name", "route_name_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"display_name": " padded"})),
                ("display_name", "display_name_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"display_name": "x".repeat(81)})),
                ("display_name", "display_name_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"display_name": ""})),
                ("display_name", "display_name_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"display_name": "tab\there"})),
                ("display_name", "display_name_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"display_name": 5})),
                ("display_name", "display_name_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"version": 0})),
                ("version", "version_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"version": "1"})),
                ("version", "version_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"name": "ok"})),
                ("body", "body_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"unknown": 1})),
                ("body", "body_invalid"),
            ),
            (
                ok.clone(),
                with(json!({"context_window": "big"})),
                ("body", "body_invalid"),
            ),
            (ok.clone(), json!([1]), ("body", "body_invalid")),
        ];
        for (path, body, (field, reason)) in cases {
            let (status, refused) =
                call(f, Method::PUT, &path, &cookie, (Some(body.clone()), None)).await?;
            assert_eq!(
                (status, detail(&refused)),
                (400, (json!(field), json!(reason))),
                "{path} {body}"
            );
        }
        // The edges that are accepted: exactly 128 bytes of name, 80 characters of label, and an explicit null label.
        let longest = format!("/api/v1/admin/routes/{}", "n".repeat(128));
        let label = with(json!({"display_name": "é".repeat(80)}));
        assert_eq!(
            call(f, Method::PUT, &longest, &cookie, (Some(label), None))
                .await?
                .0,
            201
        );
        let (status, body) = call(
            f,
            Method::PUT,
            &ok,
            &cookie,
            (
                Some(with(json!({"display_name": null, "version": null}))),
                None,
            ),
        )
        .await?;
        assert_eq!((status, &body["display_name"]), (201, &json!(null)));
        Ok(())
    })
    .await
}
