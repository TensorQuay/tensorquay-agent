//! GW-I-008: the Gate 1 relay contract covers only `openai_chat`.
//! The other relay protocols remain open for Phase 5.

#[path = "support/relay_upstream.rs"]
mod relay_upstream;
#[path = "support/fixture.rs"]
mod support;

use relay_upstream::Upstream;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::PgPool;
use std::net::Ipv4Addr;
use support::TestResult;

// Deliberate whitespace and opaque fields must survive transparent forwarding.
const TEXT: &str = r#"{ "choices": [{"index":0,"delta":{"content":"héllo"},"finish_reason":null}], "opaque":"preserved" }"#;
const USAGE: &str = r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":4},"completion_tokens_details":{"reasoning_tokens":2}}}"#;

fn registry(server: &Upstream) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='pinned-model'\nauth={{kind='bearer'}}\nkey_ref='secrets/local'\nstream_usage=true\nextra_body={{extension={{enabled=true}}}}\n\n[[routes]]\nname='other'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='other-model'\nauth={{kind='none'}}\n",
        base = server.base_url()
    ))?)
}

struct Caller {
    conversation: tqa_gateway::Conversation,
    token: SecretString,
    provider: SecretString,
}

async fn caller(fixture: &support::Fixture) -> Result<Caller, Box<dyn std::error::Error>> {
    let (owner, _) = fixture.admin().await?;
    let conversation = fixture.conversation(owner.id).await?;
    let token = fixture
        .gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &"basic-relay-sandbox".parse()?,
            Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    let provider = SecretString::from(format!("synthetic-provider-{}", uuid::Uuid::now_v7()));
    fixture
        .gateway
        .secret_store()
        .put(&"secrets/local".parse()?, provider.clone())
        .await?;
    Ok(Caller {
        conversation,
        token,
        provider,
    })
}

fn completion(fixture: &support::Fixture, route: &str) -> reqwest::RequestBuilder {
    fixture
        .client
        .post(fixture.url(&format!("/relay/r/{route}/chat/completions")))
        .header("x-tqa-run-id", "r-1")
        .json(&json!({
            "model":"caller-selected-model",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true,
            "stream_options":{"include_usage":false},
            "extension":{"enabled":false},
            "custom":{"preserved":true}
        }))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_008_invalid_and_other_conversation_tokens_never_reach_upstream(
    pool: PgPool,
) -> TestResult {
    let server = Upstream::start(TEXT, USAGE).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let caller = caller(fixture).await?;
        fixture
            .gateway
            .runtime_tokens()
            .revoke(&"basic-relay-sandbox".parse()?)
            .await?;
        let other = fixture.conversation(caller.conversation.owner_id).await?;
        // A different sandbox has a different peer address; forwarding headers cannot impersonate it.
        let foreign = fixture
            .gateway
            .runtime_tokens()
            .mint(
                &other.id,
                &"other-sandbox".parse()?,
                std::net::Ipv6Addr::LOCALHOST.into(),
            )
            .await?;
        for (label, token) in [
            ("missing", None),
            ("malformed", Some("invalid")),
            ("revoked", Some(caller.token.expose_secret())),
            ("another conversation", Some(foreign.expose_secret())),
        ] {
            let request = completion(fixture, "local").header("x-forwarded-for", "::1");
            let request = match token {
                Some(token) => request.bearer_auth(token),
                None => request,
            };
            assert_eq!(request.send().await?.status(), 401, "{label}");
            assert!(server.requests().is_empty(), "{label} reached upstream");
        }
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_008_valid_token_cannot_select_another_registered_route(pool: PgPool) -> TestResult {
    let server = Upstream::start(TEXT, USAGE).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let caller = caller(fixture).await?;
        let response = completion(fixture, "other")
            .bearer_auth(caller.token.expose_secret())
            .send()
            .await?;
        assert_eq!(response.status(), 403);
        assert!(server.requests().is_empty());
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_008_injects_route_credential_streams_exact_bytes_and_meters_once(
    pool: PgPool,
) -> TestResult {
    let server = Upstream::start(TEXT, USAGE).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let caller = caller(fixture).await?;
        // The run the call names is the task's own, so the usage row records it (GW-I-151).
        fixture
            .gateway
            .events()
            .append(&caller.conversation.id, support::started(1, "r-1")?)
            .await?;
        let mut response = completion(fixture, "local")
            .bearer_auth(caller.token.expose_secret())
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        let first = format!("data: {TEXT}\n\n");
        let mut received = Vec::new();
        while received.len() < first.len() {
            received.extend_from_slice(
                &response
                    .chunk()
                    .await?
                    .ok_or("stream ended before first record")?,
            );
        }
        assert_eq!(received, first.as_bytes());
        server.release();
        received.extend_from_slice(&response.bytes().await?);
        assert_eq!(
            received,
            format!("{first}data: {USAGE}\n\ndata: [DONE]\n\n").as_bytes()
        );
        assert_upstream(&server, &caller);
        assert_usage(fixture, &caller.conversation).await
    })
    .await;
    server.shutdown().await?;
    result
}

fn assert_upstream(server: &Upstream, caller: &Caller) {
    let requests = server.requests();
    assert_eq!(
        requests.len(),
        1,
        "one completion must be forwarded exactly once"
    );
    let upstream = &requests[0];
    let expected = format!("Bearer {}", caller.provider.expose_secret());
    assert!(
        upstream
            .headers
            .get("authorization")
            .is_some_and(|value| value.as_bytes() == expected.as_bytes()),
        "upstream authorization must contain the route credential from the secret store"
    );
    assert!(
        !upstream.headers.values().any(|value| value
            .to_str()
            .is_ok_and(|value| value.contains(caller.token.expose_secret()))),
        "the runtime token must never reach upstream headers"
    );
    assert!(
        !String::from_utf8_lossy(&upstream.encoded_body).contains(caller.token.expose_secret()),
        "the runtime token must never reach the upstream body"
    );
    assert_eq!(upstream.body["model"], "pinned-model");
    assert_eq!(upstream.body["extension"], json!({"enabled":true}));
    assert_eq!(upstream.body["custom"], json!({"preserved":true}));
    assert_eq!(
        upstream.body["messages"],
        json!([{"role":"user","content":"hello"}])
    );
    assert_eq!(
        upstream.body["stream_options"],
        json!({"include_usage":true})
    );
}

async fn assert_usage(
    fixture: &support::Fixture,
    conversation: &tqa_gateway::Conversation,
) -> TestResult {
    let counts = tokio::time::timeout(support::DEADLINE, async {
        loop {
            let rows = sqlx::query!(
                "SELECT prompt_tokens,completion_tokens,cached_tokens,reasoning_tokens FROM usage"
            )
            .fetch_all(&fixture.pool)
            .await?;
            assert_eq!(rows.len(), 1);
            let row = &rows[0];
            if row.prompt_tokens.is_some() {
                return Ok::<_, sqlx::Error>((
                    row.prompt_tokens,
                    row.completion_tokens,
                    row.cached_tokens,
                    row.reasoning_tokens,
                ));
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    assert_eq!(counts, (Some(12), Some(7), Some(4), Some(2)));
    let rows = sqlx::query!("SELECT route,run_id,conversation_id FROM usage")
        .fetch_all(&fixture.pool)
        .await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].route, "local");
    assert_eq!(rows[0].run_id.as_deref(), Some("r-1"));
    assert_eq!(
        rows[0].conversation_id.map(|id| id.to_string()).as_deref(),
        Some(conversation.id.as_str())
    );
    Ok(())
}
