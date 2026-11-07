//! GW-I-098: a route's `instructions_role` reaches the upstream through the relay; a route without it sends the
//! runtime's bytes; a value its protocol doesn't accept is refused when the route is saved.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use support::{TestResult, call};
use tqa_testkit::{FakeModelServer, Script, Scripted};

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    let base = server.base_url();
    let route = |name: &str, protocol: &str, role: &str| {
        format!(
            "[[routes]]\nname='{name}'\nprotocol='{protocol}'\nbase_url='{base}'\nmodel='m'\nauth={{kind='none'}}\n{role}"
        )
    };
    Ok(tqa_llm::Registry::from_toml(
        &[
            route("chat-dev", "openai_chat", "instructions_role='developer'\n"),
            route("chat", "openai_chat", ""),
            route(
                "responses-dev",
                "openai_responses",
                "instructions_role='developer'\n",
            ),
            route("responses", "openai_responses", ""),
        ]
        .concat(),
    )?)
}

/// The bodies a runtime sends: the portable default place for the instructions.
fn chat() -> Value {
    json!({"model":"m","stream":true,"messages":[
        {"role":"system","content":"Be brief."},{"role":"user","content":"hi"}]})
}
fn responses() -> Value {
    json!({"model":"m","stream":true,"store":false,"instructions":"Be brief.",
        "input":[{"type":"message","role":"user","content":"hi"}]})
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_098_the_routes_instructions_role_reaches_the_upstream(pool: PgPool) -> TestResult {
    let scripts = (0..4)
        .map(|_| Script::new(vec![Scripted::Text("ok".into())]))
        .collect();
    let server = FakeModelServer::start(scripts, Arc::new(tqa_llm::TokioSleeper)).await?;
    support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let (owner, _) = fixture.admin().await?;
        for (route, path, body) in [
            ("chat-dev", "chat/completions", chat()),
            ("chat", "chat/completions", chat()),
            ("responses-dev", "responses", responses()),
            ("responses", "responses", responses()),
        ] {
            // A conversation per route: a runtime token relays only to its conversation's route.
            let conversation = fixture
                .gateway
                .create_for_user(owner.id, "Fixture", route)
                .await?;
            let token = fixture
                .gateway
                .runtime_tokens()
                .mint(
                    &conversation.id,
                    &format!("relay-{route}").parse()?,
                    std::net::Ipv4Addr::LOCALHOST.into(),
                )
                .await?;
            let response = fixture
                .client
                .post(fixture.url(&format!("/relay/r/{route}/{path}")))
                .bearer_auth(token.expose_secret())
                .json(&body)
                .send()
                .await?;
            assert_eq!(response.status(), 200, "{route}");
            response.bytes().await?;
        }
        let upstream: Vec<Value> = server
            .requests()
            .into_iter()
            .map(|request| request.body)
            .collect();
        let [chat_dev, chat_default, responses_dev, responses_default] = upstream.as_slice() else {
            return Err(format!("expected 4 upstream requests, got {}", upstream.len()).into());
        };
        assert_eq!(chat_dev["messages"][0]["role"], "developer");
        assert_eq!(chat_dev["messages"][1], chat()["messages"][1]);
        assert_eq!(
            chat_default["messages"],
            chat()["messages"],
            "the default sends the runtime's messages"
        );
        assert!(
            responses_dev.get("instructions").is_none(),
            "{responses_dev}"
        );
        assert_eq!(
            responses_dev["input"][0],
            json!({"type":"message","role":"developer","content":"Be brief."})
        );
        assert_eq!(responses_default["instructions"], "Be brief.");
        assert_eq!(responses_default["input"], responses()["input"]);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_098_a_value_the_protocol_refuses_is_refused_when_the_route_is_saved(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let route = |protocol: &str, role: Option<&str>| {
            let mut body = json!({"protocol": protocol, "base_url": "https://models.example/v1", "model": "m",
                "auth": {"kind": "none"}, "max_output": 1000});
            if let Some(role) = role {
                body["instructions_role"] = json!(role);
            }
            body
        };
        for (protocol, role) in [
            ("openai_chat", "top_level"),
            ("openai_responses", "system"),
            ("anthropic_messages", "developer"),
        ] {
            let (status, refused) = call(
                f,
                Method::PUT,
                "/api/v1/admin/routes/case",
                &cookie,
                (Some(route(protocol, Some(role))), None),
            )
            .await?;
            let detail = &refused["error"]["details"][0];
            assert_eq!(
                (status, &detail["field"], &detail["reason"]),
                (400, &json!("instructions_role"), &json!("instructions_role_invalid")),
                "{protocol} {role}: {refused}"
            );
        }
        // Saved with the setting, it's shown; saved without, it's absent (its default depends on the protocol).
        for (name, role) in [("with", Some("developer")), ("without", None)] {
            let (status, _) = call(
                f,
                Method::PUT,
                &format!("/api/v1/admin/routes/{name}"),
                &cookie,
                (Some(route("openai_chat", role)), None),
            )
            .await?;
            assert!((200..300).contains(&status), "{name}: {status}");
            let (_, list) = call(f, Method::GET, "/api/v1/admin/routes", &cookie, (None, None)).await?;
            let shown = list["items"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|item| item["name"] == name)
                .cloned()
                .ok_or("route not listed")?;
            assert_eq!(shown.get("instructions_role").and_then(Value::as_str), role, "{name}: {shown}");
        }
        Ok(())
    })
    .await
}
