//! Agent teams' liveness, slice 4 (ADR-0034 points 9-11): the reconciler's notices to the lead, from stored state.
//! - GW-I-111: a helper idle past `member_idle_minutes` without a report, one whose run made no progress past
//!   `member_stuck_minutes` (marked `stuck`, `working` again on progress), and one whose run failed (its reason in plain
//!   words), each told to the lead once per episode; the deadline's warning once, at min(10 min, 20 %) before it.
//!
//! Each test backdates the stored times instead of waiting minutes, then runs one pass of the reconciler.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::Value;
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, ErrorCode, Event, ServerFrame};
use uuid::Uuid;

/// Moves every stored event of `conversation` `minutes` into the past.
async fn backdate(f: &Fixture, conversation: &ConversationId, minutes: i32) -> Result<()> {
    sqlx::query("UPDATE events SET created_at = created_at - make_interval(mins => $2) WHERE conversation_id = $1")
        .bind(Uuid::parse_str(conversation.as_str())?)
        .bind(minutes)
        .execute(&f.pool)
        .await?;
    Ok(())
}

/// Stores `event` as `conversation`'s frame `sequence` of run `run`.
async fn event(
    f: &Fixture,
    conversation: &ConversationId,
    sequence: u64,
    run: &str,
    event: Event,
) -> Result<()> {
    f.gateway
        .events()
        .append(
            conversation,
            ServerFrame::Event {
                seq: sequence.into(),
                run_id: Some(run.parse()?),
                step: None,
                ts: "2026-01-01T00:00:03Z".into(),
                event,
            },
        )
        .await?;
    Ok(())
}

/// The notices in the team's log, as `(kind, member)`, oldest first.
async fn notices(f: &Fixture, team: Uuid) -> Result<Vec<(String, Option<String>)>> {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT m.body FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
         WHERE t.id = $1 AND m.type = 'notice' ORDER BY m.seq",
    )
    .bind(team)
    .fetch_all(&f.pool)
    .await?;
    Ok(rows
        .iter()
        .map(|body| {
            (
                body["kind"].as_str().unwrap_or_default().to_owned(),
                body["member"].as_str().map(str::to_owned),
            )
        })
        .collect())
}

async fn member_state(f: &Fixture, name: &str) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM agent_team_members WHERE name = $1")
            .bind(name)
            .fetch_one(&f.pool)
            .await?,
    )
}

fn notice(kind: &str, member: &str) -> (String, Option<String>) {
    (kind.to_owned(), Some(member.to_owned()))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_111_a_helper_idle_without_a_report_is_told_to_the_lead_once(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // The researcher's run ended 11 minutes ago (past the default 10) with no report and nothing waiting for it.
        event(
            f,
            &t.researcher.0,
            2,
            "r-2",
            Event::RunCompleted {
                run_id: "r-2".parse()?,
            },
        )
        .await?;
        backdate(f, &t.researcher.0, 11).await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(
            notices(f, t.id).await?,
            [notice("member_idle", "researcher")],
            "once per episode"
        );
        // The reviewer's run ended a minute ago: not idle yet.
        event(
            f,
            &t.reviewer.0,
            2,
            "r-3",
            Event::RunCompleted {
                run_id: "r-3".parse()?,
            },
        )
        .await?;
        backdate(f, &t.reviewer.0, 1).await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(notices(f, t.id).await?.len(), 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_111_a_helper_without_progress_is_marked_stuck_and_working_again_on_progress(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // The reviewer's run is active and stored nothing for 16 minutes (past the default 15).
        backdate(f, &t.reviewer.0, 16).await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(member_state(f, "reviewer").await?, "stuck");
        assert_eq!(
            notices(f, t.id).await?,
            [notice("member_stuck", "reviewer")],
            "once per episode"
        );
        // Progress: the run stores an event now. It is working again, and nothing more is said.
        event(
            f,
            &t.reviewer.0,
            2,
            "r-3",
            Event::TextDelta {
                text: "Back.".into(),
            },
        )
        .await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(member_state(f, "reviewer").await?, "working");
        assert_eq!(notices(f, t.id).await?.len(), 1);
        // The researcher, active and quiet for only a minute, is left alone.
        assert_eq!(member_state(f, "researcher").await?, "working");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_111_a_helpers_failed_run_is_told_with_its_reason_once(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let failed = Event::RunFailed {
            run_id: "r-2".parse()?,
            reason: "the model provider kept failing".into(),
            code: ErrorCode::ProviderError,
            provider_message: None,
        };
        event(f, &t.researcher.0, 2, "r-2", failed).await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(notices(f, t.id).await?, [notice("member_failed", "researcher")]);
        let text: String = sqlx::query_scalar(
            "SELECT m.body->>'text' FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
             WHERE t.id = $1 AND m.type = 'notice'",
        )
        .bind(t.id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(text, "researcher failed: the model provider kept failing.", "the reason in plain words, no code");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_111_the_deadline_is_announced_once_inside_its_warning_window(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // A 120-minute deadline warns at 10 minutes left: at 15 left nothing, at 5 left once.
        sqlx::query(
            "UPDATE agent_teams SET created_at = CURRENT_TIMESTAMP - INTERVAL '105 minutes', \
             deadline_at = CURRENT_TIMESTAMP + INTERVAL '15 minutes' WHERE id = $1",
        )
        .bind(t.id)
        .execute(&f.pool)
        .await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert!(notices(f, t.id).await?.is_empty());
        sqlx::query(
            "UPDATE agent_teams SET created_at = CURRENT_TIMESTAMP - INTERVAL '115 minutes', \
             deadline_at = CURRENT_TIMESTAMP + INTERVAL '5 minutes' WHERE id = $1",
        )
        .bind(t.id)
        .execute(&f.pool)
        .await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(
            notices(f, t.id).await?,
            [("deadline_near".to_owned(), None)]
        );
        Ok(())
    })
    .await
}
