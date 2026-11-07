//! GW-I-078..080 (ADR-0028, ADR-0032): `/api/v1/admin/templates`. Templates are administered like routes: versioned,
//! retried by Idempotency-Key, audited without values, live from the next request. A write resets the template's
//! check; the set rules and references answer 409s; the API refuses what the file refuses, with the same reasons.
#[path = "support/fixture.rs"]
mod support;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_gateway::{CheckOutcome, CheckReason, CheckRequest, TemplateCheck};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const OTHER: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

/// The engine side, scripted per template name; an unscripted template gets no answer.
#[derive(Default)]
struct Scripted(Mutex<HashMap<String, CheckOutcome>>);

impl Scripted {
    fn set(&self, name: &str, outcome: CheckOutcome) {
        if let Ok(mut script) = self.0.lock() {
            script.insert(name.to_owned(), outcome);
        }
    }
}

#[async_trait]
impl TemplateCheck for Scripted {
    async fn check(&self, request: &CheckRequest<'_>) -> CheckOutcome {
        self.0
            .lock()
            .ok()
            .and_then(|script| script.get(request.name).cloned())
            .unwrap_or(CheckOutcome::Unavailable)
    }
}

fn verified() -> CheckOutcome {
    CheckOutcome::Verified {
        warnings: tqa_sandbox::template::secret_files::SecretFiles::default(),
        base: "workstation".into(),
        image_id: PIN.into(),
    }
}

fn entry(name: &str, default: bool) -> String {
    format!(
        "[[templates]]\nname='{name}'\ndisplay_name='{name}'\nimage='{PIN}'\ncapabilities=['shell']\ndefault={default}\n"
    )
}

/// An Idempotency-Key over the 128-character bound.
const TOO_LONG_KEY: &str = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk";

/// A template body for the API (every field but the name).
fn body(display: &str, version: Option<i64>) -> Value {
    let mut body = json!({"display_name": display, "image": OTHER, "capabilities": ["shell"]});
    if let Some(version) = version {
        body["version"] = json!(version);
    }
    body
}

async fn put(
    f: &Fixture,
    cookie: &str,
    name: &str,
    body: Value,
    key: Option<&str>,
) -> Result<(u16, Value)> {
    call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/templates/{name}"),
        cookie,
        (Some(body), key),
    )
    .await
}

async fn offered(f: &Fixture, cookie: &str) -> Result<Vec<String>> {
    let listed: Value = f
        .get("/api/v1/templates", cookie)
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(listed["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .filter_map(|item| item["name"].as_str().map(str::to_owned))
        .collect())
}

/// Waits (bounded) until `name` is offered or not.
async fn until_offered(f: &Fixture, cookie: &str, name: &str, wanted: bool) -> TestResult {
    for _ in 0..100 {
        if offered(f, cookie)
            .await?
            .iter()
            .any(|offered| offered == name)
            == wanted
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "{name} offered should be {wanted}: {:?}",
        offered(f, cookie).await?
    )
    .into())
}

/// Waits (bounded) until the admin list shows `name`'s check state as `state`.
async fn until_check(f: &Fixture, cookie: &str, name: &str, state: &str) -> Result<Value> {
    for _ in 0..100 {
        let (_, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/templates",
            cookie,
            (None, None),
        )
        .await?;
        if let Some(item) = listed["items"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["name"] == name))
            && item["check"]["state"] == state
        {
            return Ok(item.clone());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!("{name} never reached check state {state}").into())
}

/// Like [`until_check`], for the result of one check: the state with its checked version, not an earlier result left in
/// view while a re-queued check runs (`checked_version` null).
async fn until_checked(
    f: &Fixture,
    cookie: &str,
    name: &str,
    state: &str,
    version: i64,
) -> Result<Value> {
    for _ in 0..100 {
        let item = until_check(f, cookie, name, state).await?;
        if item["check"]["checked_version"] == json!(version) {
            return Ok(item);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!("{name} never reached check state {state} for version {version}").into())
}

async fn audit(
    pool: &PgPool,
    item: &str,
) -> Result<Vec<(String, String, Option<i64>, Option<i64>)>> {
    Ok(sqlx::query_as(
        "SELECT actor_kind,action,before_version,after_version FROM admin_audit WHERE area='templates' AND item=$1 ORDER BY at, id",
    )
    .bind(item)
    .fetch_all(pool)
    .await?)
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_078_templates_are_administered_versioned_retried_and_audited(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &entry("workstation", true), async |f| {
        let (_, cookie) = f.admin().await?;
        let (status, created) = put(f, &cookie, "custom", body("Custom", None), Some("create-custom")).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!((&created["version"], &created["last_set_by"], &created["check"]["state"]), (&json!(1), &json!("admin"), &json!("unchecked")));
        assert_eq!((&created["is_default"], &created["enabled"], &created["image"]), (&json!(false), &json!(true), &json!(OTHER)));
        // A retry replays the stored response; the same key with another body is refused.
        assert_eq!(put(f, &cookie, "custom", body("Custom", None), Some("create-custom")).await?, (201, created.clone()));
        assert_eq!(put(f, &cookie, "custom", body("Other", None), Some("create-custom")).await?.0, 422);
        // Versions: stale or missing on an existing template is a conflict.
        assert_eq!(put(f, &cookie, "custom", body("Custom 2", Some(5)), None).await?.1["error"]["code"], "conflict");
        let (status, replaced) = put(f, &cookie, "custom", body("Custom 2", Some(1)), None).await?;
        assert_eq!((status, &replaced["version"], &replaced["display_name"]), (200, &json!(2), &json!("Custom 2")));
        let (_, listed) = call(f, Method::GET, "/api/v1/admin/templates", &cookie, (None, None)).await?;
        let names: Vec<&str> = listed["items"].as_array().ok_or("items")?.iter().filter_map(|item| item["name"].as_str()).collect();
        assert_eq!(names, ["custom", "workstation"]);
        assert!(listed["generation"].as_i64().is_some());
        assert_eq!(
            audit(&f.pool, "custom").await?,
            vec![("admin".into(), "create".into(), None, Some(1)), ("admin".into(), "replace".into(), Some(1), Some(2))]
        );
        let values: String = sqlx::query_scalar("SELECT coalesce(string_agg(row_to_json(a)::text, ''), '') FROM admin_audit a WHERE area='templates'")
            .fetch_one(&f.pool)
            .await?;
        assert!(!values.contains(OTHER) && !values.contains("Custom"), "no values in the audit");
        // Access: members and anonymous callers are refused; an administrator's API token may write.
        let (_, member) = f.member("member@example.invalid").await?;
        assert_eq!(put(f, &member, "by-member", body("M", None), None).await?.0, 403);
        let anonymous = f.client.get(f.url("/api/v1/admin/templates")).send().await?;
        assert_eq!(anonymous.status(), 401);
        let (_, token) = call(f, Method::POST, "/api/v1/tokens", &cookie, (Some(json!({"name": "admin", "expires_in_days": 1})), None)).await?;
        let response = f
            .client
            .put(f.url("/api/v1/admin/templates/by-token"))
            .bearer_auth(token["token"].as_str().unwrap_or_default())
            .json(&body("Token", None))
            .send()
            .await?;
        assert_eq!(response.status(), 201, "an administrator's API token may change templates");
        Ok(())
    })
    .await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_078_a_write_resets_the_check_and_check_requeues_without_a_restart(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &entry("workstation", true), async |f| {
        let (_, cookie) = f.admin().await?;
        let script = Arc::new(Scripted::default());
        script.set("custom", verified());
        f.gateway.start_template_checks(script.clone());
        assert_eq!(
            put(f, &cookie, "custom", body("Custom", None), None)
                .await?
                .0,
            201
        );
        until_check(f, &cookie, "custom", "verified").await?;
        // A rename is no checked input: the check carries over and the template stays offered.
        script.set("custom", CheckOutcome::Unavailable);
        let (status, renamed) = put(f, &cookie, "custom", body("Custom 2", Some(1)), None).await?;
        assert_eq!(
            (
                status,
                &renamed["check"]["state"],
                &renamed["check"]["checked_version"]
            ),
            (200, &json!("verified"), &json!(2))
        );
        until_offered(f, &cookie, "custom", true).await?;
        // A checked input needs checking again: hidden while its check cannot be answered.
        let mut wider = body("Custom 2", Some(2));
        wider["capabilities"] = json!(["shell", "python"]);
        let (status, changed) = put(f, &cookie, "custom", wider, None).await?;
        assert_eq!(
            (status, &changed["check"]["state"]),
            (200, &json!("unchecked"))
        );
        until_offered(f, &cookie, "custom", false).await?;
        // /check re-queues at once (no retry wait): refused, then, after the fix, verified.
        script.set("custom", CheckOutcome::Refused(CheckReason::RootfsMissing));
        // Another settings write wakes the check worker before /check, as a late start-up write can under load (13h's
        // full run): it records the refusal for version 3 first. /check then leaves that result in view with no
        // checked version while the check runs again, so the wait below is for the new check's own result.
        script.set("other", verified());
        assert_eq!(
            put(f, &cookie, "other", body("Other", None), None).await?.0,
            201
        );
        let early = until_check(f, &cookie, "custom", "refused").await?;
        assert_eq!(
            early["check"]["checked_version"],
            json!(3),
            "the early check recorded version 3"
        );
        assert_eq!(
            call(
                f,
                Method::POST,
                "/api/v1/admin/templates/custom/check",
                &cookie,
                (None, None)
            )
            .await?
            .0,
            202
        );
        let refused = until_checked(f, &cookie, "custom", "refused", 3).await?;
        assert_eq!(
            (
                &refused["check"]["reason"],
                &refused["check"]["checked_version"]
            ),
            (&json!("rootfs_missing"), &json!(3))
        );
        script.set("custom", verified());
        assert_eq!(
            call(
                f,
                Method::POST,
                "/api/v1/admin/templates/custom/check",
                &cookie,
                (None, None)
            )
            .await?
            .0,
            202
        );
        until_offered(f, &cookie, "custom", true).await?;
        let checked = until_check(f, &cookie, "custom", "verified").await?;
        assert_eq!(
            (&checked["version"], &checked["base"]),
            (&json!(3), &json!("workstation"))
        );
        assert_eq!(
            call(
                f,
                Method::POST,
                "/api/v1/admin/templates/absent/check",
                &cookie,
                (None, None)
            )
            .await?
            .0,
            404
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_079_writes_keep_the_set_rules_and_references(pool: PgPool) -> TestResult {
    let mcp =
        "[[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\ntemplates=['extra']\n";
    let config = [
        entry("workstation", true),
        entry("extra", false),
        entry("spare", false),
        mcp.to_owned(),
    ]
    .join("\n");
    support::run_restarting_with(pool, None, &config, async |f| {
        let (_, cookie) = f.admin().await?;
        let script = Arc::new(Scripted::default());
        script.set("custom", verified());
        f.gateway.start_template_checks(script.clone());
        let code = |response: &(u16, Value)| (response.0, response.1["error"]["code"].clone());
        // A name that exists, a second default, no default at all.
        assert_eq!(
            code(&put(f, &cookie, "workstation", body("W", None), None).await?),
            (409, json!("name_taken"))
        );
        let mut second = body("Custom", None);
        second["default"] = json!(true);
        assert_eq!(
            code(&put(f, &cookie, "custom", second, None).await?),
            (409, json!("default_count"))
        );
        let mut none = body("workstation", Some(1));
        none["image"] = json!(PIN);
        none["default"] = json!(false);
        assert_eq!(
            code(&put(f, &cookie, "workstation", none, None).await?),
            (409, json!("default_count"))
        );
        // Only an offered template can become the default: not an unchecked, refused or disabled one.
        script.set(
            "refusing",
            CheckOutcome::Refused(CheckReason::RootfsMissing),
        );
        script.set("off", verified());
        let mut off = body("Off", None);
        off["enabled"] = json!(false);
        for (name, body) in [
            ("pending", body("Pending", None)),
            ("refusing", body("Refusing", None)),
            ("off", off),
        ] {
            assert_eq!(put(f, &cookie, name, body, None).await?.0, 201, "{name}");
        }
        until_check(f, &cookie, "refusing", "refused").await?;
        until_check(f, &cookie, "off", "verified").await?;
        for name in ["pending", "refusing", "off"] {
            let refused = call(
                f,
                Method::POST,
                &format!("/api/v1/admin/templates/{name}/default"),
                &cookie,
                (Some(json!({"version": 1})), None),
            )
            .await?;
            assert_eq!(
                code(&refused),
                (409, json!("template_unavailable")),
                "{name}"
            );
            let path = format!("/api/v1/admin/templates/{name}?version=1");
            assert_eq!(
                call(f, Method::DELETE, &path, &cookie, (None, None))
                    .await?
                    .0,
                204
            );
        }
        // The default moves atomically, by name.
        assert_eq!(
            put(f, &cookie, "custom", body("Custom", None), None)
                .await?
                .0,
            201
        );
        until_offered(f, &cookie, "custom", true).await?;
        let move_default = || {
            call(
                f,
                Method::POST,
                "/api/v1/admin/templates/custom/default",
                &cookie,
                (Some(json!({"version": 1})), Some("move-default")),
            )
        };
        let (status, moved) = move_default().await?;
        assert_eq!(
            (status, &moved["is_default"], &moved["version"]),
            (200, &json!(true), &json!(2))
        );
        // A retry replays the move instead of answering a stale version.
        assert_eq!(move_default().await?, (200, moved.clone()));
        let (_, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/templates",
            &cookie,
            (None, None),
        )
        .await?;
        let defaults: Vec<&str> = listed["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .filter(|item| item["is_default"] == true)
            .filter_map(|item| item["name"].as_str())
            .collect();
        assert_eq!(defaults, ["custom"]);
        // A stale version is a conflict; the current default stays as it is.
        let default = |version: i64| {
            call(
                f,
                Method::POST,
                "/api/v1/admin/templates/custom/default",
                &cookie,
                (Some(json!({"version": version})), None),
            )
        };
        assert_eq!(code(&default(1).await?), (409, json!("conflict")));
        let (status, unchanged) = default(2).await?;
        assert_eq!(
            (status, &unchanged["is_default"], &unchanged["version"]),
            (200, &json!(true), &json!(2))
        );
        // The default stays offered: disabling it or changing a checked input is refused; a rename is not.
        let mut disabled = body("Custom", Some(2));
        disabled["enabled"] = json!(false);
        let mut repinned = body("Custom", Some(2));
        repinned["image"] = json!(PIN);
        let mut other_tools = body("Custom", Some(2));
        other_tools["capabilities"] = json!(["shell", "node"]);
        let mut smaller = body("Custom", Some(2));
        smaller["memory_mib"] = json!(512);
        for (case, write) in [
            ("disabled", disabled),
            ("image", repinned),
            ("capabilities", other_tools),
            ("memory", smaller),
        ] {
            assert_eq!(
                code(&put(f, &cookie, "custom", write, None).await?),
                (409, json!("template_is_default")),
                "{case}"
            );
        }
        let (status, renamed) = put(f, &cookie, "custom", body("Custom 2", Some(2)), None).await?;
        assert_eq!((status, &renamed["version"]), (200, &json!(3)), "{renamed}");
        until_offered(f, &cookie, "custom", true).await?;
        // The default, a bound template, and one an MCP server lists cannot be deleted.
        let (status, refused) = call(
            f,
            Method::DELETE,
            "/api/v1/admin/templates/custom?version=3",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            (
                status,
                &refused["error"]["code"],
                &refused["error"]["in_use"]["default"]
            ),
            (409, &json!("template_in_use"), &json!(true))
        );
        // Losing the default moved workstation to version 2.
        assert_eq!(
            call(
                f,
                Method::POST,
                "/api/v1/admin/templates/workstation/default",
                &cookie,
                (Some(json!({"version": 2})), None)
            )
            .await?
            .0,
            200
        );
        until_offered(f, &cookie, "custom", true).await?;
        let (status, _) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (
                Some(json!({"title": "Bound", "model_route": "local", "template": "custom"})),
                Some("bound"),
            ),
        )
        .await?;
        assert_eq!(status, 201);
        let (status, refused) = call(
            f,
            Method::DELETE,
            "/api/v1/admin/templates/custom?version=4",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            (
                status,
                &refused["error"]["code"],
                &refused["error"]["in_use"]["conversations"]
            ),
            (409, &json!("template_in_use"), &json!(1))
        );
        let (status, refused) = call(
            f,
            Method::DELETE,
            "/api/v1/admin/templates/extra?version=1",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            (
                status,
                &refused["error"]["code"],
                &refused["error"]["in_use"]["mcp_servers"]
            ),
            (409, &json!("template_in_use"), &json!(["docs"]))
        );
        // At most 32 templates: four exist.
        for index in 0..28 {
            assert_eq!(
                put(f, &cookie, &format!("t{index}"), body("T", None), None)
                    .await?
                    .0,
                201,
                "t{index}"
            );
        }
        assert_eq!(
            code(&put(f, &cookie, "one-more", body("T", None), None).await?),
            (409, json!("template_count"))
        );
        // A delete takes exactly one current version and a bounded key; a retry replays it.
        let delete = |path: &'static str, key: Option<&'static str>| {
            call(f, Method::DELETE, path, &cookie, (None, key))
        };
        assert_eq!(
            code(&delete("/api/v1/admin/templates/spare?version=9", None).await?),
            (409, json!("conflict"))
        );
        assert_eq!(
            delete("/api/v1/admin/templates/spare?version=1&v=2", None)
                .await?
                .0,
            400
        );
        assert_eq!(
            delete(
                "/api/v1/admin/templates/spare?version=1",
                Some(TOO_LONG_KEY)
            )
            .await?
            .0,
            400
        );
        // A deleted seeded template stays deleted until its seed changes.
        assert_eq!(
            delete(
                "/api/v1/admin/templates/spare?version=1",
                Some("delete-spare")
            )
            .await?
            .0,
            204
        );
        assert_eq!(
            delete(
                "/api/v1/admin/templates/spare?version=1",
                Some("delete-spare")
            )
            .await?
            .0,
            204
        );
        f.restart().await?;
        let (_, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/templates",
            &cookie,
            (None, None),
        )
        .await?;
        assert!(
            !listed.to_string().contains("\"spare\""),
            "an unchanged seed does not bring it back"
        );
        f.reconfigure(&config.replace("display_name='spare'", "display_name='Spare 2'"))?;
        f.restart().await?;
        let (_, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/templates",
            &cookie,
            (None, None),
        )
        .await?;
        assert!(
            listed.to_string().contains("\"spare\""),
            "an edited seed brings it back"
        );
        // A key is optional.
        assert_eq!(
            call(
                f,
                Method::DELETE,
                "/api/v1/admin/templates/t0?version=1",
                &cookie,
                (None, None)
            )
            .await?
            .0,
            204
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_080_the_api_refuses_what_the_file_refuses_with_the_same_reason(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &entry("workstation", true), async |f| {
        let (_, cookie) = f.admin().await?;
        let with = |field: &str, value: Value| {
            let mut body = body("T", None);
            body[field] = value;
            body
        };
        for (name, body, field, reason) in [
            ("Work", body("T", None), "name", "name_invalid"),
            (
                "t",
                with("display_name", json!("")),
                "display_name",
                "display_name_invalid",
            ),
            (
                "t",
                with("image", json!("tqa-runtime:workstation")),
                "image",
                "image_not_pinned",
            ),
            (
                "t",
                with("capabilities", json!(["shell", "shell"])),
                "capabilities",
                "capabilities_invalid",
            ),
            (
                "t",
                with("capabilities", json!(["fly"])),
                "capabilities",
                "capabilities_invalid",
            ),
            ("t", with("cpus", json!(0)), "cpus", "limit_out_of_range"),
            ("t", with("unknown", json!(1)), "body", "body_invalid"),
            (
                "t",
                with("memory_mib", json!(63)),
                "memory_mib",
                "limit_out_of_range",
            ),
        ] {
            let (status, refused) = put(f, &cookie, name, body, None).await?;
            assert_eq!(status, 400, "{field}: {refused}");
            assert_eq!(refused["error"]["code"], "invalid_request");
            assert_eq!(
                refused["error"]["details"],
                json!([{"field": field, "reason": reason}]),
                "{field}"
            );
        }
        assert_eq!(
            put(f, &cookie, "t", body("T", None), Some(TOO_LONG_KEY))
                .await?
                .0,
            400
        );
        for (body, key) in [
            (json!({"version": 1}), Some(TOO_LONG_KEY)),
            (json!({"version": "1"}), None),
        ] {
            let path = "/api/v1/admin/templates/workstation/default";
            let refused = call(f, Method::POST, path, &cookie, (Some(body), key)).await?;
            assert_eq!(refused.0, 400, "{}", refused.1);
        }
        Ok(())
    })
    .await
}

// TEST: GW-U-063
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_063_acknowledged_credential_files_are_file_only(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &entry("workstation", true), async |f| {
        let (_, cookie) = f.admin().await?;
        let mut acknowledging = body("Custom", None);
        acknowledging["acknowledged_secret_files"] = json!(["/srv/dev.key"]);
        let (status, refused) = put(f, &cookie, "custom", acknowledging, None).await?;
        assert_eq!(status, 400, "{refused}");
        let detail = &refused["error"]["details"][0];
        assert_eq!(
            (&detail["field"], &detail["reason"]),
            (&json!("acknowledged_secret_files"), &json!("file_only")),
            "{refused}"
        );
        // Nothing was written.
        let (_, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/templates",
            &cookie,
            (None, None),
        )
        .await?;
        let names: Vec<&str> = listed["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .filter_map(|item| item["name"].as_str())
            .collect();
        assert_eq!(names, ["workstation"]);
        Ok(())
    })
    .await
}
