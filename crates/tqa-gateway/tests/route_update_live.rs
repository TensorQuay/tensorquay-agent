//! GW-I-157: a route an administrator changes reaches every open task that uses it, directly or as a fallback, from
//! its next message: the gateway sends the task's live runtime a `session.update` with its route and fallbacks as they
//! now are, so following an error's advice ("raise Max output on the route") and replying works without restarting the
//! environment. A task on another route gets nothing.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use support::{Fixture, TestResult, call};
use tqa_types::ClientFrame;

fn registry() -> Result<tqa_llm::Registry, tqa_llm::RegistryError> {
    tqa_llm::Registry::from_toml(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='fixture'\nauth={kind='none'}\n\n[[routes]]\nname='other'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='second'\nauth={kind='none'}\nfallbacks=['spare']\n\n[[routes]]\nname='spare'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='third'\nauth={kind='none'}",
    )
}

/// Changes a seeded route's model, as the Models page does: the route's own settings and the version read.
async fn change_model(f: &Fixture, cookie: &str, name: &str, model: &str) -> TestResult {
    let (status, routes) =
        call(f, Method::GET, "/api/v1/admin/routes", cookie, (None, None)).await?;
    assert_eq!(status, 200, "{routes}");
    let version = routes["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["name"] == name))
        .map(|item| item["version"].clone())
        .ok_or(name.to_owned())?;
    let body: Value = json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": model,
        "auth": {"kind": "none"}, "fallbacks": if name == "other" { json!(["spare"]) } else { json!([]) },
        "version": version});
    let path = format!("/api/v1/admin/routes/{name}");
    let (status, answer) = call(f, Method::PUT, &path, cookie, (Some(body), None)).await?;
    assert_eq!(status, 200, "{answer}");
    Ok(())
}

// TEST: GW-I-157
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_157_a_changed_route_reaches_every_open_task_using_it(pool: PgPool) -> TestResult {
    support::run_configured(pool, Some(registry()?), "", async |f| {
        let (user, cookie) = f.admin().await?;
        let on_local = f.conversation(user.id).await?;
        let on_other = f.conversation(user.id).await?;
        let path = format!("/api/v1/conversations/{}", on_other.id);
        let switched = f
            .client
            .patch(f.url(&path))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .header("idempotency-key", "switch-other")
            .json(&json!({"route": "other"}))
            .send()
            .await?;
        assert_eq!(switched.status(), 200);
        let mut local = f.runtime(&on_local).await?;
        let mut other = f.runtime(&on_other).await?;
        // A fallback changes: the task whose route falls back to it hears, with its own route; the other doesn't.
        change_model(f, &cookie, "spare", "third-v2").await?;
        let Some(ClientFrame::SessionUpdate {
            route,
            fallback_routes,
        }) = other.try_action(support::DEADLINE).await?
        else {
            return Err(
                "the task falling back to the changed route must receive session.update".into(),
            );
        };
        assert_eq!(
            (route.name.as_str(), route.model.as_str()),
            ("other", "second")
        );
        let fallbacks: Vec<(&str, &str)> = fallback_routes
            .iter()
            .map(|fallback| (fallback.name.as_str(), fallback.model.as_str()))
            .collect();
        assert_eq!(fallbacks, [("spare", "third-v2")]);
        assert!(
            local
                .try_action(Duration::from_millis(500))
                .await?
                .is_none(),
            "a task on another route hears nothing"
        );
        // Its own route changes: that task hears, from its next message.
        change_model(f, &cookie, "local", "fixture-v2").await?;
        let Some(ClientFrame::SessionUpdate { route, .. }) =
            local.try_action(support::DEADLINE).await?
        else {
            return Err("the task on the changed route must receive session.update".into());
        };
        assert_eq!(
            (route.name.as_str(), route.model.as_str()),
            ("local", "fixture-v2")
        );
        assert!(
            other
                .try_action(Duration::from_millis(500))
                .await?
                .is_none()
        );
        Ok(())
    })
    .await
}
