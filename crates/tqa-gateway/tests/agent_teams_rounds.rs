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
