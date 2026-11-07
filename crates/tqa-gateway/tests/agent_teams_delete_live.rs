//! GW-I-253 (the owner's edge cases, 7 Oct 2026): deleting a lead while its helpers are live. GW-I-108 deletes a lead
//! whose helpers have no sandbox; here one helper's run is going and the other, idle, has a wake from the lead queued.
//! The delete leaves nothing behind: every sandbox retired, every runtime token revoked, the queued wake never
//! delivered nor a sandbox started for it, the team ended for `lead_deleted`, and all three tasks deleted.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::json;
use sqlx::PgPool;
use support::TestResult;
use tqa_sandbox::SandboxBackend;
use uuid::Uuid;

// TEST: GW-I-253
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_253_deleting_a_lead_with_live_helpers_leaves_nothing_behind(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // The researcher's run goes on (r-2); the reviewer's ends, and the lead's decision queues it a wake.
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        let (status, _) = send(f, &t.lead.1, "r-1:c-1", &json!({"type": "decision", "text": "Go"})).await?;
        assert_eq!(status, 201);
        assert_eq!(wakes_settle(f, &t.reviewer.0, 1).await?.len(), 1, "the reviewer's wake is queued");
        let ours = [&t.lead.0, &t.researcher.0, &t.reviewer.0];
        let before = f.backend.creates.load(std::sync::atomic::Ordering::SeqCst);
        let ids: Vec<Uuid> = ours
            .iter()
            .map(|id| Uuid::parse_str(id.as_str()))
            .collect::<std::result::Result<_, _>>()?;
        // The person deletes the lead with its helpers.
        let deleted = f
            .client
            .delete(f.url(&format!("/api/v1/conversations/{}?with_helpers=true", t.lead.0)))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .send()
            .await?;
        assert_eq!(deleted.status().as_u16(), 204);
        // Each runtime, its token revoked and its link closed, is refused on its redial and exits, as a real one does:
        // its container stops. The fake's runtimes don't, so the test stops them.
        for handle in f.backend.list().await? {
            if ours.iter().any(|id| **id == handle.conversation_id) {
                f.backend.lose(&handle).await?;
            }
        }
        // Every sandbox of the three is retired, and stays so: the queued wake starts none.
        let started = std::time::Instant::now();
        loop {
            let live = f.backend.list_conversations().await?;
            let left: Vec<&str> = [("lead", &t.lead.0), ("researcher", &t.researcher.0), ("reviewer", &t.reviewer.0)]
                .iter()
                .filter(|(_, id)| live.contains(id))
                .map(|(name, _)| *name)
                .collect();
            if left.is_empty() {
                break;
            }
            assert!(
                started.elapsed() < support::DEADLINE,
                "still live {:?} after the delete: {left:?}",
                started.elapsed()
            );
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        for _ in 0..5 {
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let live = f.backend.list_conversations().await?;
        assert!(!ours.iter().any(|id| live.contains(id)), "a sandbox came back after the delete");
        assert_eq!(
            f.backend.creates.load(std::sync::atomic::Ordering::SeqCst),
            before,
            "no sandbox was started for the queued wake"
        );
        // The gateway retired each lost sandbox for good: no record left, and none replaced.
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let records: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM sandbox_instances WHERE conversation_id = ANY($1)")
                        .bind(&ids)
                        .fetch_one(&f.pool)
                        .await?;
                if records == 0 {
                    return Ok::<_, Box<dyn std::error::Error>>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
        .await??;
        let tokens: i64 = sqlx::query_scalar("SELECT count(*) FROM runtime_tokens WHERE conversation_id = ANY($1)")
            .bind(&ids)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(tokens, 0, "every runtime token revoked");
        let queued: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE conversation_id = ANY($1) AND execution_status IN ('queued','sending')",
        )
        .bind(&ids)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(queued, 0, "no turn left waiting to go out");
        let states: Vec<String> = sqlx::query_scalar("SELECT state FROM conversations WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(&f.pool)
            .await?;
        assert_eq!(states, ["deleted", "deleted", "deleted"]);
        let (state, reason): (String, Option<String>) =
            sqlx::query_as("SELECT state, stop_reason FROM agent_teams WHERE id = $1")
                .bind(t.id)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!((state.as_str(), reason.as_deref()), ("ended", Some("lead_deleted")));
        Ok(())
    })
    .await
}
