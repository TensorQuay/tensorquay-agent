//! Bringing a helper back across rounds, as the real-model run found it (ADR-0034's rounds amendment, A2 and A3, amended
//! on 19 Nov 2026):
//! - GW-I-339: a helper of an earlier round named again is brought back whatever it is doing; busy, it joins the round
//!   queued, its brief behind its current run, never a new `<role>-2`.
//! - GW-I-340: a helper not in the round is never woken by the round's messages.
//! - GW-I-341: two helpers of one role in the same round still get a suffix.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{Event, ServerFrame, TeamWake};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A new run of the lead's task, `run`, after a turn at `seq`: a person's message, or the gateway's team wake.
async fn turn(f: &Fixture, t: &Team, seq: u64, run: &str, person: bool) -> Result<()> {
    let message = ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: Event::MessageUser {
            text: "Another pass, please.".into(),
            attachments: Vec::new(),
            lead_brief: None,
            team_wake: (!person).then_some(TeamWake { after: 1 }),
            channel_wake: None,
        },
    };
    f.gateway.events().append(&t.lead.0, message).await?;
    f.gateway
        .events()
        .append(&t.lead.0, support::started(seq + 1, run)?)
        .await?;
    RUNS.lock()
        .map_err(|_| "runs")?
        .insert(t.lead.1.clone(), run.to_owned());
    Ok(())
}

/// The lead's `start_member` for `role`, in the lead's current run.
async fn start(f: &Fixture, t: &Team, key: &str, role: &str) -> Result<(u16, Value)> {
    let mut request = f
        .client
        .post(f.url("/runtime/teams/members"))
        .bearer_auth(&t.lead.1)
        .header("idempotency-key", key);
    if let Some(run) = run_of(&t.lead.1) {
        request = request.header("x-tqa-run-id", run);
    }
    let request = request.json(&json!({
        "role": role, "workspace": "own_copy",
        "brief": {"objective": "Check the totals.", "output": "A table.", "boundaries": "Read only."},
    }));
    support::send_retrying(request).await
}

/// Ends the team's current round for `reason`, `ago` seconds back.
async fn end_round(f: &Fixture, team: Uuid, reason: &str, ago: i32) -> Result<()> {
    sqlx::query(
        "UPDATE agent_teams SET state = 'ended', stop_reason = $2, \
         ended_at = CURRENT_TIMESTAMP - make_interval(secs => $3) WHERE id = $1",
    )
    .bind(team)
    .bind(reason)
    .bind(f64::from(ago))
    .execute(&f.pool)
    .await?;
    Ok(())
}

/// The lead's `start_member` with a raw body (a bring-back's `files`, a changed `workspace`), in its current run.
async fn start_with(f: &Fixture, t: &Team, key: &str, body: Value) -> Result<(u16, Value)> {
    let mut request = f
        .client
        .post(f.url("/runtime/teams/members"))
        .bearer_auth(&t.lead.1)
        .header("idempotency-key", key);
    if let Some(run) = run_of(&t.lead.1) {
        request = request.header("x-tqa-run-id", run);
    }
    support::send_retrying(request.json(&body)).await
}

fn brief() -> Value {
    json!({"objective": "Check round two's totals.", "output": "A table.", "boundaries": "Read only."})
}

/// Round 1 ends at `done` with `researcher` done, and a person's follow-up starts the lead's next run.
async fn between_rounds(f: &Fixture, t: &Team) -> Result<()> {
    sqlx::query(
        "UPDATE agent_team_members SET state = 'done' WHERE team_id = $1 AND name = 'researcher'",
    )
    .bind(t.id)
    .execute(&f.pool)
    .await?;
    end_round(f, t.id, "done", 5).await?;
    turn(f, t, 2, "r-5", true).await
}

/// A helper's row: its state and round.
async fn helper(f: &Fixture, t: &Team, name: &str) -> Result<(String, i32)> {
    Ok(sqlx::query_as(
        "SELECT state, round FROM agent_team_members WHERE team_id = $1 AND name = $2",
    )
    .bind(t.id)
    .bind(name)
    .fetch_one(&f.pool)
    .await?)
}

/// The helpers' names in the team, sorted.
async fn names(f: &Fixture, t: &Team) -> Result<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT name FROM agent_team_members WHERE team_id = $1 ORDER BY name")
            .bind(t.id)
            .fetch_all(&f.pool)
            .await?,
    )
}

// TEST: GW-I-339
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_339_naming_a_busy_helper_of_an_earlier_round_brings_it_back_queued(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        between_rounds(f, &t).await?;
        // Between rounds the person wrote to researcher, and it is still at work on that message (its run is open).
        sqlx::query("UPDATE agent_team_members SET state = 'working' WHERE team_id = $1 AND name = 'researcher'")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "researcher", "workspace": "own_copy", "brief": brief(), "files": "keep"}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        // The same helper, brought back into round 2: never a new "researcher-2".
        assert_eq!(
            (&body["brought_back"], &body["member"]["name"], &body["round"]),
            (&json!(true), &json!("researcher"), &json!(2)),
            "{body}"
        );
        assert_eq!(names(f, &t).await?, vec!["researcher", "reviewer"]);
        // Its round's brief waits behind the run it is in, as a person's message would: it shows as queued.
        let (state, round) = helper(f, &t, "researcher").await?;
        assert_eq!((state.as_str(), round), ("queued", 2));
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE conversation_id = $1 AND key LIKE 'team-round:%:2' \
             AND execution_status = 'queued'",
        )
        .bind(Uuid::parse_str(t.researcher.0.as_str())?)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(waiting, 1, "its round's turn is queued behind its current run");
        Ok(())
    })
    .await
}

// TEST: GW-I-340
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_340_a_helper_not_in_the_round_is_never_woken_by_its_messages(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // Round 1 ends with both helpers done and idle (their runs over).
        sqlx::query("UPDATE agent_team_members SET state = 'done' WHERE team_id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        between_rounds(f, &t).await?;
        // Round 2 brings back researcher only, and the lead writes to its members.
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "researcher", "workspace": "own_copy", "brief": brief(), "files": "keep"}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        let (status, body) = send(f, &t.lead.1, "r-5:c-2", &json!({"type": "question", "text": "Any blockers?"})).await?;
        assert_eq!(status, 201, "{body}");
        // reviewer isn't in round 2: nothing the round says wakes it.
        assert_eq!(wakes_settle(f, &t.reviewer.0, 1).await?, Vec::<String>::new());
        let (state, round) = helper(f, &t, "reviewer").await?;
        assert_eq!((state.as_str(), round), ("done", 1));
        Ok(())
    })
    .await
}

// TEST: GW-I-341
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_341_two_helpers_of_one_role_in_the_same_round_get_a_suffix(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // researcher is at work in round 1: naming its role again starts a second one beside it.
        let (status, body) = start(f, &t, "r-1:c-1", "researcher").await?;
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["member"]["name"], json!("researcher-2"), "{body}");
        assert_eq!(body.get("brought_back"), None, "{body}");
        assert_eq!(
            names(f, &t).await?,
            vec!["researcher", "researcher-2", "reviewer"]
        );
        Ok(())
    })
    .await
}
