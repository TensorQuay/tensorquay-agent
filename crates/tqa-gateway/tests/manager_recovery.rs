//! GW-I-011: process-local services can be replaced without losing accepted work.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{DEADLINE, TestResult, run_restarting};
use tqa_sandbox::SandboxBackend;

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_011_restart_replaces_a_missing_sandbox_and_revokes_its_binding(
    pool: PgPool,
) -> TestResult {
    Box::pin(run_restarting(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let original = fixture.gateway.sandboxes().ensure(&conversation.id).await?;
        let old_token = fixture.backend.token(&conversation.id).ok_or("token")?;
        fixture.gateway.cancel();
        fixture.backend.destroy(&original).await?;
        fixture.restart().await?;
        let runtime = fixture.runtime(&conversation).await?;
        assert_ne!(runtime.sandbox.as_str(), original.id);
        assert_eq!(fixture.backend.creates.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.backend.list().await?.len(), 1);
        assert_eq!(
            fixture
                .gateway
                .runtime_tokens()
                .resolve(&old_token, std::net::Ipv4Addr::LOCALHOST.into())
                .await?,
            None
        );
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_011_restart_finishes_interrupted_retirement_and_fails_its_queued_work(
    pool: PgPool,
) -> TestResult {
    run_restarting(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        fixture
            .action(&path, &cookie, Some("retiring"), &json!({"text":"pending"}))
            .await?
            .error_for_status()?;
        fixture.gateway.sandboxes().ensure(&conversation.id).await?;
        let token = fixture.backend.token(&conversation.id).ok_or("token")?;
        fixture.gateway.cancel();
        sqlx::query("UPDATE sandbox_instances SET phase='destroying'")
            .execute(&fixture.pool)
            .await?;
        fixture.restart().await?;
        assert!(fixture.backend.list().await?.is_empty());
        assert_eq!(fixture.backend.creates.load(Ordering::SeqCst), 1);
        assert_eq!(
            fixture
                .gateway
                .runtime_tokens()
                .resolve(&token, std::net::Ipv4Addr::LOCALHOST.into())
                .await?,
            None
        );
        let row: (String, Option<String>) = sqlx::query_as(
            "SELECT execution_status,execution_error FROM idempotency WHERE key='retiring'",
        )
        .fetch_one(&fixture.pool)
        .await?;
        assert_eq!(row, ("failed".into(), Some("sandbox_failed".into())));
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sandbox_instances")
            .fetch_one(&fixture.pool)
            .await?;
        assert_eq!(remaining, 0);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_011_restart_recovers_the_original_queued_action_without_reminting(
    pool: PgPool,
) -> TestResult {
    run_restarting(pool,async|fixture| {
        let (owner,cookie)=fixture.admin().await?;
        let conversation=fixture.conversation(owner.id).await?;
        let path=format!("/api/v1/conversations/{}/messages",conversation.id);
        let body=json!({"text":"durable before restart"});
        let response=fixture.action(&path,&cookie,Some("restart-key"),&body).await?;
        assert_eq!(response.status(),202);
        let original=response.bytes().await?;
        let stored:Vec<u8>=sqlx::query_scalar("SELECT queued_frame FROM idempotency WHERE key='restart-key'").fetch_one(&fixture.pool).await?;
        let frame:serde_json::Value=serde_json::from_slice(&stored)?;
        assert_eq!(frame,json!({"type":"user.message","text":"durable before restart","attachments":[]}));
        tokio::time::timeout(DEADLINE,async {
            loop {
                let live:i64=sqlx::query_scalar("SELECT count(*) FROM sandbox_instances WHERE phase='live'").fetch_one(&fixture.pool).await?;
                if live==1 {return Ok::<(),sqlx::Error>(());}
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await??;
        let before:Vec<u8>=sqlx::query_scalar("SELECT token_hash FROM runtime_tokens").fetch_one(&fixture.pool).await?;
        fixture.restart().await?;
        let replay=fixture.action(&path,&cookie,Some("restart-key"),&body).await?;
        assert_eq!(replay.status(),202);
        assert_eq!(replay.bytes().await?,original);
        let mut runtime=fixture.runtime(&conversation).await?;
        assert_eq!(serde_json::to_value(runtime.action().await?)?,frame);
        assert_eq!(fixture.backend.creates.load(Ordering::SeqCst),1);
        let after:Vec<u8>=sqlx::query_scalar("SELECT token_hash FROM runtime_tokens").fetch_one(&fixture.pool).await?;
        assert_eq!(before,after);
        fixture.action(&path,&cookie,Some("after-restart"),&json!({"text":"next"})).await?.error_for_status()?;
        assert!(matches!(runtime.action().await?,tqa_types::ClientFrame::UserMessage {text,..} if text=="next"));
        Ok(())
    }).await
}
