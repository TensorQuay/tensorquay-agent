//! GW-I-074 (ADR-0021 amendment): when the per-sandbox network pool is exhausted, accepted work waits in the queue
//! instead of failing, the episode is counted, and the work starts once a subnet is free again.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult, run};

async fn metrics(f: &Fixture) -> Result<String, Box<dyn std::error::Error>> {
    use tower::ServiceExt;
    let response = f
        .gateway
        .metrics_router()
        .oneshot(axum::http::Request::get("/metrics").body(axum::body::Body::empty())?)
        .await?;
    Ok(String::from_utf8(
        http_body_util::BodyExt::collect(response.into_body())
            .await?
            .to_bytes()
            .to_vec(),
    )?)
}

// TEST: GW-I-074
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_074_an_exhausted_network_pool_queues_work_until_a_subnet_is_free(
    pool: PgPool,
) -> TestResult {
    run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        f.backend.pool_exhausted.store(true, Ordering::SeqCst);
        let conversation = f.conversation(owner.id).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        let accepted = f
            .action(&path, &cookie, Some("pool-full"), &json!({"text":"start"}))
            .await?;
        assert_eq!(accepted.status(), 202);
        // The create finds no subnet: its admission record and token reservation are released and admission tries
        // again (every pass re-admits and releases it, so the store is sampled only for the failure that must not
        // happen), and nothing fails.
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let failed: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE key='pool-full' AND execution_status='failed'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(failed, 0);
        let status: String =
            sqlx::query_scalar("SELECT execution_status FROM idempotency WHERE key='pool-full'")
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(status, "queued", "the work waits instead of failing");
        assert!(
            metrics(f)
                .await?
                .contains("tqa_sandbox_network_pool_exhausted_total"),
            "the episode is counted"
        );
        // A subnet is free again: the next admission pass starts the sandbox and the work runs.
        f.backend.pool_exhausted.store(false, Ordering::SeqCst);
        f.gateway.sandboxes().admission_pass_now().await?;
        // The pass's worker makes the sandbox: wait for it, then connect its runtime with that sandbox's own token. The
        // fixture's `runtime` asked for a sandbox itself (`ensure`) beside the worker's, and on a loaded runner the
        // second registration was refused `sandbox_token_exists` (main's CI on 3388b41a9, 23 Nov 2026).
        // The token authenticates only once the gateway has bound it to the sandbox (runtime_tokens' sandbox_id), which
        // happens after the backend's create returns: connecting when the fake backend has the token but the binding is
        // not stored yet answered 401 on a loaded runner (PR #168's CI, 23 Nov 2026). Wait for both.
        let id = uuid::Uuid::parse_str(&conversation.id.to_string())?;
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let bound: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM runtime_tokens WHERE conversation_id = $1 AND sandbox_id IS NOT NULL",
                )
                .bind(id)
                .fetch_one(&f.pool)
                .await
                .unwrap_or(0);
                if bound > 0 && matches!(f.backend.live_token(&conversation.id).await, Ok(Some(_))) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await?;
        let _runtime = f.runtime_of_sandbox(&conversation.id).await?;
        assert!(
            f.backend.created(&conversation.id).is_some(),
            "a sandbox exists"
        );
        Ok(())
    })
    .await
}
