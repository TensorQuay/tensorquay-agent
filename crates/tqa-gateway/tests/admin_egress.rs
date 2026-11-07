//! GW-I-061 (ADR-0028): egress rules live in the database, seeded from `[[egress.allow]]`, and are administered through
//! `/api/v1/admin/egress` with the file's own validator: canonical storage, typed refusals, versions, idempotent retries,
//! audit rows, administrator access (session or token), and a read-only list when egress is disabled.

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

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_061_rules_are_canonical_versioned_retried_and_audited(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let path = "/api/v1/admin/egress/API.Example.COM.";
        let body = |ports: Value, version: Option<i64>| Some(json!({"ports": ports, "version": version}));
        let (status, created) = call(f, Method::PUT, path, &cookie, (body(json!([8443, 443]), None), Some("create-1"))).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(
            created,
            json!({"host": "api.example.com", "ports": [443, 8443], "internal": false, "version": 1, "seed": null,
                   "preset": null, "last_set_by": "admin", "updated_at": created["updated_at"]}),
            "stored in the canonical form"
        );
        let (status, replayed) = call(f, Method::PUT, path, &cookie, (body(json!([8443, 443]), None), Some("create-1"))).await?;
        assert_eq!((status, &replayed), (201, &created), "a retry replays instead of answering 409");
        let canonical = "/api/v1/admin/egress/api.example.com";
        assert_eq!(call(f, Method::PUT, canonical, &cookie, (body(json!([443]), Some(1)), None)).await?.0, 200);
        for stale in [body(json!([443]), Some(1)), body(json!([443]), None)] {
            let (status, refused) = call(f, Method::PUT, canonical, &cookie, (stale, None)).await?;
            assert_eq!((status, &refused["error"]["code"]), (409, &json!("conflict")));
        }
        let (status, list) = call(f, Method::GET, "/api/v1/admin/egress", &cookie, (None, None)).await?;
        assert_eq!(status, 200);
        assert_eq!((&list["disabled"], &list["limit"], &list["items"][0]["ports"]), (&json!(false), &json!(256), &json!([443])));
        assert_eq!(call(f, Method::DELETE, &format!("{canonical}?version=1"), &cookie, (None, None)).await?.0, 409);
        assert_eq!(call(f, Method::DELETE, canonical, &cookie, (None, None)).await?.0, 409, "a delete needs the version");
        assert_eq!(call(f, Method::DELETE, &format!("{canonical}?version=2"), &cookie, (None, None)).await?.0, 204);
        assert_eq!(call(f, Method::DELETE, &format!("{canonical}?version=2"), &cookie, (None, None)).await?.0, 404);
        let audit: Vec<(String, Option<i64>, Option<i64>, bool)> = sqlx::query_as(
            "SELECT action,before_version,after_version,value_hash_changed FROM admin_audit \
             WHERE area='egress' AND actor_user_id=$1 AND item='api.example.com' ORDER BY at, id",
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
async fn gw_i_061_the_api_refuses_what_the_file_refuses_and_only_administrators_write(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let ports = |ports: Value| Some(json!({"ports": ports}));
        // Each case is the one the file loader refuses with the same kind (GW-I-012's invalid rules).
        let cases = [
            ("bad_host!", json!([443]), ("host", "host_invalid")),
            ("*", json!([443]), ("host", "wildcard_invalid")),
            ("a.*.example.com", json!([443]), ("host", "host_invalid")),
            ("*.10.0.0.1", json!([443]), ("host", "wildcard_invalid")),
            ("example.com", json!([]), ("ports", "ports_invalid")),
            ("example.com", json!([0]), ("ports", "ports_invalid")),
            ("example.com", json!([443, 443]), ("ports", "ports_invalid")),
            (
                "example.com",
                json!((1..=17).collect::<Vec<u16>>()),
                ("ports", "ports_invalid"),
            ),
        ];
        assert!(
            tqa_gateway::GatewayConfig::from_toml(&format!(
                "{}\n[egress]\n[[egress.allow]]\nhost='example.com'\nports=[443]",
                minimal()
            ))
            .is_ok()
        );
        for (host, list, (field, reason)) in cases {
            let (status, body) = call(
                f,
                Method::PUT,
                &format!("/api/v1/admin/egress/{host}"),
                &cookie,
                (ports(list.clone()), None),
            )
            .await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!(field), json!(reason))),
                "{host} {list}"
            );
            let file = format!("[egress]\n[[egress.allow]]\nhost='{host}'\nports={list}");
            assert!(
                tqa_gateway::GatewayConfig::from_toml(&format!("{}\n{file}", minimal())).is_err(),
                "the file refuses {host} {list}"
            );
        }
        let (status, body) = call(
            f,
            Method::PUT,
            "/api/v1/admin/egress/example.com",
            &cookie,
            (Some(json!({"ports": [443], "extra": 1})), None),
        )
        .await?;
        assert_eq!(
            (status, detail(&body)),
            (400, (json!("body"), json!("body_invalid")))
        );
        let (status, body) = call(
            f,
            Method::DELETE,
            "/api/v1/admin/egress/example.com?version=0",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            (status, detail(&body)),
            (400, (json!("version"), json!("version_invalid")))
        );
        assert_eq!(
            call(
                f,
                Method::GET,
                "/api/v1/admin/egress",
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
                "/api/v1/admin/egress/example.com",
                &member,
                (ports(json!([443])), None)
            )
            .await?
            .0,
            403
        );
        let (status, token) = call(
            f,
            Method::POST,
            "/api/v1/tokens",
            &cookie,
            (Some(json!({"name":"admin","expires_in_days":1})), None),
        )
        .await?;
        assert_eq!(status, 201);
        let response = f
            .client
            .put(f.url("/api/v1/admin/egress/example.com"))
            .bearer_auth(token["token"].as_str().unwrap_or_default())
            .json(&json!({"ports": [443]}))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            201,
            "an administrator's API token may change egress rules"
        );
        Ok(())
    })
    .await
}

/// The smallest gateway configuration the loader accepts, for checking a file refusal.
fn minimal() -> &'static str {
    "listen='127.0.0.1:0'\npublic_origin='http://127.0.0.1:1'\nmodels_path='models.toml'\n[storage]\nroot='/var/lib/tqa'"
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_061_with_egress_disabled_the_list_is_read_only(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[egress]\ndisabled=true", async |f| {
        let (_, cookie) = f.admin().await?;
        let (status, list) = call(
            f,
            Method::GET,
            "/api/v1/admin/egress",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            (status, &list["disabled"], &list["items"]),
            (200, &json!(true), &json!([]))
        );
        let (status, body) = call(
            f,
            Method::PUT,
            "/api/v1/admin/egress/example.com",
            &cookie,
            (Some(json!({"ports": [443]})), None),
        )
        .await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("egress_disabled"))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_061_seeded_rules_follow_the_seed_change_rule(pool: PgPool) -> TestResult {
    let seeded = "[egress]\n[[egress.allow]]\nhost='Seeded.Example'\nports=[443]\n[[egress.allow]]\nhost='dropped.example'\nports=[80]";
    support::run_restarting_with(pool, None, seeded, async |f| {
        let (_, cookie) = f.admin().await?;
        let rules = async |f: &support::Fixture| -> Result<Value, Box<dyn std::error::Error>> {
            Ok(call(
                f,
                Method::GET,
                "/api/v1/admin/egress",
                &cookie,
                (None, None),
            )
            .await?
            .1["items"]
                .clone())
        };
        let listed = rules(f).await?;
        assert_eq!(
            (
                &listed[1]["host"],
                &listed[1]["seed"],
                &listed[1]["last_set_by"],
                &listed[1]["version"]
            ),
            (
                &json!("seeded.example"),
                &json!("current"),
                &json!("seed"),
                &json!(1)
            )
        );
        let path = "/api/v1/admin/egress/seeded.example";
        assert_eq!(
            call(
                f,
                Method::PUT,
                path,
                &cookie,
                (Some(json!({"ports": [8443], "version": 1})), None)
            )
            .await?
            .0,
            200
        );
        f.reconfigure("[egress]\n[[egress.allow]]\nhost='seeded.example.'\nports=[443]")?;
        f.restart().await?;
        let listed = rules(f).await?;
        assert_eq!(
            (&listed[0]["host"], &listed[0]["seed"]),
            (&json!("dropped.example"), &json!("removed")),
            "kept and marked"
        );
        assert_eq!(
            (&listed[1]["ports"], &listed[1]["last_set_by"]),
            (&json!([8443]), &json!("admin")),
            "an unchanged seed keeps the change"
        );
        f.reconfigure("[egress]\n[[egress.allow]]\nhost='seeded.example'\nports=[443, 80]")?;
        f.restart().await?;
        let listed = rules(f).await?;
        assert_eq!(
            (
                &listed[1]["ports"],
                &listed[1]["last_set_by"],
                &listed[1]["version"]
            ),
            (&json!([80, 443]), &json!("seed"), &json!(3)),
            "an edited seed wins"
        );
        f.reconfigure("[egress]\n[[egress.allow]]\nhost='seeded.example'\nports=[443, 80]\n[[egress.allow]]\nhost='dropped.example'\nports=[80]")?;
        f.restart().await?;
        let listed = rules(f).await?;
        assert_eq!(
            (&listed[0]["seed"], &listed[0]["version"]),
            (&json!("current"), &json!(1)),
            "listed again unchanged: current again, with no new version"
        );
        sqlx::query("UPDATE egress_rules SET host='bad host' WHERE host='dropped.example'")
            .execute(&f.pool)
            .await?;
        let refused = f.start_again().await;
        assert!(
            matches!(refused, Err(tqa_gateway::GatewayError::Configuration(_))),
            "a stored rule the validator refuses stops the start: {refused:?}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_061_the_list_is_bounded_and_a_retried_delete_replays(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let path = "/api/v1/admin/egress/example.com";
        let (status, body) = call(f, Method::PUT, path, &cookie, (Some(json!({"ports": [443], "version": 0})), None)).await?;
        assert_eq!((status, detail(&body)), (400, (json!("version"), json!("version_invalid"))));
        assert_eq!(call(f, Method::PUT, path, &cookie, (Some(json!({"ports": [443]})), None)).await?.0, 201);
        let delete = format!("{path}?version=1");
        for attempt in ["first", "retry"] {
            let (status, _) = call(f, Method::DELETE, &delete, &cookie, (None, Some("delete-1"))).await?;
            assert_eq!(status, 204, "the {attempt} answer is the stored one");
        }
        // Fill the list to its bound of 256; the next rule is refused.
        sqlx::query("INSERT INTO egress_rules (host,ports) SELECT 'h' || n || '.example', ARRAY[443] FROM generate_series(1,256) AS n")
            .execute(&f.pool)
            .await?;
        let (status, body) = call(f, Method::PUT, path, &cookie, (Some(json!({"ports": [443]})), None)).await?;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("egress_rule_limit")));
        Ok(())
    })
    .await
}
