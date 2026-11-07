//! GW-I-006: failed bootstrap never leaves a usable credential or a capacity reservation.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{TestResult, run};
use tqa_sandbox::SandboxBackend;

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_create_and_address_failures_release_reservations_and_fail_accepted_work(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        for fail_address in [false, true] {
            fixture.backend.fail_create.store(!fail_address, Ordering::SeqCst);
            fixture.backend.fail_address.store(fail_address, Ordering::SeqCst);
            let conversation = fixture.conversation(owner.id).await?;
            let mut stream = fixture.stream(&conversation.id, &cookie, 0).await?;
            let path = format!("/api/v1/conversations/{}/messages", conversation.id);
            let key = format!("bootstrap-failure-{fail_address}");
            let accepted = fixture.action(&path, &cookie, Some(&key), &json!({"text":"start"})).await?;
            assert_eq!(accepted.status(), 202);
            let (_, frame) = stream.next().await?;
            assert!(matches!(frame, tqa_types::ServerFrame::Error {
                seq: None, code: tqa_types::ErrorCode::SandboxFailed, retryable: true, ..
            }));
            assert!(fixture.backend.list().await?.is_empty());
            let status: String = sqlx::query_scalar("SELECT execution_status FROM idempotency WHERE key=$1")
                .bind(&key).fetch_one(&fixture.pool).await?;
            assert_eq!(status, "failed");
            // The failure is reported before cleanup (so a failing cleanup never hides it): wait for the cleanup.
            tokio::time::timeout(support::DEADLINE, async {
                loop {
                    let residual: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM runtime_tokens)+(SELECT count(*) FROM runtime_token_reservations)+(SELECT count(*) FROM sandbox_instances)")
                        .fetch_one(&fixture.pool).await?;
                    if residual == 0 {
                        return Ok::<(), sqlx::Error>(());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            }).await??;
        }
        assert_eq!(fixture.backend.creates.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.backend.destroys.load(Ordering::SeqCst), 1);
        Ok(())
    }).await
}
