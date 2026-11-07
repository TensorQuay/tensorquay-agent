//! GW-I-005: expiry of a replay cache must not erase accepted, undelivered work.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, run_configured};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_005_pending_actions_survive_retention_and_key_reuse(pool: PgPool) -> TestResult {
    run_configured(pool, None, "[limits]\nidempotency_purge_seconds=1", async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        let body = json!({"text":"still pending"});
        assert_eq!(fixture.action(&path, &cookie, Some("pending"), &body).await?.status(), 202);
        sqlx::query("UPDATE idempotency SET created_at=CURRENT_TIMESTAMP-INTERVAL '25 hours'")
            .execute(&fixture.pool).await?;
        let reuse = fixture.action(&path, &cookie, Some("pending"), &json!({"text":"different"})).await?;
        assert_eq!(reuse.status(), 422);
        // An expired terminal row is a deterministic witness that the periodic sweep ran.
        sqlx::query("INSERT INTO idempotency (key,user_id,route,response_status,response_body,created_at) VALUES ('sweep-witness',$1,'witness',202,'{}',CURRENT_TIMESTAMP-INTERVAL '25 hours')")
            .bind(owner.id).execute(&fixture.pool).await?;
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let witness: i64 = sqlx::query_scalar("SELECT count(*) FROM idempotency WHERE key='sweep-witness'")
                    .fetch_one(&fixture.pool).await?;
                if witness == 0 { return Ok::<(), sqlx::Error>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM idempotency WHERE key='pending' AND execution_status='queued'")
            .fetch_one(&fixture.pool).await?;
        assert_eq!(pending, 1);
        let mut runtime = fixture.runtime(&conversation).await?;
        assert!(matches!(runtime.action().await?, tqa_types::ClientFrame::UserMessage {text,..} if text=="still pending"));
        Ok(())
    }).await
}
