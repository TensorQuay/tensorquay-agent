//! GW-I-076 (ADR-0028, ADR-0032): templates are served from the settings snapshot. A template is offered when it is
//! enabled and verified, or still unchecked when its stored values are the operator's file's; never once a check
//! refused it. Choosing one that is not offered is `template_unavailable`; conversations already bound keep working.
//! Check results reach the snapshot through the checker's own settings write (lock, bump, refresh), with no restart.
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
use support::{Fixture, TestResult, call};
use tqa_gateway::{CheckOutcome, CheckReason, CheckRequest, TemplateCheck};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

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

fn templates() -> String {
    let entry = |name: &str, default: bool| {
        format!(
            "[[templates]]\nname='{name}'\ndisplay_name='{name}'\nimage='{PIN}'\ncapabilities=['shell']\ndefault={default}\n"
        )
    };
    [
        entry("workstation", true),
        entry("extra", false),
        entry("again", false),
    ]
    .join("\n")
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

/// Waits (bounded) until the offered templates are `expected`: the checker works in the background.
async fn until_offered(f: &Fixture, cookie: &str, expected: &[&str]) -> TestResult {
    for _ in 0..100 {
        if offered(f, cookie).await? == expected {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "offered {:?}, expected {expected:?}",
        offered(f, cookie).await?
    )
    .into())
}

/// Creating a conversation on `template` (or the default): the status and the error code, if any.
async fn create(
    f: &Fixture,
    cookie: &str,
    key: &str,
    template: Option<&str>,
) -> Result<(u16, Value)> {
    let mut body = json!({"title": "T", "model_route": "local"});
    if let Some(template) = template {
        body["template"] = json!(template);
    }
    let (status, body) = call(
        f,
        Method::POST,
        "/api/v1/conversations",
        cookie,
        (Some(body), Some(key)),
    )
    .await?;
    Ok((status, body["error"]["code"].clone()))
}

/// Restarts the gateway, whose start-up reloads the database, and starts the checker again.
async fn restart(f: &mut Fixture, script: &Arc<Scripted>) -> TestResult {
    f.restart().await?;
    f.gateway.start_template_checks(script.clone());
    Ok(())
}

async fn set(f: &Fixture, sql: &str) -> TestResult {
    // Fixture-only statements for what slice 4's API will write (with lock, bump and refresh), applied by a restart.
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(&f.pool)
        .await?;
    Ok(())
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_076_templates_are_offered_by_state_and_provenance(pool: sqlx::PgPool) -> TestResult {
    support::run_restarting_with(pool, None, &templates(), async |f| {
        let (_, cookie) = f.admin().await?;
        let script = Arc::new(Scripted::default());
        script.set("extra", CheckOutcome::Refused(CheckReason::NotDerived));
        let (status, bound) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (
                Some(json!({"title": "Bound", "model_route": "local", "template": "extra"})),
                Some("bound"),
            ),
        )
        .await?;
        assert_eq!(status, 201, "{bound}");
        let bound = bound["id"].as_str().ok_or("id")?.to_owned();
        assert_eq!(
            offered(f, &cookie).await?,
            ["workstation", "again", "extra"],
            "the file's, unchecked: offered"
        );

        // A check refuses a file-provided template: it leaves the list without a restart; a bound conversation keeps it.
        f.gateway.start_template_checks(script.clone());
        until_offered(f, &cookie, &["workstation", "again"]).await?;
        assert_eq!(
            create(f, &cookie, "refused", Some("extra")).await?,
            (409, json!("template_unavailable"))
        );
        let (status, other) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (
                Some(json!({"title": "Other", "model_route": "local"})),
                Some("other"),
            ),
        )
        .await?;
        assert_eq!(status, 201, "{other}");
        let (status, switched) = call(
            f,
            Method::PUT,
            &format!(
                "/api/v1/conversations/{}/environment",
                other["id"].as_str().ok_or("id")?
            ),
            &cookie,
            (Some(json!({"template": "extra"})), Some("switch")),
        )
        .await?;
        assert_eq!(
            (status, &switched["error"]["code"]),
            (409, &json!("template_unavailable"))
        );
        let conversation: Value = f
            .get(&format!("/api/v1/conversations/{bound}"), &cookie)
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(conversation["template"], "extra");

        // Stored values that are no longer the file's (an administrator's, as re-creating a seeded name after a delete
        // leaves them, with the old seed digest): offered only once verified (review R2).
        set(
            f,
            "UPDATE templates SET last_set_by='admin' WHERE name='again'",
        )
        .await?;
        restart(f, &script).await?;
        until_offered(f, &cookie, &["workstation"]).await?;
        assert_eq!(
            create(f, &cookie, "unverified", Some("again")).await?,
            (409, json!("template_unavailable"))
        );
        script.set("again", verified());
        set(
            f,
            "UPDATE templates SET version=version+1 WHERE name='again'",
        )
        .await?;
        restart(f, &script).await?;
        until_offered(f, &cookie, &["workstation", "again"]).await?;
        assert_eq!(create(f, &cookie, "verified", Some("again")).await?.0, 201);

        // A disabled default: every create without a template is refused, and the readiness detail says why (D1).
        set(
            f,
            "UPDATE templates SET enabled=false WHERE name='workstation'",
        )
        .await?;
        restart(f, &script).await?;
        until_offered(f, &cookie, &["again"]).await?;
        assert_eq!(
            create(f, &cookie, "no-default", None).await?,
            (409, json!("template_unavailable"))
        );
        let (_, detail) = call(f, Method::GET, "/readyz?detail=1", &cookie, (None, None)).await?;
        assert_eq!(detail["templates"]["default"], "workstation");
        assert_eq!(detail["templates"]["default_offered"], false);
        assert_eq!(detail["templates"]["default_reason"], "disabled");
        // A name that does not exist stays unknown.
        assert_eq!(
            create(f, &cookie, "unknown", Some("absent")).await?,
            (422, json!("unknown_template"))
        );
        Ok(())
    })
    .await
}
