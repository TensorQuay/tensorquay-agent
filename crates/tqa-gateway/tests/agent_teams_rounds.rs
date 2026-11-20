//! Agent teams in rounds, step 2 (ADR-0034's rounds amendment, A1 and A4):
//! - GW-I-330: a person's follow-up to a lead whose team ended for a round's reason opens round 2: the team row runs
//!   again with a fresh budget and time from the administrator's current limits, `round_started` comes before the
//!   helper's `assign`, and the rounds table keeps round 1's record.
//! - GW-I-331: only a person's follow-up opens a round, once per run: the run that was leading when its round ended,
//!   a team wake's run, and a second round in one run are refused `round_needs_person`; within the round a run opened,
//!   a second helper joins that round.
//! - GW-I-332: each round's end note carries its round and `tokens_all_rounds`; an ended round is charged nothing.
//! - GW-I-333 (step 3, A3): `start_member` naming a finished helper, in any letter case, brings it back: the same task
//!   and row, `starting` in the new round, its round's turn sent once, and the answer says so.
//! - GW-I-334 (step 3): a bring-back's refusals: a changed workspace or profile, `fresh_copy` for a `my_files` helper,
//!   `workspace_gone` when its copy no longer exists.
//! - SEC-067 (step 3): a brought-back helper gets no more than its lead's current settings, narrowed between rounds.
//! - GW-I-337 (step 4, A5): what the person reads: `GET …/team` says the round, every round's record (newest last) and
//!   all rounds' tokens, and each helper's round; `Conversation.team` says the round; the team's version rises when a
//!   round opens.
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

// TEST: GW-I-331
// Once per run on its own: a person's run opens round 2, and round 2's end is (as written straight to the store)
// earlier than that run's start, so only the run's opened round, recorded with it, refuses a third (the negative
// control of 18 Nov 2026 found no other case held this rule alone).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_331_a_run_that_opened_a_round_opens_no_other(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        end_round(f, t.id, "done", 60).await?;
        turn(f, &t, 2, "r-5", true).await?;
        let (status, body) = start(f, &t, "r-5:c-1", "writer").await?;
        assert_eq!(status, 201, "{body}");
        end_round(f, t.id, "done", 30).await?;
        let answer = start(f, &t, "r-5:c-2", "editor").await?;
        assert!(
            answer.0 == 409 && answer.1["error"]["code"] == "round_needs_person",
            "{answer:?}"
        );
        assert_eq!(row(f, t.id).await?.0, 2, "no third round");
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

// TEST: GW-I-333
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_333_naming_a_finished_helper_brings_it_back(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        between_rounds(f, &t).await?;
        let members_before: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_team_members WHERE team_id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        // Any letter case; the default own_copy passes for an own_copy helper.
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "Researcher", "workspace": "own_copy", "brief": brief(), "files": "keep"}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        assert_eq!(
            (&body["brought_back"], &body["files"], &body["round"], &body["member"]["name"]),
            (&json!(true), &json!("keep"), &json!(2), &json!("researcher"))
        );
        assert_eq!(body["member"]["conversation"], json!(t.researcher.0.as_str()), "the same task");
        let members_after: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_team_members WHERE team_id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(members_after, members_before, "the same row, no new helper");
        // Starting in round 2, as a new helper's answer says; by now its round's turn is sent (queued or working).
        assert_eq!(body["member"]["state"], json!("starting"));
        let (state, round) = helper(f, &t, "researcher").await?;
        assert!(["starting", "queued", "working"].contains(&state.as_str()), "{state}");
        assert_eq!(round, 2);
        // Its round's turn, once, keyed per helper and round.
        let turns: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE conversation_id = $1 AND key LIKE 'team-round:%:2'",
        )
        .bind(Uuid::parse_str(t.researcher.0.as_str())?)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(turns, 1);
        // A new role word is a new helper in the same round.
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-2",
            json!({"role": "fact checker", "workspace": "own_copy", "brief": brief()}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        assert_eq!(body.get("brought_back"), None, "{body}");
        Ok(())
    })
    .await
}

// TEST: GW-I-334
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_334_a_bring_backs_refusals(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        between_rounds(f, &t).await?;
        let invalid = |answer: &(u16, Value)| {
            answer.0 == 400 && answer.1["error"]["details"][0]["reason"] == "parameter_invalid"
        };
        // A changed workspace: the helper's own, named in the refusal.
        let answer = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "researcher", "workspace": "my_files", "brief": brief()}),
        )
        .await?;
        assert!(invalid(&answer), "{answer:?}");
        assert!(answer.1.to_string().contains("own copy"), "{answer:?}");
        // Its copy gone: keep can't, and fresh_copy is named as the way.
        let workspace: Uuid = sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id = $1")
            .bind(Uuid::parse_str(t.researcher.0.as_str())?)
            .fetch_one(&f.pool)
            .await?;
        sqlx::query("UPDATE workspaces SET state = 'deleted' WHERE id = $1")
            .bind(workspace)
            .execute(&f.pool)
            .await?;
        let answer = start_with(
            f,
            &t,
            "r-5:c-2",
            json!({"role": "researcher", "workspace": "own_copy", "brief": brief(), "files": "keep"}),
        )
        .await?;
        assert_eq!((answer.0, &answer.1["error"]["code"]), (409, &json!("workspace_gone")), "{answer:?}");
        assert!(answer.1.to_string().contains("fresh_copy"), "{answer:?}");
        // A my_files helper takes keep only.
        sqlx::query("UPDATE agent_team_members SET workspace = 'my_files', state = 'done' WHERE team_id = $1 AND name = 'reviewer'")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        let answer = start_with(
            f,
            &t,
            "r-5:c-3",
            json!({"role": "reviewer", "workspace": "my_files", "brief": brief(), "files": "fresh_copy"}),
        )
        .await?;
        assert!(invalid(&answer), "{answer:?}");
        assert_eq!(helper(f, &t, "reviewer").await?.0, "done", "nothing changed");
        Ok(())
    })
    .await
}

// TEST: SEC-067
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_067_a_brought_back_helper_gets_no_more_than_its_leads_current_settings(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let (lead_task, helper_task) = (Uuid::parse_str(t.lead.0.as_str())?, Uuid::parse_str(t.researcher.0.as_str())?);
        // Round 1: the helper had what its lead had then.
        sqlx::query("UPDATE conversations SET mcp_servers = ARRAY['docs', 'wiki-mcp'], connectors = ARRAY['tracker', 'wiki'] WHERE id = ANY($1)")
            .bind(vec![lead_task, helper_task])
            .execute(&f.pool)
            .await?;
        between_rounds(f, &t).await?;
        // Between rounds the lead's settings narrowed.
        sqlx::query("UPDATE conversations SET mcp_servers = ARRAY['docs'], connectors = ARRAY['tracker'] WHERE id = $1")
            .bind(lead_task)
            .execute(&f.pool)
            .await?;
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "researcher", "workspace": "own_copy", "brief": brief()}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        let (servers, connectors): (Vec<String>, Vec<String>) =
            sqlx::query_as("SELECT mcp_servers, connectors FROM conversations WHERE id = $1")
                .bind(helper_task)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(
            (servers, connectors),
            (vec!["docs".to_owned()], vec!["tracker".to_owned()]),
            "no more than the lead has now"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-335
// The first-turn sweep knows a round's turn (A3): a helper brought back whose turn didn't go out gets it from the
// sweep, under its round's key, timed from the round's start, never failed for a start long ago in round 1.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_335_the_sweep_sends_a_brought_back_helpers_round_turn(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        between_rounds(f, &t).await?;
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "researcher", "workspace": "own_copy", "brief": brief()}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        let helper_task = Uuid::parse_str(t.researcher.0.as_str())?;
        // As if the send after the start had failed, for a helper first started long ago.
        let mut transaction = f.pool.begin().await?;
        sqlx::query("DELETE FROM idempotency WHERE conversation_id = $1 AND key LIKE 'team-round:%'")
            .bind(helper_task)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE agent_team_members SET state = 'starting', created_at = CURRENT_TIMESTAMP - INTERVAL '2 hours' \
             WHERE conversation_id = $1",
        )
        .bind(helper_task)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        let mut sent = 0;
        for _ in 0..200 {
            sent = sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM idempotency WHERE conversation_id = $1 AND key LIKE 'team-round:%:2'",
            )
            .bind(helper_task)
            .fetch_one(&f.pool)
            .await?;
            if sent == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(sent, 1, "the sweep sent its round turn");
        assert_ne!(helper(f, &t, "researcher").await?.0, "failed", "not failed for its old start");
        Ok(())
    })
    .await
}

// TEST: GW-I-336
// `files: fresh_copy` (A3): the helper brought back works in a new fork of its lead's files now, and its old copy is
// deleted once the new one is ready.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_336_fresh_copy_gives_a_brought_back_helper_a_new_fork(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        between_rounds(f, &t).await?;
        let workspace_of = async |task: &str| -> Result<Uuid> {
            Ok(sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id = $1")
                .bind(Uuid::parse_str(task)?)
                .fetch_one(&f.pool)
                .await?)
        };
        let (lead_files, old_copy) = (workspace_of(t.lead.0.as_str()).await?, workspace_of(t.researcher.0.as_str()).await?);
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "researcher", "workspace": "own_copy", "brief": brief(), "files": "fresh_copy"}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["files"], json!("fresh_copy"));
        let new_copy = workspace_of(t.researcher.0.as_str()).await?;
        assert_ne!(new_copy, old_copy, "a new copy");
        let forked_from: Option<Uuid> = sqlx::query_scalar("SELECT forked_from FROM workspaces WHERE id = $1")
            .bind(new_copy)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(forked_from, Some(lead_files), "of the lead's files now");
        let mut old_state = String::new();
        for _ in 0..200 {
            old_state = sqlx::query_scalar("SELECT state FROM workspaces WHERE id = $1")
                .bind(old_copy)
                .fetch_one(&f.pool)
                .await?;
            if old_state == "deleted" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(old_state, "deleted", "the old copy goes once the new one is ready");
        Ok(())
    })
    .await
}

// TEST: SEC-067
// The lead's profile narrowed between rounds (a person switched the lead task to fewer tools): the helper brought back
// gets no more than its lead's profile now, whether it carried a copy of its lead's profile or one of its own
// (reviewed on 18 Nov 2026: the narrower profile is taken, as a new helper's is narrowed, never a refusal).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_067_a_lead_profile_narrowed_between_rounds_narrows_the_helper_brought_back(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let lead_task = Uuid::parse_str(t.lead.0.as_str())?;
        let (copied, own) = (
            Uuid::parse_str(t.researcher.0.as_str())?,
            Uuid::parse_str(t.reviewer.0.as_str())?,
        );
        let profile = |tools: &[&str]| json!({"name": "analyst", "tools": tools});
        let set = async |task: Uuid, id: Uuid, value: Value| -> Result<()> {
            sqlx::query("UPDATE conversations SET profile_id = $2, profile_version = 1, profile = $3 WHERE id = $1")
                .bind(task)
                .bind(id)
                .bind(value)
                .execute(&f.pool)
                .await?;
            Ok(())
        };
        // Round 1: the lead and the researcher (a copy of its lead's) allow shell, read_file and grep; the reviewer
        // has a profile of its own allowing the same.
        let (lead_profile, own_profile) = (Uuid::now_v7(), Uuid::now_v7());
        set(lead_task, lead_profile, profile(&["shell", "read_file", "grep"])).await?;
        set(copied, lead_profile, profile(&["shell", "read_file", "grep"])).await?;
        set(own, own_profile, json!({"name": "reviewer", "tools": ["shell", "read_file", "grep"]})).await?;
        sqlx::query("UPDATE agent_team_members SET state = 'done' WHERE team_id = $1 AND name = 'reviewer'")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        between_rounds(f, &t).await?;
        // Between rounds a person narrowed the lead task's profile to read_file alone.
        sqlx::query("UPDATE conversations SET profile = $2, profile_version = 2 WHERE id = $1")
            .bind(lead_task)
            .bind(profile(&["read_file"]))
            .execute(&f.pool)
            .await?;
        for (key, role, task) in [("r-5:c-1", "researcher", copied), ("r-5:c-2", "reviewer", own)] {
            let (status, body) = start_with(
                f,
                &t,
                key,
                json!({"role": role, "workspace": "own_copy", "brief": brief()}),
            )
            .await?;
            assert_eq!(status, 201, "{role}: {body}");
            let tools: Value = sqlx::query_scalar("SELECT profile->'tools' FROM conversations WHERE id = $1")
                .bind(task)
                .fetch_one(&f.pool)
                .await?;
            assert_eq!(tools, json!(["read_file"]), "{role}: no more than its lead's profile now");
        }
        Ok(())
    })
    .await
}

/// The person's team answer for the lead, and the lead's own `Conversation.team`.
async fn people_read(f: &Fixture, t: &Team) -> Result<(Value, Value)> {
    let team = f
        .get(
            &format!("/api/v1/conversations/{}/team", t.lead.0),
            &t.cookie,
        )
        .await?;
    assert_eq!(team.status(), 200);
    let lead = f
        .get(&format!("/api/v1/conversations/{}", t.lead.0), &t.cookie)
        .await?;
    assert_eq!(lead.status(), 200);
    Ok((
        team.json::<Value>().await?,
        lead.json::<Value>().await?["team"].clone(),
    ))
}

// TEST: GW-I-337
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_337_the_person_reads_the_round_every_rounds_record_and_all_rounds_tokens(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let (one, lead_one) = people_read(f, &t).await?;
        assert_eq!(one["round"], json!(1), "{one}");
        assert_eq!(lead_one["round"], json!(1), "{lead_one}");
        let rounds = one["rounds"].as_array().ok_or("rounds")?;
        assert_eq!(rounds.len(), 1, "{one}");
        assert_eq!(
            (&rounds[0]["round"], &rounds[0]["ended_at"]),
            (&json!(1), &Value::Null)
        );
        for helper in one["helpers"].as_array().ok_or("helpers")? {
            assert_eq!(helper["round"], json!(1), "{helper}");
        }
        let before = one["version"].as_u64().ok_or("version")?;
        // Round 1 spends 1,200 tokens, ends at `done`, and a person's follow-up brings the researcher back.
        sqlx::query("UPDATE agent_teams SET spent_tokens = 1200 WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        between_rounds(f, &t).await?;
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "researcher", "workspace": "own_copy", "brief": brief()}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        // The version rises once the change is flushed (at most a second).
        let mut two = Value::Null;
        let mut lead_two = Value::Null;
        for _ in 0..50 {
            (two, lead_two) = people_read(f, &t).await?;
            if two["version"]
                .as_u64()
                .is_some_and(|version| version > before)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(two["version"].as_u64().ok_or("version")? > before, "{two}");
        assert_eq!(two["round"], json!(2), "{two}");
        assert_eq!(lead_two["round"], json!(2), "{lead_two}");
        let rounds = two["rounds"].as_array().ok_or("rounds")?;
        assert_eq!(rounds.len(), 2, "{two}");
        assert_eq!(rounds[0]["round"], json!(1));
        assert_eq!(rounds[0]["stop_reason"], json!("done"));
        assert!(rounds[0]["ended_at"].is_string(), "{two}");
        assert_eq!(rounds[0]["budget"]["spent"], json!(1200));
        assert!(
            rounds[0]["budget"]["tokens"]
                .as_i64()
                .is_some_and(|tokens| tokens > 0)
        );
        assert!(rounds[0]["started_at"].is_string());
        assert_eq!(rounds[1]["round"], json!(2));
        assert_eq!(
            (&rounds[1]["ended_at"], &rounds[1]["stop_reason"]),
            (&Value::Null, &Value::Null)
        );
        assert_eq!(rounds[1]["budget"]["spent"], two["budget"]["spent"]);
        let spent = two["budget"]["spent"].as_i64().ok_or("spent")?;
        assert_eq!(two["tokens_all_rounds"], json!(1200 + spent));
        let round_of = |name: &str| {
            two["helpers"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["name"] == json!(name)))
                .map(|row| row["round"].clone())
        };
        assert_eq!(
            round_of("researcher"),
            Some(json!(2)),
            "brought back into round 2"
        );
        assert_eq!(round_of("reviewer"), Some(json!(1)), "not in this round");
        // The person's log has round 2's start, for them, and round 1's end note names its round (as the contract says).
        let log = f
            .get(
                &format!(
                    "/api/v1/conversations/{}/team/messages?after=0&limit=200",
                    t.lead.0
                ),
                &t.cookie,
            )
            .await?;
        assert_eq!(log.status(), 200);
        let log = log.json::<Value>().await?;
        let items = log["items"].as_array().ok_or("items")?;
        let started = items
            .iter()
            .find(|item| item["type"] == json!("round_started"))
            .ok_or("round_started")?;
        assert_eq!(
            (&started["round"], &started["to"]["kind"]),
            (&json!(2), &json!("person")),
            "{started}"
        );
        if let Some(ended) = items.iter().find(|item| item["type"] == json!("ended")) {
            assert_eq!(ended["round"], json!(1), "{ended}");
        }
        Ok(())
    })
    .await
}

/// A usage row for `task`, `tokens` in all, at `ago` seconds before now.
async fn usage(f: &Fixture, task: &str, tokens: i64, ago: i32) -> Result<()> {
    sqlx::query(
        "INSERT INTO usage (id, conversation_id, route, prompt_tokens, completion_tokens, created_at) \
         VALUES ($1, $2::uuid, 'local', $3, 0, CURRENT_TIMESTAMP - make_interval(secs => $4))",
    )
    .bind(Uuid::now_v7())
    .bind(task)
    .bind(tokens)
    .bind(f64::from(ago))
    .execute(&f.pool)
    .await?;
    Ok(())
}

// TEST: GW-I-338
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_338_a_rounds_end_note_counts_only_that_rounds_reports_members_and_usage(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // The team started a while ago; round 1's usage and report come before it ends.
        sqlx::query("UPDATE agent_teams SET created_at = CURRENT_TIMESTAMP - interval '1 hour' WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        usage(f, t.researcher.0.as_str(), 300, 1800).await?;
        let (status, body) = send(f, &t.researcher.1, "r-2:c-1", &json!({"type": "report", "text": "Round one."})).await?;
        assert_eq!(status, 201, "{body}");
        // Round 1's end stops a helper still at work, as the end path does; between_rounds writes only the team row.
        sqlx::query("UPDATE agent_team_members SET state = 'stopped' WHERE team_id = $1 AND name = 'reviewer'")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        between_rounds(f, &t).await?;
        let (status, body) = start_with(
            f,
            &t,
            "r-5:c-1",
            json!({"role": "researcher", "workspace": "own_copy", "brief": brief()}),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        // Round 2: the researcher uses 120 tokens and reports once; the lead ends it.
        usage(f, t.researcher.0.as_str(), 120, 0).await?;
        sqlx::query("UPDATE agent_teams SET spent_tokens = 120 WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        let (status, body) = send(f, &t.researcher.1, "r-6:c-1", &json!({"type": "report", "text": "Round two."})).await?;
        assert_eq!(status, 201, "{body}");
        let (status, body) = done(f, &t.lead.1, "r-5:done", &json!({"summary": "Round two."})).await?;
        assert_eq!(status, 200, "{body}");
        until_ended(f, t.id).await?;
        let notes = end_notes(f, t.id).await?;
        let last = notes.last().ok_or("an end note")?;
        assert_eq!(last["round"], json!(2), "{last}");
        // Only round 2's report, and only round 2's members with round 2's usage.
        let reports = last["reports"].as_array().ok_or("reports")?;
        assert_eq!(reports.len(), 1, "{last}");
        assert_eq!(last["tokens"]["members"], json!([{"name": "researcher", "tokens": 120}]), "{last}");
        Ok(())
    })
    .await
}
