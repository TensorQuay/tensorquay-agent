//! GW-I-255 (the Product Owner's edge cases, 7 Oct 2026): a gateway restart while a team works. GW-I-106, GW-I-108 and
//! GW-I-109 restart it mid-start, mid-stop and across a single read; here the lead's and a helper's runs are going, a
//! report is read but its result not stored yet, and another helper's run has ended. After the restart nothing is lost
//! or doubled: the read answers the same and its cursor moves once, states and the team carry on, the next decision
//! wakes the idle helper once, and the reconciler finds nothing to say.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::json;
use sqlx::PgPool;
use support::TestResult;

// TEST: GW-I-255
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_255_a_restart_while_helpers_work_loses_and_doubles_nothing(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // The researcher reports while the lead's run goes; the lead reads it, and the gateway restarts before the
        // read's result is stored.
        let (status, reported) = send(f, &t.researcher.1, "r-2:c-1", &json!({"type": "report", "text": "Found it."})).await?;
        assert_eq!(status, 201);
        let report = reported["seq"].as_u64().ok_or("seq")?;
        let (status, first) = read(f, &t.lead.1, ("r-1:c-9", "r-1")).await?;
        assert_eq!((status, seqs(&first)), (200, vec![report]));
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        let spent: i64 = sqlx::query_scalar("SELECT spent_tokens FROM agent_teams WHERE id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        f.restart().await?;
        // The same read answers the same: the report isn't lost.
        let (status, again) = read(f, &t.lead.1, ("r-1:c-9", "r-1")).await?;
        assert_eq!((status, seqs(&again)), (200, vec![report]), "the read across the restart answers the same");
        // Its result is stored: the cursor moves once, and the next read has nothing more.
        f.gateway.events().append(&t.lead.0, result(2, "r-1", "c-9")?).await?;
        let (status, next) = read(f, &t.lead.1, ("r-1:c-10", "r-1")).await?;
        assert_eq!((status, seqs(&next)), (200, Vec::<u64>::new()), "nothing doubled");
        // The team and its helpers carry on as they were, and the budget is as it was.
        let (state, after): (String, i64) = sqlx::query_as("SELECT state, spent_tokens FROM agent_teams WHERE id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!((state.as_str(), after), ("running", spent));
        let states: Vec<(String, String)> =
            sqlx::query_as("SELECT name, state FROM agent_team_members WHERE team_id = $1 ORDER BY name")
                .bind(t.id)
                .fetch_all(&f.pool)
                .await?;
        assert_eq!(states.len(), 2);
        assert!(states.iter().all(|(_, state)| state != "stopped" && state != "failed"), "{states:?}");
        // A decision after the restart wakes the idle reviewer once; the researcher, still running, reads it itself.
        let (status, _) = send(f, &t.lead.1, "r-1:c-11", &json!({"type": "decision", "text": "Go on."})).await?;
        assert_eq!(status, 201);
        assert_eq!(wakes_settle(f, &t.reviewer.0, 1).await?.len(), 1);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(wakes(f, &t.reviewer.0).await?.len(), 1, "woken once");
        assert!(wakes(f, &t.researcher.0).await?.is_empty());
        // The reconciler's passes after the restart find nothing to tell the lead.
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        let notices: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
             WHERE t.id = $1 AND m.type = 'notice'",
        )
        .bind(t.id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(notices, 0);
        Ok(())
    })
    .await
}
