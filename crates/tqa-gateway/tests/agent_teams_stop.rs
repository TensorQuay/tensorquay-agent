//! Stopping an agent team, slice 4 (ADR-0034 point 10): the one stop path, whatever started it.
//! - GW-I-108: the lead's `done`, the budget, the deadline and the idle stop each end the team: its helpers stopped,
//!   their queued wakes and first turns not yet started dropped, the gateway's end note stored to the lead once (type `ended`, its reason and whole
//!   text), and the lead's next run offered `read_reports` alone to read it; a restart part way through a stop finishes
//!   it on the next pass with no second note. (A person's Stop and the deleted or disabled lead arrive with slice 6.)
//! - GW-I-211: a helper whose first turn is stored but hasn't run yet keeps the idle stop away.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, Event, ServerFrame};
use uuid::Uuid;

/// The team's state and stop reason.
async fn state(f: &Fixture, team: Uuid) -> Result<(String, Option<String>)> {
    Ok(
        sqlx::query_as("SELECT state, stop_reason FROM agent_teams WHERE id = $1")
            .bind(team)
            .fetch_one(&f.pool)
            .await?,
    )
}

/// The team's end notes: each one's addressee and body.
async fn end_notes(f: &Fixture, team: Uuid) -> Result<Vec<(String, Value)>> {
    Ok(sqlx::query_as(
        "SELECT m.to_kind, m.body FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
         WHERE t.id = $1 AND m.type = 'ended' ORDER BY m.seq",
    )
    .bind(team)
    .fetch_all(&f.pool)
    .await?)
}

/// Ends `conversation`'s run `run` at `sequence` and moves its whole journal `minutes` into the past.
async fn ended_long_ago(
    f: &Fixture,
    conversation: &ConversationId,
    run: &str,
    minutes: i32,
) -> Result<()> {
    f.gateway
        .events()
        .append(
            conversation,
            ServerFrame::Event {
                seq: 2.into(),
                run_id: Some(run.parse()?),
                step: None,
                ts: "2026-01-01T00:00:04Z".into(),
                event: Event::RunCompleted {
                    run_id: run.parse()?,
                },
            },
        )
        .await?;
    sqlx::query("UPDATE events SET created_at = created_at - make_interval(mins => $2) WHERE conversation_id = $1")
        .bind(Uuid::parse_str(conversation.as_str())?)
        .bind(minutes)
        .execute(&f.pool)
        .await?;
    Ok(())
}

async fn role(f: &Fixture, token: &str) -> Result<Value> {
    Ok(f.client
        .get(f.url("/runtime/teams/role"))
        .bearer_auth(token)
        .send()
        .await?
        .json()
        .await?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_each_stop_reason_ends_the_team_with_one_end_note_to_the_lead(
    pool: PgPool,
) -> TestResult {
    // Four teams of four connected tasks each: room for all sixteen.
    let config =
        format!("{ENABLED}[sandbox]\nmax_live = 16\n[admission]\nslots = 16\nper_user = 16\n");
    support::run_configured(pool, None, &config, async |f| {
        // Four teams, one stop reason each.
        let (owner, cookie) = f.admin().await?;
        let by_done = team_for(f, owner.id, cookie.clone()).await?;
        let by_budget = team_for(f, owner.id, cookie.clone()).await?;
        let by_deadline = team_for(f, owner.id, cookie.clone()).await?;
        let by_idle = team_for(f, owner.id, cookie).await?;
        // A wake queued for one helper, as a post leaves one for a reader without a sandbox.
        queue(f, by_budget.id, &by_budget.reviewer.0).await?;
        sqlx::query("UPDATE idempotency SET key = 'team-wake:x:0' WHERE conversation_id = $1")
            .bind(Uuid::parse_str(by_budget.reviewer.0.as_str())?)
            .execute(&f.pool)
            .await?;
        // A helper still waiting for its first turn (behind a slot, the lead's lease or its copy): queued under its
        // start key. The stop must drop it, or admission would start the helper after the team ended.
        queue(f, by_deadline.id, &by_deadline.researcher.0).await?;
        let start_key = format!("team-start:{}", by_deadline.researcher.0.as_str());
        sqlx::query("UPDATE idempotency SET key = $1 WHERE conversation_id = $2")
            .bind(&start_key)
            .bind(Uuid::parse_str(by_deadline.researcher.0.as_str())?)
            .execute(&f.pool)
            .await?;
        let (status, _) = done(f, &by_done.lead.1, "r-1:c-1", &json!({"summary": "Q3 table done."})).await?;
        assert_eq!(status, 200);
        sqlx::query("UPDATE agent_teams SET state = 'stopping', stop_reason = 'budget' WHERE id = $1")
            .bind(by_budget.id)
            .execute(&f.pool)
            .await?;
        sqlx::query("UPDATE agent_teams SET deadline_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' WHERE id = $1")
            .bind(by_deadline.id)
            .execute(&f.pool)
            .await?;
        // Nothing for 31 minutes (past the default 30) and no run active: the idle stop.
        for (conversation, run) in [(&by_idle.lead.0, "r-1"), (&by_idle.researcher.0, "r-2"), (&by_idle.reviewer.0, "r-3")] {
            ended_long_ago(f, conversation, run, 31).await?;
        }
        sqlx::query("UPDATE agent_teams SET created_at = created_at - INTERVAL '31 minutes' WHERE id = $1")
            .bind(by_idle.id)
            .execute(&f.pool)
            .await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        for (t, reason) in [(&by_done, "done"), (&by_budget, "budget"), (&by_deadline, "deadline"), (&by_idle, "idle")] {
            assert_eq!(state(f, t.id).await?, ("ended".to_owned(), Some(reason.to_owned())), "{reason}");
            let notes = end_notes(f, t.id).await?;
            assert_eq!(notes.len(), 1, "{reason}: one end note");
            let (to, body) = &notes[0];
            assert_eq!(to, "lead", "{reason}");
            assert_eq!(body["reason"], reason);
            assert!(body["text"].as_str().is_some_and(|text| !text.is_empty()), "{reason}: {body}");
            let working: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM agent_team_members WHERE team_id = $1 AND state NOT IN ('stopped', 'done', 'failed')",
            )
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
            assert_eq!(working, 0, "{reason}: every helper stopped");
        }
        // The lead's summary is in the done team's note.
        let (_, body) = &end_notes(f, by_done.id).await?[0];
        assert_eq!(body["summary"], "Q3 table done.");
        // The queued wake was dropped.
        let wake: String = sqlx::query_scalar("SELECT execution_status FROM idempotency WHERE key = 'team-wake:x:0'")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(wake, "cancelled");
        let first_turn: String = sqlx::query_scalar("SELECT execution_status FROM idempotency WHERE key = $1")
            .bind(&start_key)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(first_turn, "cancelled", "a helper stopped before its first turn never gets it");
        // The lead's next run is offered the read that delivers its note.
        assert_eq!(role(f, &by_done.lead.1).await?, json!({"role": "ended"}));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_a_stop_cut_short_by_a_restart_finishes_on_the_next_pass(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, ENABLED, async |f| {
        let id = {
            let t = team(f).await?;
            // As if the gateway stopped part way, on purpose: the team stopping, its helpers already stopped, no note
            // yet.
            sqlx::query(
                "UPDATE agent_teams SET state = 'stopping', stop_reason = 'budget' WHERE id = $1",
            )
            .bind(t.id)
            .execute(&f.pool)
            .await?;
            sqlx::query("UPDATE agent_team_members SET state = 'stopped' WHERE team_id = $1")
                .bind(t.id)
                .execute(&f.pool)
                .await?;
            t.id
        };
        f.restart().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(
            state(f, id).await?,
            ("ended".to_owned(), Some("budget".to_owned()))
        );
        assert_eq!(
            end_notes(f, id).await?.len(),
            1,
            "one note, however many passes"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_211_a_helper_that_hasnt_run_yet_keeps_the_idle_stop_away(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        for (conversation, run) in [(&t.lead.0, "r-1"), (&t.researcher.0, "r-2")] {
            ended_long_ago(f, conversation, run, 31).await?;
        }
        // The reviewer's first turn waits for a slot: it has never run.
        sqlx::query("DELETE FROM events WHERE conversation_id = $1")
            .bind(Uuid::parse_str(t.reviewer.0.as_str())?)
            .execute(&f.pool)
            .await?;
        sqlx::query(
            "UPDATE agent_teams SET created_at = created_at - INTERVAL '31 minutes' WHERE id = $1",
        )
        .bind(t.id)
        .execute(&f.pool)
        .await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(state(f, t.id).await?, ("running".to_owned(), None));
        // Once it has run and ended long ago too, the team is idle and stops.
        f.gateway
            .events()
            .append(&t.reviewer.0, support::started(1, "r-3")?)
            .await?;
        ended_long_ago(f, &t.reviewer.0, "r-3", 31).await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(
            state(f, t.id).await?,
            ("ended".to_owned(), Some("idle".to_owned()))
        );
        Ok(())
    })
    .await
}
