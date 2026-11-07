//! GW-I-006: first-frame configuration is an explicit, secret-free snapshot.
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use support::TestResult;

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_first_frame_is_configuration_without_secrets_or_vendor_settings(
    pool: PgPool,
) -> TestResult {
    let registry = tqa_llm::Registry::from_toml(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='configured-model'\nauth={kind='bearer'}\nkey_ref='secrets/local'\ncontext_window=8192\nmax_output=1024\nnative_tools=true\nparallel_tools=true\nvision=false\nextra_body={extension='vendor-body-canary'}\nextra_headers={x-setting='vendor-header-canary'}\nreasoning={field='vendor-thinking-field',allowed=['low','high'],default='low'}",
    )?;
    support::run_configured(pool,Some(registry),"[run_limits]\nmax_steps=17\nwall_clock_secs=300\ntool_result_bytes=4096", async |fixture| {
        let (owner,cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let secret = secrecy::SecretString::from(format!("synthetic-provider-{}",uuid::Uuid::now_v7()));
        fixture.gateway.secret_store().put(&"secrets/local".parse()?,secret.clone()).await?;
        fixture.gateway.events().append(&conversation.id,support::frame(1,"stored")).await?;
        let mut runtime = fixture.runtime(&conversation).await?;
        // The install CA the sandbox trusts for keyed hosts (ADR-0037 §6), one PEM certificate before any rotation;
        // the rest of the frame is as before.
        let host_ca = runtime.config.as_object_mut().and_then(|config| config.remove("host_ca")).ok_or("no host_ca")?;
        let host_ca = host_ca.as_str().ok_or("host_ca is text")?;
        assert!(host_ca.starts_with("-----BEGIN CERTIFICATE-----\n") && host_ca.matches("-----BEGIN").count() == 1, "{host_ca}");
        assert_eq!(runtime.config,json!({"type":"session.config","gateway_revision":tqa_types::PROTOCOL_REVISION,"conversation_id":conversation.id,"last_stored_seq":1,"protocol":1,"environment":{"template":"minimal","digest":"example/runtime@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","capabilities":["shell","python"]},"network":{"allowed":[],"more":0},"limits":{"max_steps":17,"wall_clock_secs":300,"tool_result_bytes":4096,"max_output_tokens":131_072},"workspace":{"display":"/workspace"},"route":{"name":"local","protocol":"openai_chat","model":"configured-model","relay_path":"/relay/r/local","context_window":8192,"max_output":1024,"native_tools":true,"parallel_tools":true,"vision":false,"reasoning":{"allowed":["low","high"],"default":"low"},"max_request_bytes":16_777_216,"first_response_secs":60}}));
        let serialized = serde_json::to_string(&runtime.config)?;
        for forbidden in [secret.expose_secret(),"vendor-body-canary","vendor-header-canary","vendor-thinking-field","extra_body","extra_headers","key_ref","base_url"] { assert!(!serialized.contains(forbidden),"forbidden route setting reached runtime"); }
        let response = fixture.action(&format!("/api/v1/conversations/{}/messages",conversation.id),&cookie,Some("after-config"),&json!({"text":"after"})).await?;
        assert_eq!(response.status(),202);
        assert!(matches!(runtime.action().await?,tqa_types::ClientFrame::UserMessage { text,.. } if text=="after"));
        Ok(())
    }).await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_session_config_carries_no_permissions_since_adr_0035(pool: PgPool) -> TestResult {
    support::run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let runtime = fixture.runtime(&conversation).await?;
        let serialized = runtime.config.to_string();
        for removed in [
            "\"policy\"",
            "\"rules\"",
            "\"push\"",
            "approval",
            "\"class\"",
        ] {
            assert!(!serialized.contains(removed), "{removed} in {serialized}");
        }
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_session_config_carries_the_routes_fallbacks_in_order(pool: PgPool) -> TestResult {
    let route = |name: &str, fallbacks: &str| {
        format!(
            "[[routes]]\nname='{name}'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='{name}-model'\nauth={{kind='none'}}\nfallbacks=[{fallbacks}]\n"
        )
    };
    // A cycle among fallbacks is refused when the registry loads, before any gateway starts with it.
    let cycle = format!("{}{}", route("a", "'b'"), route("b", "'a'"));
    assert!(tqa_llm::Registry::from_toml(&cycle).is_err());
    let registry = tqa_llm::Registry::from_toml(&format!(
        "{}{}{}",
        route("local", "'second', 'third'"),
        route("second", ""),
        route("third", "")
    ))?;
    support::run_configured(
        pool,
        Some(registry),
        "[relay]\nbody_bytes=2097152\n",
        async |fixture| {
            let (owner, _) = fixture.admin().await?;
            let conversation = fixture.conversation(owner.id).await?;
            let runtime = fixture.runtime(&conversation).await?;
            let fallbacks = runtime.config["fallback_routes"]
                .as_array()
                .ok_or("fallback_routes")?;
            let names: Vec<_> = fallbacks
                .iter()
                .map(|route| {
                    (
                        &route["name"],
                        &route["relay_path"],
                        &route["max_request_bytes"],
                    )
                })
                .collect();
            assert_eq!(
                names,
                [
                    (
                        &json!("second"),
                        &json!("/relay/r/second"),
                        &json!(2_097_152)
                    ),
                    (&json!("third"), &json!("/relay/r/third"), &json!(2_097_152)),
                ]
            );
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-006 (revision 7): each route snapshot carries its cache_hint; `none` is left out.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_session_config_carries_each_routes_cache_hint(pool: PgPool) -> TestResult {
    let route = |name: &str, protocol: &str, hint: &str, fallbacks: &str| {
        format!(
            "[[routes]]\nname='{name}'\nprotocol='{protocol}'\nbase_url='https://models.example/v1'\nmodel='{name}-model'\n\
             auth={{kind='none'}}\nmax_output=4096\n{hint}fallbacks=[{fallbacks}]\n"
        )
    };
    let registry = tqa_llm::Registry::from_toml(&format!(
        "{}{}{}",
        route(
            "local",
            "anthropic_messages",
            "cache_hint='anthropic_cache_control'\n",
            "'second', 'third'"
        ),
        route("second", "openai_chat", "cache_hint='prefix'\n", ""),
        route("third", "openai_chat", "", "")
    ))?;
    support::run_configured(pool, Some(registry), "", async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let runtime = fixture.runtime(&conversation).await?;
        assert_eq!(
            runtime.config["route"]["cache_hint"],
            json!("anthropic_cache_control")
        );
        let fallbacks = runtime.config["fallback_routes"]
            .as_array()
            .ok_or("fallback_routes")?;
        assert_eq!(fallbacks[0]["cache_hint"], json!("prefix"));
        assert!(
            fallbacks[1].get("cache_hint").is_none(),
            "a route without a hint leaves the field out: {}",
            fallbacks[1]
        );
        Ok(())
    })
    .await
}
