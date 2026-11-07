//! GW-I-039 (Responses): the relay's Responses path pins the model and the stateless options, maps
//! the reasoning effort, refuses what the route does not allow, and meters the terminal usage.

#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use support::TestResult;
use tqa_testkit::{FakeModelServer, Script, Scripted};

/// A Responses route with an effort policy, and a Chat route declared as its fallback.
fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    let base = server.base_url();
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_responses'\nbase_url='{base}'\nmodel='pinned-model'\n\
         auth={{kind='bearer'}}\nkey_ref='secrets/local'\nfallbacks=['chat']\n\
         reasoning={{field='reasoning_effort',allowed=['low','high'],default='low'}}\n\
         [[routes]]\nname='chat'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='chat-model'\nauth={{kind='none'}}\n"
    ))?)
}

fn body(effort: &str) -> Value {
    json!({"model":"untrusted-model","stream":false,"store":true,"reasoning":{"effort":effort},
        "input":[{"type":"message","role":"user","content":"hi"},
            {"type":"reasoning","id":"rs_1","encrypted_content":"opaque"}]})
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_the_responses_path_pins_the_route_and_meters_terminal_usage(
    pool: PgPool,
) -> TestResult {
    let usage = tqa_llm::Usage {
        prompt_tokens: 12,
        completion_tokens: 7,
        cached_tokens: Some(4),
        reasoning_tokens: Some(2),
    };
    let script = Script::new(vec![Scripted::Text("héllo".into()), Scripted::Usage(usage)]);
    let server = FakeModelServer::start(
        vec![script.clone(), script],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let peer = std::net::Ipv4Addr::LOCALHOST.into();
        let token = fixture
            .gateway
            .runtime_tokens()
            .mint(&conversation.id, &"relay-sandbox".parse()?, peer)
            .await?;
        let secret =
            secrecy::SecretString::from(format!("synthetic-provider-{}", uuid::Uuid::now_v7()));
        fixture
            .gateway
            .secret_store()
            .put(&"secrets/local".parse()?, secret.clone())
            .await?;
        let input = body("high");
        let response = fixture
            .client
            .post(fixture.url("/relay/r/local/responses"))
            .bearer_auth(token.expose_secret())
            .json(&input)
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        let bytes = response.bytes().await?;
        let direct = fixture
            .client
            .post(format!("{}/responses", server.base_url()))
            .json(&input)
            .send()
            .await?
            .bytes()
            .await?;
        assert_eq!(bytes, direct, "the stream passes through unchanged");
        let requests = server.requests();
        let upstream = requests.first().ok_or("missing upstream request")?;
        assert_eq!(upstream.path, "/v1/responses");
        assert_eq!(
            (
                &upstream.body["model"],
                &upstream.body["stream"],
                &upstream.body["store"]
            ),
            (&json!("pinned-model"), &json!(true), &json!(false))
        );
        assert_eq!(upstream.body["reasoning"], json!({"effort":"high"}));
        assert_eq!(
            upstream.body["input"], input["input"],
            "reasoning items pass through"
        );
        assert!(upstream.headers.contains_key("authorization"));
        assert!(!String::from_utf8_lossy(&upstream.encoded_body).contains(token.expose_secret()));
        let row = wait_usage(&fixture.pool).await?;
        assert_eq!(row, (Some(12), Some(7), Some(4), Some(2)));
        let refused = [
            ("/relay/r/local/responses", body("extreme"), 400),
            (
                "/relay/r/local/chat/completions",
                json!({"messages":[]}),
                404,
            ),
            ("/relay/r/chat/responses", body("low"), 404),
        ];
        for (path, request, status) in refused {
            let response = fixture
                .client
                .post(fixture.url(path))
                .bearer_auth(token.expose_secret())
                .json(&request)
                .send()
                .await?;
            assert_eq!(response.status(), status, "{path}");
        }
        assert_eq!(
            server.requests().len(),
            2,
            "only the accepted request and the direct call"
        );
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

type Counters = (Option<i64>, Option<i64>, Option<i64>, Option<i64>);
async fn wait_usage(pool: &PgPool) -> Result<Counters, Box<dyn std::error::Error>> {
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            let row = sqlx::query!(
                "SELECT prompt_tokens,completion_tokens,cached_tokens,reasoning_tokens FROM usage"
            )
            .fetch_one(pool)
            .await?;
            if row.prompt_tokens.is_some() {
                return Ok((
                    row.prompt_tokens,
                    row.completion_tokens,
                    row.cached_tokens,
                    row.reasoning_tokens,
                ));
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}
