//! Agent teams in rounds, step 2 (ADR-0034's rounds amendment, A1 and A4):
//! - GW-I-330: a person's follow-up to a lead whose team ended for a round's reason opens round 2: the team row runs
//!   again with a fresh budget and time from the administrator's current limits, `round_started` comes before the
//!   helper's `assign`, and the rounds table keeps round 1's record.
//! - GW-I-331: only a person's follow-up opens a round, once per run: the run that was leading when its round ended,
//!   a team wake's run, and a second round in one run are refused `round_needs_person`; within the round a run opened,
//!   a second helper joins that round.
//! - GW-I-332: each round's end note carries its round and `tokens_all_rounds`; an ended round is charged nothing.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
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

/// The team row's round, state, budget, spent tokens and seconds to its deadline.
async fn row(f: &Fixture, team: Uuid) -> Result<(i32, String, i64, i64, f64)> {
    Ok(sqlx::query_as(
        "SELECT round, state, token_budget, spent_tokens, \
         EXTRACT(EPOCH FROM deadline_at - CURRENT_TIMESTAMP)::float8 FROM agent_teams WHERE id = $1",
    )
    .bind(team)
    .fetch_one(&f.pool)
    .await?)
}

/// Sets an administrator's limit by key, at its current version.
async fn limit(f: &Fixture, t: &Team, key: &str, value: u64) -> Result<()> {
    let (_, limits) = call(
        f,
        Method::GET,
        "/api/v1/admin/limits",
        &t.cookie,
        (None, None),
    )
    .await?;
    let version = limits["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["key"] == key)
        .ok_or("no such limit")?["version"]
        .clone();
    let (status, body) = call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/limits/{key}"),
        &t.cookie,
        (
            Some(json!({"value": value, "version": version})),
            Some(&format!("limit-{key}-{value}")),
        ),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(())
}

// TEST: GW-I-330
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_330_a_persons_follow_up_opens_round_two(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        sqlx::query("UPDATE agent_teams SET spent_tokens = 1234 WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        end_round(f, t.id, "done", 5).await?;
        // The administrator's limits changed since round 1: round 2 takes them.
        limit(f, &t, "agent_teams.token_budget", 77_000).await?;
        limit(f, &t, "agent_teams.deadline_minutes", 45).await?;
        turn(f, &t, 2, "r-5", true).await?;
        let (status, body) = start(f, &t, "r-5:c-1", "writer").await?;
        assert_eq!(status, 201, "{body}");
        let (round, state, budget, spent, left) = row(f, t.id).await?;
        assert_eq!((round, state.as_str(), budget, spent), (2, "running", 77_000, 0));
        assert!((44.0 * 60.0..=45.0 * 60.0).contains(&left), "a fresh deadline: {left}");
        // Round 1's record, and round 2's open one.
        let rounds: Vec<(i32, Option<String>, i64)> = sqlx::query_as(
            "SELECT round, stop_reason, spent_tokens FROM agent_team_rounds WHERE team_id = $1 ORDER BY round",
        )
        .bind(t.id)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(rounds, vec![(1, Some("done".to_owned()), 1234), (2, None, 0)]);
        // round_started, then the new helper's assign.
        let kinds: Vec<String> = sqlx::query_scalar(
            "SELECT m.type FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
             WHERE t.id = $1 AND m.type IN ('round_started', 'assign') ORDER BY m.seq",
        )
        .bind(t.id)
        .fetch_all(&f.pool)
        .await?;
        let started = kinds.iter().position(|kind| kind == "round_started").ok_or("no round_started")?;
        assert_eq!(kinds.get(started + 1).map(String::as_str), Some("assign"), "{kinds:?}");
        Ok(())
    })
    .await
}

// TEST: GW-I-331
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_331_only_a_persons_follow_up_opens_a_round_once_per_run(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let refused = |answer: &(u16, Value)| {
            answer.0 == 409 && answer.1["error"]["code"] == "round_needs_person"
        };
        // The run that was leading when its round ended (its own done): no round.
        end_round(f, t.id, "done", 0).await?;
        let answer = start(f, &t, "r-1:c-9", "writer").await?;
        assert!(refused(&answer), "{answer:?}");
        assert_eq!(row(f, t.id).await?.0, 1, "no round opened");
        assert_eq!(row(f, t.id).await?.1, "ended");
        // A team wake's run: no round.
        turn(f, &t, 2, "r-5", false).await?;
        let answer = start(f, &t, "r-5:c-1", "writer").await?;
        assert!(refused(&answer), "{answer:?}");
        // A person's follow-up: one round, and a second helper joins that same round.
        turn(f, &t, 4, "r-6", true).await?;
        let (status, body) = start(f, &t, "r-6:c-1", "writer").await?;
        assert_eq!(status, 201, "{body}");
        let (status, body) = start(f, &t, "r-6:c-2", "editor").await?;
        assert_eq!(status, 201, "{body}");
        assert_eq!(row(f, t.id).await?.0, 2);
        // That round reaches its budget mid-run: the same run opens no other.
        end_round(f, t.id, "budget", 0).await?;
        let answer = start(f, &t, "r-6:c-3", "checker").await?;
        assert!(refused(&answer), "{answer:?}");
        assert_eq!(row(f, t.id).await?.0, 2, "no third round");
        Ok(())
    })
    .await
}

// TEST: GW-I-331
// The two cases asked at review: a person's message queued while a team wake's run is still going (its run comes
// after the wake's, so it may open a round), and a person's run that started while its round was still running and
// saw it end mid-run (it may not).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_331_a_queued_persons_message_opens_a_round_and_a_run_older_than_the_end_doesnt(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // A person's run that started while round 1 ran; round 1 ends in it.
        turn(f, &t, 2, "r-5", true).await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        end_round(f, t.id, "budget", 0).await?;
        let answer = start(f, &t, "r-5:c-1", "writer").await?;
        assert!(
            answer.0 == 409 && answer.1["error"]["code"] == "round_needs_person",
            "{answer:?}"
        );
        // A team wake's run, and the person's message queued behind it: the person's own run opens round 2.
        turn(f, &t, 4, "r-6", false).await?;
        turn(f, &t, 6, "r-7", true).await?;
        let (status, body) = start(f, &t, "r-7:c-1", "writer").await?;
        assert_eq!(status, 201, "{body}");
        assert_eq!(row(f, t.id).await?.0, 2);
        Ok(())
    })
    .await
}

/// The team's end notes' bodies, oldest first.
async fn end_notes(f: &Fixture, team: Uuid) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        "SELECT m.body FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
         WHERE t.id = $1 AND m.type = 'ended' ORDER BY m.seq",
    )
    .bind(team)
    .fetch_all(&f.pool)
    .await?)
}

// TEST: GW-I-332
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_332_each_rounds_end_note_carries_its_round_and_all_rounds_tokens(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        sqlx::query("UPDATE agent_teams SET spent_tokens = 1000 WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        let (status, body) =
            done(f, &t.lead.1, "r-1:done", &json!({"summary": "Round one."})).await?;
        assert_eq!(status, 200, "{body}");
        until_ended(f, t.id).await?;
        // Ended: a relayed call charges nothing to it.
        assert_eq!(row(f, t.id).await?.3, 1000);
        turn(f, &t, 2, "r-5", true).await?;
        let (status, body) = start(f, &t, "r-5:c-1", "writer").await?;
        assert_eq!(status, 201, "{body}");
        sqlx::query("UPDATE agent_teams SET spent_tokens = 250 WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        let (status, body) =
            done(f, &t.lead.1, "r-5:done", &json!({"summary": "Round two."})).await?;
        assert_eq!(status, 200, "{body}");
        until_ended(f, t.id).await?;
        let notes = end_notes(f, t.id).await?;
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert_eq!(
            (
                notes[0]["round"].clone(),
                notes[0]["tokens_all_rounds"].clone()
            ),
            (json!(1), json!(1000))
        );
        assert_eq!(
            (
                notes[1]["round"].clone(),
                notes[1]["tokens_all_rounds"].clone()
            ),
            (json!(2), json!(1250))
        );
        assert_eq!(notes[1]["summary"], "Round two.");
        Ok(())
    })
    .await
}

/// Waits (within 5 s) for the team's stop to finish.
async fn until_ended(f: &Fixture, team: Uuid) -> Result<()> {
    for _ in 0..100 {
        let state: String = sqlx::query_scalar("SELECT state FROM agent_teams WHERE id = $1")
            .bind(team)
            .fetch_one(&f.pool)
            .await?;
        if state == "ended" {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err("the team didn't end".into())
}
