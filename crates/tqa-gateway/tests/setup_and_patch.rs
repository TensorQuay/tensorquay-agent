//! GW-I-044: setup state, a signed-in first administrator, and conversation PATCH (title and route).
#[path = "support/fixture.rs"]
mod support;

use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ClientFrame, ConversationId, Event, ServerFrame};

fn registry() -> Result<tqa_llm::Registry, tqa_llm::RegistryError> {
    tqa_llm::Registry::from_toml(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='fixture'\nauth={kind='none'}\n\n[[routes]]\nname='other'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='second'\nauth={kind='none'}\ncontext_window=4096\nfallbacks=['spare']\n\n[[routes]]\nname='spare'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='third'\nauth={kind='none'}",
    )
}

async fn patch(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
    key: &str,
    body: &Value,
) -> Result<reqwest::Response, reqwest::Error> {
    f.client
        .patch(f.url(&format!("/api/v1/conversations/{id}")))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", key)
        .json(body)
        .send()
        .await
}

async fn stored(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(f.get(&format!("/api/v1/conversations/{id}"), cookie)
        .await?
        .json()
        .await?)
}

async fn setup_required(f: &Fixture) -> Result<bool, Box<dyn std::error::Error>> {
    let response = f.client.get(f.url("/api/v1/setup")).send().await?;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await?;
    assert_eq!(
        body.as_object().map(serde_json::Map::len),
        Some(1),
        "setup exposes nothing else"
    );
    Ok(body["required"]
        .as_bool()
        .ok_or("required must be a boolean")?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_044_setup_reports_a_fresh_install_and_bootstrap_signs_the_administrator_in(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        assert!(setup_required(f).await?);
        let response = f
            .post(
                "/api/v1/auth/bootstrap",
                &json!({"email":"first@example.invalid","password":f.password(),"token":f.bootstrap()}),
            )
            .await?;
        assert_eq!(response.status(), 201);
        let cookie = response
            .headers()
            .get("set-cookie")
            .ok_or("bootstrap must set the session cookie")?
            .to_str()?
            .split(';')
            .next()
            .ok_or("missing cookie")?
            .to_owned();
        let user: Value = response.json().await?;
        assert_eq!(user["email"], "first@example.invalid");
        let me: Value = f.get("/api/v1/auth/me", &cookie).await?.error_for_status()?.json().await?;
        assert_eq!(me["id"], user["id"]);
        assert!(!setup_required(f).await?);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_044_patch_renames_and_switches_the_route_for_the_next_run(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, Some(registry()?), "", async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let renamed = patch(f, id, &cookie, "rename", &json!({"title":"  Renamed  "})).await?;
        assert_eq!(renamed.status(), 200);
        assert_eq!(stored(f, id, &cookie).await?["title"], "Renamed");
        for invalid in [
            json!({"title":"   "}),
            json!({"title":"x".repeat(201)}),
            json!({}),
            json!({"name":"x"}),
        ] {
            let refused = patch(f, id, &cookie, &format!("invalid-{invalid}"), &invalid).await?;
            assert_eq!(refused.status(), 400, "{invalid}");
        }
        let mut runtime = f.runtime(&conversation).await?;
        let switched = patch(f, id, &cookie, "switch", &json!({"route":"other"})).await?;
        assert_eq!(switched.status(), 200);
        assert_eq!(stored(f, id, &cookie).await?["model_route"], "other");
        let ClientFrame::SessionUpdate {
            route,
            fallback_routes,
        } = runtime.action().await?
        else {
            return Err("the live link must receive session.update".into());
        };
        // The new route's fallbacks come with it (074), with their own relay paths and the same request limit.
        let fallbacks: Vec<(&str, &str, Option<u32>)> = fallback_routes
            .iter()
            .map(|fallback| {
                (
                    fallback.name.as_str(),
                    fallback.relay_path.as_str(),
                    fallback.max_request_bytes,
                )
            })
            .collect();
        assert_eq!(
            fallbacks,
            [("spare", "/relay/r/spare", Some(16 * 1024 * 1024))]
        );
        assert_eq!(
            (route.name.as_str(), route.model.as_str()),
            ("other", "second")
        );
        assert_eq!(route.relay_path, "/relay/r/other");
        assert!(
            !serde_json::to_string(&route)?.contains("models.example"),
            "no upstream settings reach the runtime"
        );
        // The relay now accepts only the stored route.
        let token = f.backend.token(id).ok_or("runtime token")?;
        let old = f
            .client
            .post(f.url("/relay/r/local/chat/completions"))
            .bearer_auth(token.expose_secret())
            .json(&json!({"messages":[{"role":"user","content":"hello"}]}))
            .send()
            .await?;
        assert_eq!(old.status(), 403);
        // A replay returns the first response and sends nothing new.
        let replay = patch(f, id, &cookie, "switch", &json!({"route":"other"})).await?;
        assert_eq!(replay.status(), 200);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_044_route_change_is_refused_during_a_run_or_for_a_disallowed_route(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, Some(registry()?), "", async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let refused = patch(f, id, &cookie, "absent", &json!({"route":"absent"})).await?;
        assert_eq!(refused.status(), 403);
        assert_eq!(stored(f, id, &cookie).await?["model_route"], "local");
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
        let during = patch(
            f,
            id,
            &cookie,
            "during",
            &json!({"route":"other","title":"Also refused"}),
        )
        .await?;
        assert_eq!(during.status(), 409);
        assert_eq!(during.json::<Value>().await?["error"]["code"], "run_active");
        let after = stored(f, id, &cookie).await?;
        assert_eq!(
            (after["model_route"].as_str(), after["title"].as_str()),
            (Some("local"), Some("Fixture"))
        );
        // A rename alone is allowed during a run.
        let renamed = patch(
            f,
            id,
            &cookie,
            "rename-during",
            &json!({"title":"Renamed while running"}),
        )
        .await?;
        assert_eq!(renamed.status(), 200);
        // Another user cannot patch the conversation.
        let (_, other) = f.member("other@example.invalid").await?;
        let foreign = patch(f, id, &other, "foreign", &json!({"title":"Taken"})).await?;
        assert_eq!(foreign.status(), 403);
        Ok(())
    })
    .await
}
