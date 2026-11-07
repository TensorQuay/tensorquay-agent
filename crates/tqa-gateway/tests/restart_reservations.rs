//! GW-I-252 (E2E-AGENTS-020's try 5, 7 Oct 2026): a gateway restart that interrupts a sandbox create leaves its
//! runtime-token reservation behind (reserved, never bound or abandoned). Reservations are one per conversation, and a
//! reservation can only belong to a create of a process that is gone, so the restarted gateway clears them at its
//! start. Before that, the conversation's next start collided with the leftover (`sandbox_token_exists`) and its queued
//! turn failed until the reservation expired.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use support::TestResult;
use uuid::Uuid;

// TEST: GW-I-252
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_252_a_reservation_a_restart_left_behind_fails_no_start(pool: PgPool) -> TestResult {
    support::run_restarting(pool, async |f| {
        let (owner, _) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let conversation = Uuid::parse_str(task.id.as_str())?;
        // A create the shutdown interrupted: its reservation, unexpired, and nothing else.
        sqlx::query(
            "INSERT INTO runtime_token_reservations (id, conversation_id, token_hash, expires_at) \
             VALUES ($1, $2, $3, CURRENT_TIMESTAMP + INTERVAL '10 minutes')",
        )
        .bind(Uuid::now_v7())
        .bind(conversation)
        .bind(vec![7_u8; 32])
        .execute(&f.pool)
        .await?;
        f.restart().await?;
        let (_, cookie) = f.login("admin@example.invalid").await?;
        let sent = f
            .action(
                &format!("/api/v1/conversations/{}/messages", task.id),
                &cookie,
                Some("after-restart"),
                &json!({"text": "carry on"}),
            )
            .await?;
        assert_eq!(sent.status(), 202);
        // The start goes ahead: its sandbox is made, its runtime registers and gets the turn.
        let mut runtime = f.runtime(&task).await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        let failed: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE conversation_id = $1 AND execution_status = 'failed'",
        )
        .bind(conversation)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(failed, 0, "no queued turn failed");
        Ok(())
    })
    .await
}
