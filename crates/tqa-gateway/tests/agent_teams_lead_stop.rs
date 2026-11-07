//! GW-I-250 (the owner's edge cases, 7 Oct 2026): the composer's Stop on the lead stops only the lead's own run
//! (ADR-0034, Stop: "The composer's Stop still stops only that task's own run"). The team keeps running, its helpers
//! keep working and are never cancelled, and what they report while the lead was stopped isn't lost: the lead reads it
//! at its next run, which the gateway's wake starts once the stopped run has ended.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::json;
use sqlx::PgPool;
use support::TestResult;
use uuid::Uuid;

// TEST: GW-I-250
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_250_stop_on_the_leads_run_leaves_the_team_and_its_helpers_working(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let lead = Uuid::parse_str(t.lead.0.as_str())?;
        // A helper reports while the lead's run goes: an active run isn't woken, it reads with its own tools.
        let (status, _) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "Found the three sources."}),
        )
        .await?;
        assert_eq!(status, 201);
        // The person presses the composer's Stop on the lead: its run, not the team.
        let response = f
            .action(
                &format!("/api/v1/conversations/{}/runs/r-1/cancel", t.lead.0),
                &t.cookie,
                Some("stop-lead-run"),
                &json!({}),
            )
            .await?;
        assert_eq!(response.status(), 202, "{}", response.text().await?);
        // The lead's runtime ends the run as cancelled.
        f.gateway
            .events()
            .append(
                &t.lead.0,
                tqa_types::ServerFrame::Event {
                    seq: 2.into(),
                    run_id: Some("r-1".parse()?),
                    step: None,
                    ts: "2026-01-01T00:00:01Z".into(),
                    event: tqa_types::Event::RunCancelled {
                        run_id: "r-1".parse()?,
                    },
                },
            )
            .await?;
        // The team runs on, and neither helper was stopped.
        let state: String = sqlx::query_scalar("SELECT state FROM agent_teams WHERE id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(state, "running");
        let members: Vec<(String, String)> = sqlx::query_as(
            "SELECT name, state FROM agent_team_members WHERE team_id = $1 ORDER BY name",
        )
        .bind(t.id)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(
            members,
            [
                ("researcher".to_owned(), "working".to_owned()),
                ("reviewer".to_owned(), "working".to_owned())
            ]
        );
        // The report the lead hadn't read starts its next run: one wake, after the stopped run's end.
        let started = std::time::Instant::now();
        while wakes(f, &t.lead.0).await?.is_empty()
            && started.elapsed() < std::time::Duration::from_secs(20)
        {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        // The wake comes from the reconciler's next pass (every 15 s), not at the run's end itself.
        let woken = wakes(f, &t.lead.0).await?;
        assert_eq!(
            woken.len(),
            1,
            "the lead is woken once for what it hadn't read"
        );
        assert!(
            woken[0].starts_with(&format!("team-wake:{lead}:")),
            "{woken:?}"
        );
        // A helper's later report needs nothing more: the same wake covers it until the lead reads.
        let (status, _) = send(
            f,
            &t.reviewer.1,
            "r-3:c-1",
            &json!({"type": "report", "text": "Reviewed."}),
        )
        .await?;
        assert_eq!(status, 201);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(wakes(f, &t.lead.0).await?.len(), 1);
        Ok(())
    })
    .await
}
