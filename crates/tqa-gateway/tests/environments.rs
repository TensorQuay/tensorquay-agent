//! GW-I-043: environment templates, binding at creation, and switching while no run is active.
#[path = "support/fixture.rs"]
mod support;

use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, Event, ServerFrame};

const WORKSTATION: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const MINIMAL: &str =
    "example/minimal@sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn templates() -> String {
    format!(
        "[[templates]]\nname='workstation'\ndisplay_name='Workstation'\nimage='{WORKSTATION}'\ncapabilities=['browser','shell','python']\ndefault=true\nmemory_mib=2048\n\n[[templates]]\nname='minimal'\ndisplay_name='Minimal'\nimage='{MINIMAL}'\ncapabilities=['shell']\n"
    )
}

async fn switch(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
    key: &str,
    template: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    f.client
        .put(f.url(&format!("/api/v1/conversations/{id}/environment")))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", key)
        .json(&json!({ "template": template }))
        .send()
        .await
}

async fn binding(f: &Fixture, id: &str) -> Result<(String, String), sqlx::Error> {
    sqlx::query_as("SELECT template,template_digest FROM conversations WHERE id=$1::uuid")
        .bind(id)
        .fetch_one(&f.pool)
        .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_043_templates_are_listed_and_bound_at_creation(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &templates(), async |f| {
        let (_, cookie) = f.admin().await?;
        let listed: Value = f.get("/api/v1/templates", &cookie).await?.error_for_status()?.json().await?;
        assert_eq!(
            listed,
            json!({"items":[
                {"name":"workstation","display_name":"Workstation","description":null,"capabilities":["shell","python","browser"],"is_default":true,"cpus":1.0,"memory_mib":2048},
                {"name":"minimal","display_name":"Minimal","description":null,"capabilities":["shell"],"is_default":false,"cpus":1.0,"memory_mib":512}
            ]})
        );
        assert!(!listed.to_string().contains("sha256"), "no image references or digests");
        for (key, template, expected) in [("default", None, ("workstation", WORKSTATION)), ("minimal", Some("minimal"), ("minimal", MINIMAL))] {
            let mut body = json!({"title":"Environment","model_route":"local"});
            if let Some(template) = template {
                body["template"] = json!(template);
            }
            let created: Value = f.action("/api/v1/conversations", &cookie, Some(key), &body).await?.error_for_status()?.json().await?;
            assert_eq!(created["template"], expected.0);
            let id = created["id"].as_str().ok_or("id")?;
            assert_eq!(binding(f, id).await?, (expected.0.to_owned(), expected.1.to_owned()));
        }
        let unknown = f
            .action("/api/v1/conversations", &cookie, Some("unknown"), &json!({"title":"Environment","model_route":"local","template":"absent"}))
            .await?;
        assert_eq!(unknown.status(), 422);
        assert_eq!(unknown.json::<Value>().await?["error"]["code"], "unknown_template");
        let listed: Value = f.get("/api/v1/conversations", &cookie).await?.json().await?;
        assert_eq!(listed["items"].as_array().map(Vec::len), Some(2), "a refused create creates nothing");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_043_idle_switch_retires_the_sandbox_and_the_next_one_uses_the_new_template(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &templates(), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let runtime = f.runtime(&conversation).await?;
        assert_eq!(f.backend.created(id).ok_or("spec")?, (WORKSTATION.to_owned(), 2048 * 1024 * 1024));
        assert_eq!(
            runtime.config["environment"],
            json!({"template":"workstation","digest":WORKSTATION,"capabilities":["shell","python","browser"]})
        );
        let mounts = f.backend.mounts(id).ok_or("mounts")?;
        // The same template is a successful no-op.
        let same = switch(f, id, &cookie, "same", "workstation").await?;
        assert_eq!(same.status(), 200);
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        let switched = switch(f, id, &cookie, "switch", "minimal").await?;
        assert_eq!(switched.status(), 200);
        assert_eq!(switched.json::<Value>().await?["template"], "minimal");
        // The switch retires the sandbox in the background. A new one may start only once the retirement has
        // released the instance record (admission refuses a start until then), which is after the backend destroy.
        f.until_released(id).await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 1);
        drop(runtime);
        f.forget_runtime(id)?;
        let replacement = f.runtime(&conversation).await?;
        assert_eq!(f.backend.created(id).ok_or("spec")?.0, MINIMAL);
        assert_eq!(f.backend.mounts(id).ok_or("mounts")?, mounts, "same workspace and state");
        assert_eq!(
            replacement.config["environment"],
            json!({"template":"minimal","digest":MINIMAL,"capabilities":["shell"]})
        );
        // A replay returns the first response and retires nothing further.
        let replay = switch(f, id, &cookie, "switch", "minimal").await?;
        assert_eq!(replay.status(), 200);
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_043_switch_is_refused_during_a_run_or_for_an_unknown_template(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &templates(), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let unknown = switch(f, id, &cookie, "unknown", "absent").await?;
        assert_eq!(unknown.status(), 422);
        assert_eq!(
            unknown.json::<Value>().await?["error"]["code"],
            "unknown_template"
        );
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 1.into(),
                run_id: None,
                step: None,
                ts: "2026-01-01T00:00:00Z".into(),
                event: Event::RunStarted {
                    run_id: "active".parse()?,
                },
            })
            .await?;
        f.until_sequence(id, 1).await?;
        let during = switch(f, id, &cookie, "during", "minimal").await?;
        assert_eq!(during.status(), 409);
        assert_eq!(during.json::<Value>().await?["error"]["code"], "run_active");
        assert_eq!(
            binding(f, id.as_str()).await?,
            ("workstation".to_owned(), WORKSTATION.to_owned())
        );
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
        // Another user cannot switch it.
        let (_, other) = f.member("other@example.invalid").await?;
        assert_eq!(
            switch(f, id, &other, "foreign", "minimal").await?.status(),
            403
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_043_a_message_during_switch_retirement_is_queued_and_starts_the_new_template(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &templates(), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let runtime = f.runtime(&conversation).await?;
        f.backend.hold_destroy.store(true, Ordering::SeqCst);
        assert_eq!(
            switch(f, id, &cookie, "switch", "minimal").await?.status(),
            200
        );
        // Wait until the old sandbox is retiring: its record is `destroying` and destroy has begun.
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.destroys.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let message = f
            .action(
                &format!("/api/v1/conversations/{id}/messages"),
                &cookie,
                Some("after-switch"),
                &json!({"text":"run on the new template"}),
            )
            .await?;
        assert_eq!(
            message.status(),
            202,
            "accepted and queued on the first attempt"
        );
        f.backend.hold_destroy.store(false, Ordering::SeqCst);
        f.backend.release_destroy.notify_waiters();
        drop(runtime);
        f.forget_runtime(id)?;
        // Admission starts the replacement for the queued message once the old record is released.
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let mut replacement = f.runtime(&conversation).await?;
        assert_eq!(f.backend.created(id).ok_or("spec")?.0, MINIMAL);
        assert!(matches!(
            replacement.action().await?,
            tqa_types::ClientFrame::UserMessage { text, .. } if text == "run on the new template"
        ));
        assert_eq!(f.backend.creates.load(Ordering::SeqCst), 2);
        Ok(())
    })
    .await
}

/// A switch while the sandbox is still starting (its runtime not yet ready) retires it at once, not after the
/// readiness bound (`ready_seconds`, 30 s here, longer than the test's deadline): the worker wakes on the switch.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_043_a_switch_during_the_start_retires_the_starting_sandbox_at_once(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &templates(), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        // Started, but no runtime ever links: the worker waits for readiness.
        f.gateway.sandboxes().ensure(id).await?;
        let switched = switch(f, id, &cookie, "switch", "minimal").await?;
        assert_eq!(switched.status(), 200);
        f.until_released(id).await?;
        assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 1);
        // The next sandbox is built from the new template.
        let replacement = f.runtime(&conversation).await?;
        assert_eq!(f.backend.created(id).ok_or("spec")?.0, MINIMAL);
        assert_eq!(replacement.config["environment"]["template"], "minimal");
        Ok(())
    })
    .await
}
