//! GW-I-039 (Messages): the relay's Messages path pins the model, sends the route credential and the
//! pinned protocol version upstream, refuses what the route does not allow, and meters split usage.

#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use support::TestResult;
use tqa_testkit::{FakeModelServer, Script, Scripted};

/// A Messages route with a thinking budget table, and a Chat route declared as its fallback.
fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    let base = server.base_url();
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='anthropic_messages'\nbase_url='{base}'\nmodel='pinned-model'\n\
         auth={{kind='header',name='x-api-key'}}\nkey_ref='secrets/local'\nmax_output=4096\nfallbacks=['chat']\n\
         extra_headers={{anthropic-beta='route-feature'}}\n\
         reasoning={{field='thinking',allowed=['low'],default='low',budget_tokens={{low=2048}}}}\n\
         [[routes]]\nname='chat'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='chat-model'\nauth={{kind='none'}}\n"
    ))?)
}

fn body(budget: u32) -> Value {
    json!({"model":"untrusted-model","max_tokens":4096,"stream":true,
        "thinking":{"type":"enabled","budget_tokens":budget},
        "messages":[{"role":"user","content":[{"type":"text","text":"hi"}]},
            {"role":"assistant","content":[{"type":"thinking","thinking":"Plan.","signature":"opaque"},
                {"type":"tool_use","id":"c1","name":"read_file","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"c1","content":"x"}]}]})
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_the_messages_path_pins_the_route_and_meters_split_usage(
    pool: PgPool,
) -> TestResult {
    let usage = tqa_llm::Usage {
        prompt_tokens: 12,
        completion_tokens: 7,
        cached_tokens: Some(4),
        reasoning_tokens: None,
    };
    let script = Script::new(vec![
        Scripted::Reasoning("think".into()),
        Scripted::Text("héllo".into()),
        Scripted::Usage(usage),
    ]);
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
        let input = body(2048);
        let response = fixture
            .client
            .post(fixture.url("/relay/r/local/messages"))
            .bearer_auth(token.expose_secret())
            .header("X-TQA-Run-ID", "relay-run")
            .header("anthropic-version", "1999-01-01")
            .json(&input)
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        let bytes = response.bytes().await?;
        let direct = fixture
            .client
            .post(format!("{}/messages", server.base_url()))
            .json(&input)
            .send()
            .await?
            .bytes()
            .await?;
        assert_eq!(bytes, direct, "the stream passes through unchanged");
        let requests = server.requests();
        let upstream = requests.first().ok_or("missing upstream request")?;
        assert_eq!(upstream.path, "/v1/messages");
        assert_eq!(upstream.body["model"], "pinned-model");
        assert_eq!(upstream.body["thinking"], input["thinking"]);
        assert_eq!(upstream.body["messages"], input["messages"]);
        assert_eq!(
            upstream
                .headers
                .get("anthropic-version")
                .map(String::as_str),
            Some("2023-06-01")
        );
        assert!(
            upstream.headers.contains_key("x-api-key")
                && upstream.headers.contains_key("anthropic-beta")
        );
        assert!(
            !upstream.headers.contains_key("authorization"),
            "the runtime token never goes upstream"
        );
        assert!(!String::from_utf8_lossy(&upstream.encoded_body).contains(token.expose_secret()));
        let row = wait_usage(&fixture.pool).await?;
        assert_eq!(row, (Some(12), Some(7), Some(4), None));
        // Refusals never reach the model: an unmapped budget, and paths of another protocol.
        let refused = [
            ("/relay/r/local/messages", body(4096), 400),
            (
                "/relay/r/local/chat/completions",
                json!({"messages":[]}),
                404,
            ),
            ("/relay/r/chat/messages", body(2048), 404),
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
