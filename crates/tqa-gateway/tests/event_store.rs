//! GW-I-005: durability, spill storage and user-scoped action idempotency.

#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_005_spilled_frames_round_trip_and_duplicate_or_missing_sequences_are_rejected(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        let frame = support::frame(1, &"x".repeat(70_000));
        fixture.gateway.events().append(id, frame.clone()).await?;
        assert_eq!(fixture.gateway.events().read(id, 0.into(), 100).await?, vec![frame.clone()]);
        assert!(fixture.gateway.events().append(id, frame).await.is_err());
        assert!(fixture.gateway.events().append(id, support::frame(3, "gap")).await.is_err());
        fixture.gateway.events().append(id, support::frame(2, "contiguous")).await?;
        let row = sqlx::query!("SELECT frame IS NULL AS \"spilled!\",blob_key FROM events WHERE conversation_id=$1 AND seq=1", uuid::Uuid::parse_str(id.as_str())?).fetch_one(&fixture.pool).await?;
        assert!(row.spilled);
        assert_eq!(row.blob_key, Some(format!("{id}/1")));
        let response = fixture.get(&format!("/api/v1/conversations/{id}/events?after=1&limit=1"), &cookie).await?;
        assert_eq!(response.status(), 200);
        let page: serde_json::Value = response.json().await?;
        assert_eq!(page["items"][0]["seq"], 2);
        assert_eq!(page["items"].as_array().ok_or("items")?.len(), 1);
        Ok(())
    }).await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_005_idempotency_is_scoped_to_the_user_and_route_and_forwards_once(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (first, first_cookie) = fixture.admin().await?;
        let (second, second_cookie) = fixture.member("second@example.invalid").await?;
        let mut responses = Vec::new();
        for (user, cookie) in [(first, first_cookie), (second, second_cookie)] {
            let created = fixture.conversation(user.id).await?;
            let id = &created.id;
            let path = format!("/api/v1/conversations/{id}/messages");
            let body = json!({"text":"hello", "attachments":[]});
            let pending=fixture.action(&path, &cookie, Some("shared-key"), &body).await?;
            assert_eq!(pending.status(), 202);
            assert_eq!(pending.headers().get("x-tqa-execution-status").ok_or("execution status")?,"queued");
            let mut runtime = fixture.runtime(&created).await?;
            let response = fixture.action(&path, &cookie, Some("shared-key"), &body).await?;
            assert_eq!(response.status(), 202);
            let accepted: serde_json::Value = response.json().await?;
            assert_eq!(accepted["conversation_id"], id.as_str());
            assert!(matches!(runtime.action().await?, tqa_types::ClientFrame::UserMessage { text, .. } if text == "hello"));
            let retry = fixture.action(&path, &cookie, Some("shared-key"), &body).await?;
            assert_eq!(retry.status(), 202);
            assert_eq!(retry.json::<serde_json::Value>().await?, accepted);
            let conflict = fixture.action(&format!("/api/v1/conversations/{id}/runs/run-one/cancel"), &cookie, Some("shared-key"), &json!({})).await?;
            assert_eq!(conflict.status(), 409);
            // The next action proves the retry did not enqueue a second user.message.
            assert_eq!(fixture.action(&format!("/api/v1/conversations/{id}/runs/run-one/cancel"), &cookie, Some("next-key"), &json!({})).await?.status(), 202);
            assert!(matches!(runtime.action().await?, tqa_types::ClientFrame::RunCancel { .. }));
            responses.push(accepted);
        }
        assert_ne!(responses[0], responses[1]);
        Ok(())
    }).await
}
