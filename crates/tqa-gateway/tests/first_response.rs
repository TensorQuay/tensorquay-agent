//! GW-I-150: each route's first-response bound reaches the runtime in session.config (its own setting, else 300 s for a
//! local route and 60 s otherwise, by the one local-route rule), and the model relay waits that long for the provider's
//! first byte: a slower one is refused as the gateway's own 504 `timeout`, a faster one goes through.
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use support::TestResult;
use tqa_testkit::{FakeModelServer, Script, Scripted};

fn chat() -> Value {
    json!({"model":"m","stream":true,"messages":[{"role":"user","content":"hi"}]})
}

// TEST: GW-I-150
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_150_session_config_carries_each_routes_first_response_bound(
    pool: PgPool,
) -> TestResult {
    let registry = tqa_llm::Registry::from_toml(concat!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='http://127.0.0.1:11434/v1'\nmodel='m'\n",
        "[[routes]]\nname='hosted'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='m'\n",
        "auth={kind='bearer'}\nkey_ref='secrets/hosted'\n",
        "[[routes]]\nname='patient'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='m'\n",
        "auth={kind='bearer'}\nkey_ref='secrets/hosted'\nfirst_response_secs=120\n",
    ))?;
    support::run_configured(pool, Some(registry), "", async |fixture| {
        let (owner, _) = fixture.admin().await?;
        fixture
            .gateway
            .secret_store()
            .put(
                &"secrets/hosted".parse()?,
                secrecy::SecretString::from("synthetic-key"),
            )
            .await?;
        for (route, expected) in [("local", 300), ("hosted", 60), ("patient", 120)] {
            let conversation = fixture
                .gateway
                .create_for_user(owner.id, "Fixture", route)
                .await?;
            let runtime = fixture.runtime(&conversation).await?;
            assert_eq!(
                runtime.config["route"]["first_response_secs"],
                json!(expected),
                "{route}"
            );
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-150
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_150_the_relay_waits_the_routes_bound_for_the_first_byte(pool: PgPool) -> TestResult {
    let slow = |seconds| Script {
        header_delay: Duration::from_secs(seconds),
        events: vec![Scripted::Text("ok".into())],
    };
    let server =
        FakeModelServer::start(vec![slow(8), slow(2)], Arc::new(tqa_llm::TokioSleeper)).await?;
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='slow'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nfirst_response_secs=5\n",
        server.base_url()
    ))?;
    support::run_configured(pool, Some(registry), "", async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture
            .gateway
            .create_for_user(owner.id, "Fixture", "slow")
            .await?;
        let token = fixture
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"relay-slow".parse()?,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        let relay = || {
            fixture
                .client
                .post(fixture.url("/relay/r/slow/chat/completions"))
                .bearer_auth(token.expose_secret())
                .json(&chat())
                .send()
        };
        // Past the route's 5 s: the gateway's own refusal, named as such, not the global 60 s.
        let refused = relay().await?;
        assert_eq!(refused.status(), 504);
        assert_eq!(
            refused
                .headers()
                .get(tqa_types::GATEWAY_ERROR_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("timeout")
        );
        let body: Value = refused.json().await?;
        assert_eq!(body["error"]["code"], "timeout");
        // Within it: answered.
        let answered = relay().await?;
        assert_eq!(answered.status(), 200);
        assert!(answered.text().await?.contains("ok"));
        Ok(())
    })
    .await
}
