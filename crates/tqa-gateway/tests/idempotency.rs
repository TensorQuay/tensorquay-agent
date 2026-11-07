//! GW-I-005: request fingerprints and bounded retention prevent silent action loss.

#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, run, run_configured};
use tqa_types::ClientFrame;

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_005_reusing_a_key_with_a_different_body_returns_422(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (user, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(user.id).await?;
        let mut runtime = fixture.runtime(&conversation).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        let first = fixture
            .action(&path, &cookie, Some("body-key"), &json!({"text":"first"}))
            .await?;
        assert_eq!(first.status(), 202);
        let accepted = first.json::<serde_json::Value>().await?;
        assert!(
            matches!(runtime.action().await?,ClientFrame::UserMessage {text,..} if text=="first")
        );
        let duplicate = fixture
            .action(
                &path,
                &cookie,
                Some("body-key"),
                &json!({"attachments":[],"text":"first"}),
            )
            .await?;
        assert_eq!(duplicate.status(), 202);
        assert_eq!(duplicate.json::<serde_json::Value>().await?, accepted);
        for body in [
            json!({"text":"different"}),
            json!({"text":"first","attachments":["uploads/attachment-one"]}),
        ] {
            let response = fixture
                .action(&path, &cookie, Some("body-key"), &body)
                .await?;
            assert_eq!(response.status(), 422);
            assert_eq!(
                response.json::<serde_json::Value>().await?["error"]["code"],
                "idempotency_key_reused"
            );
        }
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_005_expired_keys_accept_a_new_body_after_24_hours(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (user, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(user.id).await?;
        let mut runtime = fixture.runtime(&conversation).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        assert_eq!(
            fixture
                .action(
                    &path,
                    &cookie,
                    Some("expired-key"),
                    &json!({"text":"before"})
                )
                .await?
                .status(),
            202
        );
        runtime.action().await?;
        // The worker marks the action delivered just after the runtime receives it, and a queued action's key is never
        // forgotten; so age the record only once it has left the queue. Bounded: this observes the real worker.
        tokio::time::timeout(support::DEADLINE, async {
            while sqlx::query_scalar::<_, bool>(
                "SELECT execution_status='queued' FROM idempotency WHERE key='expired-key'",
            )
            .fetch_one(&fixture.pool)
            .await?
            {
                tokio::task::yield_now().await;
            }
            Ok::<_, sqlx::Error>(())
        })
        .await??;
        sqlx::query!("UPDATE idempotency SET created_at=CURRENT_TIMESTAMP-INTERVAL '25 hours'")
            .execute(&fixture.pool)
            .await?;
        assert_eq!(
            fixture
                .action(
                    &path,
                    &cookie,
                    Some("expired-key"),
                    &json!({"text":"after"})
                )
                .await?
                .status(),
            202
        );
        assert!(
            matches!(runtime.action().await?,ClientFrame::UserMessage {text,..} if text=="after")
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_005_periodic_purge_removes_expired_records_only(pool: PgPool) -> TestResult {
    run_configured(pool,None,"[limits]\nidempotency_hours=1\nidempotency_purge_seconds=1",async |fixture| {
        let (user,cookie)=fixture.admin().await?;
        let conversation=fixture.conversation(user.id).await?;
        let mut runtime=fixture.runtime(&conversation).await?;
        let path=format!("/api/v1/conversations/{}/messages",conversation.id);
        for key in ["expired","retained"] {
            assert_eq!(fixture.action(&path,&cookie,Some(key),&json!({"text":"message"})).await?.status(),202);
            runtime.action().await?;
        }
        sqlx::query!("UPDATE idempotency SET created_at=CURRENT_TIMESTAMP-INTERVAL '2 hours' WHERE key='expired'").execute(&fixture.pool).await?;
        tokio::time::timeout(support::DEADLINE,async {
            loop {
                let keys=sqlx::query_scalar!("SELECT key FROM idempotency ORDER BY key").fetch_all(&fixture.pool).await?;
                if keys==["retained"] {return Ok::<_,sqlx::Error>(());}
                tokio::task::yield_now().await;
            }
        }).await??;
        Ok(())
    }).await
}
