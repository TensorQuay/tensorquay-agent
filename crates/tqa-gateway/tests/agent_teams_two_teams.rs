//! GW-I-256 (the Product Owner's edge cases, 7 Oct 2026): two agent teams of one person at once, at that person's
//! admission cap. The first team holds all the person's slots; the second team's lead has a turn queued. An idle helper
//! of the first yields (its waiting read says `yield_slot`), its turn ends, and admission gives its slot to the other
//! team's lead: the person's own cap never starves the person's second team.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use support::TestResult;

// TEST: GW-I-256
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_256_a_persons_second_team_gets_a_slot_from_the_firsts_idle_helper(
    pool: PgPool,
) -> TestResult {
    let config =
        format!("{ENABLED}[sandbox]\nmax_live = 8\n[admission]\nslots = 8\nper_user = 4\n");
    support::run_configured(pool, None, &config, async |f| {
        // The first team: its lead, two helpers and one more task of the person's, four sandboxes, the person's cap.
        let t = team(f).await?;
        let owner: uuid::Uuid =
            sqlx::query_scalar("SELECT owner_id FROM agent_teams WHERE id = $1")
                .bind(t.id)
                .fetch_one(&f.pool)
                .await?;
        // The second team's lead, a task of the same person, has a turn queued: no slot of the person's is free.
        let second = f.conversation(owner).await?;
        let response = f
            .action(
                &format!("/api/v1/conversations/{}/messages", second.id),
                &t.cookie,
                Some("second-lead"),
                &json!({"text": "Start your own team."}),
            )
            .await?;
        assert_eq!(response.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        assert!(
            f.backend.created(&second.id).is_none(),
            "the person's cap holds it"
        );
        // The first team's reviewer waits for news: it is told to yield its slot, and its turn ends.
        let (status, answer) = read_waiting(f, &t.reviewer.1, ("r-3:c-1", "r-3"), 20).await?;
        assert_eq!(status, 200, "{answer}");
        assert_eq!(answer["hint"], json!({"kind": "yield_slot"}));
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        // Admission gives the idle helper's slot to the other team's lead, within a few passes.
        let started = std::time::Instant::now();
        while f.backend.created(&second.id).is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the person's second team still waits for a slot"
            );
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        // The first team's running members are left alone.
        let live = f.backend.list_conversations().await?;
        for running in [&t.lead.0, &t.researcher.0] {
            assert!(live.contains(running), "a running member was evicted");
        }
        Ok(())
    })
    .await
}
