//! GW-I-039 (route limiter, LLM-U-012): a route with `concurrency = 1` holds a second request while the first
//! streams, refuses one more than the queue holds with 429 and `Retry-After` without contacting the model, and meters
//! only forwarded requests.

#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use support::TestResult;
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_a_route_limiter_holds_the_next_request_and_refuses_beyond_its_queue(
    pool: PgPool,
) -> TestResult {
    // The first reply streams until the model's fake clock moves, which keeps its permit held.
    let clock = FakeClock::new();
    let held = Script::new(vec![
        Scripted::Text("first".into()),
        Scripted::Delay(Duration::from_mins(1)),
        Scripted::Text(" done".into()),
    ]);
    let next = Script::new(vec![Scripted::Text("second".into())]);
    let server =
        FakeModelServer::start(vec![held, next], Arc::new(FakeSleeper::new(clock.clone()))).await?;
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nauth={{kind='none'}}\nconcurrency=1\n",
        server.base_url()
    ))?;
    let result = support::run_configured(
        pool,
        Some(registry),
        "[relay]\nqueue_max=1\n",
        async |fixture| {
            let (owner, _) = fixture.admin().await?;
            let conversation = fixture.conversation(owner.id).await?;
            let peer = std::net::Ipv4Addr::LOCALHOST.into();
            let token = fixture
                .gateway
                .runtime_tokens()
                .mint(&conversation.id, &"limited".parse()?, peer)
                .await?;
            let url = fixture.url("/relay/r/local/chat/completions");
            let body = json!({"messages":[{"role":"user","content":"hi"}],"stream":true});
            let post = || {
                let request = fixture
                    .client
                    .post(&url)
                    .bearer_auth(token.expose_secret())
                    .json(&body);
                tokio::spawn(request.send())
            };
            let first = post().await??;
            assert_eq!(first.status(), 200);
            // Two more arrive while the first streams: one takes the only queue place and waits for its permit, the
            // other is refused at once. The one that answers first is therefore the refused one.
            let (answered, waiting) = match futures::future::select(post(), post()).await {
                futures::future::Either::Left((answered, waiting))
                | futures::future::Either::Right((answered, waiting)) => (answered??, waiting),
            };
            let refused = answered;
            assert_eq!(refused.status(), 429);
            assert_eq!(
                refused
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok()),
                Some("1")
            );
            // GW-U-033: the relay's own refusal is marked as a route throttle.
            assert_eq!(
                refused
                    .headers()
                    .get("tqa-relay-throttle")
                    .and_then(|value| value.to_str().ok()),
                Some("route")
            );
            let error: serde_json::Value = refused.json().await?;
            assert_eq!(error["error"]["code"], "rate_limited");
            assert_eq!(
                server.requests().len(),
                1,
                "the refused and the queued request did not reach the model"
            );
            // The first stream ends; its permit frees the route and the queued request proceeds.
            clock.advance(Duration::from_mins(1))?;
            assert!(String::from_utf8_lossy(&first.bytes().await?).contains("done"));
            let waiting = tokio::time::timeout(support::DEADLINE, waiting).await???;
            assert_eq!(waiting.status(), 200);
            let waiting = waiting.bytes().await?;
            assert!(String::from_utf8_lossy(&waiting).contains("second"));
            assert_eq!(server.requests().len(), 2);
            let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM usage")
                .fetch_one(&fixture.pool)
                .await?;
            assert_eq!(rows, 2, "only forwarded requests are metered");
            Ok(())
        },
    )
    .await;
    server.shutdown().await?;
    result
}
