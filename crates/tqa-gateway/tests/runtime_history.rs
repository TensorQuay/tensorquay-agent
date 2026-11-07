//! GW-I-005: runtime history is paged by its authenticated conversation and transport address.
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_005_runtime_history_pages_exactly_and_rejects_invalid_bindings(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        for seq in 1..=3 {
            fixture
                .gateway
                .events()
                .append(&conversation.id, support::frame(seq, "history"))
                .await?;
        }
        let tokens = fixture.gateway.runtime_tokens();
        let sandbox = "history-sandbox".parse()?;
        let token = tokens
            .mint(
                &conversation.id,
                &sandbox,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        for (after, expected) in [(0, json!([1, 2])), (2, json!([3])), (3, json!([]))] {
            let path = format!("/runtime/history?after={after}&limit=2");
            let response = fixture
                .client
                .get(fixture.url(&path))
                .bearer_auth(token.expose_secret())
                .send()
                .await?;
            assert_eq!(response.status(), 200);
            let page: serde_json::Value = response.json().await?;
            let actual = page["items"]
                .as_array()
                .ok_or("missing items")?
                .iter()
                .map(|frame| frame["seq"].clone())
                .collect::<Vec<_>>();
            assert_eq!(json!(actual), expected);
            assert_eq!(
                page["next_after"],
                expected
                    .as_array()
                    .and_then(|items| items.last())
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
            );
        }
        assert_eq!(
            fixture.get("/runtime/history", &cookie).await?.status(),
            401
        );
        for query in [
            "limit=0",
            "limit=1001",
            "after=18446744073709551615",
            "conversation_id=other",
        ] {
            assert_eq!(
                fixture
                    .client
                    .get(fixture.url(&format!("/runtime/history?{query}")))
                    .bearer_auth(token.expose_secret())
                    .send()
                    .await?
                    .status(),
                400
            );
        }
        tokens.revoke(&sandbox).await?;
        assert_eq!(
            fixture
                .client
                .get(fixture.url("/runtime/history"))
                .bearer_auth(token.expose_secret())
                .send()
                .await?
                .status(),
            401
        );
        let wrong = tokens
            .mint(
                &conversation.id,
                &"wrong-history-address".parse()?,
                std::net::Ipv6Addr::LOCALHOST.into(),
            )
            .await?;
        assert_eq!(
            fixture
                .client
                .get(fixture.url("/runtime/history"))
                .bearer_auth(wrong.expose_secret())
                .header("x-forwarded-for", "::1")
                .send()
                .await?
                .status(),
            401
        );
        Ok(())
    })
    .await
}
