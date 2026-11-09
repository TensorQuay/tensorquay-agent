//! A helper's state as it happens (ADR-0034 point 9, the member states of 4 Oct 2026), through the routes and the run
//! events only, no state set by hand (the first turn's `queued` is GW-I-106's; GW-I-306's reminder rule sets a
//! helper `queued` or `starting` by hand, the states only a first turn has).
//! - GW-I-212: a run started or resumed makes a helper `working`; one that ends makes it `done` after a report since its
//!   last assignment, else `waiting`; a done helper is woken by a new assignment, counts toward the member cap (whose
//!   refusal tells the lead to stop one), and is stopped, freeing its seat.
//! - GW-I-260 (E2E-AGENTS-021): a failed run makes a helper `failed`, as its person's page shows it; it keeps its seat,
//!   the lead's next message wakes it and its next run makes it `working`; `stop_member` stops it.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, ErrorCode, Event, ServerFrame};
use uuid::Uuid;

async fn member_state(f: &Fixture, member: &ConversationId) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM agent_team_members WHERE conversation_id = $1")
            .bind(Uuid::parse_str(member.as_str())?)
            .fetch_one(&f.pool)
            .await?,
    )
}

/// `conversation`'s run `run` stores `event` at `sequence`, as its runtime reports it.
async fn event(
    f: &Fixture,
    conversation: &ConversationId,
    (sequence, run): (u64, &str),
    event: Event,
) -> TestResult {
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

async fn assign(f: &Fixture, t: &Team, key: &str, to: &str) -> TestResult {
    let (status, answer) = send(
        f,
        &t.lead.1,
        key,
        &json!({"type": "assign", "to": to, "text": "The north region."}),
    )
    .await?;
    assert_eq!(status, 201, "{answer}");
    Ok(())
}

/// The researcher reports on its assignment and its run ends: done.
async fn researcher_done(f: &Fixture, t: &Team) -> TestResult {
    assign(f, t, "r-1:c-1", "researcher").await?;
    let (status, answer) = send(
        f,
        &t.researcher.1,
        "r-2:c-1",
        &json!({"type": "report", "text": "North: 4.1 M."}),
    )
    .await?;
    assert_eq!(status, 201, "{answer}");
    run_ends(f, &t.researcher.0, 2, "r-2").await?;
    assert_eq!(member_state(f, &t.researcher.0).await?, "done");
    Ok(())
}

async fn start(f: &Fixture, t: &Team, key: &str, role: &str) -> Result<(u16, Value)> {
    let response = f
        .client
        .post(f.url("/runtime/teams/members"))
        .bearer_auth(&t.lead.1)
        .header("idempotency-key", key)
        .header("x-tqa-run-id", "r-1")
        .json(&json!({
            "role": role, "workspace": "own_copy",
            "brief": {"objective": "Check the totals.", "output": "A table.", "boundaries": "Read only."},
        }))
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_212_a_helpers_runs_make_it_working_done_or_waiting(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        researcher_done(f, &t).await?;
        // The reviewer's run ends with no report: it waits for its lead. Its next run makes it working, and one
        // cancelled without a report leaves it waiting again.
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        assert_eq!(member_state(f, &t.reviewer.0).await?, "waiting");
        f.gateway
            .events()
            .append(&t.reviewer.0, support::started(3, "r-7")?)
            .await?;
        assert_eq!(member_state(f, &t.reviewer.0).await?, "working");
        let cancelled = Event::RunCancelled {
            run_id: "r-7".parse()?,
        };
        event(f, &t.reviewer.0, (4, "r-7"), cancelled).await?;
        assert_eq!(member_state(f, &t.reviewer.0).await?, "waiting");
        // A new assignment wakes the done researcher; its run makes it working (a failed run is GW-I-260's).
        assign(f, &t, "r-1:c-2", "researcher").await?;
        assert_eq!(
            wakes_settle(f, &t.researcher.0, 1).await?.len(),
            1,
            "a done helper is woken"
        );
        assert_eq!(
            member_state(f, &t.researcher.0).await?,
            "done",
            "until its run starts"
        );
        f.gateway
            .events()
            .append(&t.researcher.0, support::started(3, "r-8")?)
            .await?;
        assert_eq!(member_state(f, &t.researcher.0).await?, "working");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_212_a_done_helper_keeps_its_seat_until_it_is_stopped(pool: PgPool) -> TestResult {
    // max_members (3) is the cap, not the slots.
    let config =
        format!("{ENABLED}[sandbox]\nmax_live = 16\n[admission]\nslots = 16\nper_user = 16\n");
    support::run_configured(pool, None, &config, async |f| {
        let t = team(f).await?;
        researcher_done(f, &t).await?;
        // The done researcher, the reviewer and a third helper fill the cap.
        let (status, answer) = start(f, &t, "r-1:c-10", "analyst").await?;
        assert_eq!(status, 201, "{answer}");
        let (status, refused) = start(f, &t, "r-1:c-11", "writer").await?;
        assert_eq!(
            (
                status,
                &refused["error"]["code"],
                &refused["error"]["details"]
            ),
            (
                409,
                &json!("member_limit"),
                &json!({"limit": 3, "by": "max_members"})
            ),
            "{refused}"
        );
        let message = refused["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("done") && message.contains("stop_member"),
            "the refusal tells the lead how to free a seat: {message}"
        );
        // Stopping the done helper frees its seat.
        let stopped = f
            .client
            .post(f.url("/runtime/teams/members/researcher/stop"))
            .bearer_auth(&t.lead.1)
            .header("idempotency-key", "r-1:c-12")
            .header("x-tqa-run-id", "r-1")
            .json(&json!({"reason": "Finished."}))
            .send()
            .await?;
        assert_eq!(stopped.status(), 200);
        assert_eq!(member_state(f, &t.researcher.0).await?, "stopped");
        let (status, answer) = start(f, &t, "r-1:c-13", "writer").await?;
        assert_eq!(status, 201, "{answer}");
        Ok(())
    })
    .await
}

fn failed(run: &str) -> Result<Event> {
    Ok(Event::RunFailed {
        run_id: run.parse()?,
        reason: "the model provider kept failing".into(),
        code: ErrorCode::ProviderError,
        provider_message: None,
    })
}

/// The lead's task as its person reads it: the helpers needing attention.
async fn attention(f: &Fixture, t: &Team) -> Result<Value> {
    let response = f
        .get(&format!("/api/v1/conversations/{}", t.lead.0), &t.cookie)
        .await?;
    assert_eq!(response.status(), 200);
    Ok(response.json::<Value>().await?["team"]["attention"].clone())
}

/// The helper's own task as its person reads it: its state.
async fn helper_state(f: &Fixture, t: &Team, helper: &ConversationId) -> Result<Value> {
    let response = f
        .get(&format!("/api/v1/conversations/{helper}"), &t.cookie)
        .await?;
    assert_eq!(response.status(), 200);
    Ok(response.json::<Value>().await?["team"]["helper_state"].clone())
}

/// Whether the person's page offers Stop helper for `helper`: on the helper's own place and on the lead's helper row.
async fn can_stop(f: &Fixture, t: &Team, helper: &ConversationId) -> Result<(Value, Value)> {
    let own = f
        .get(&format!("/api/v1/conversations/{helper}"), &t.cookie)
        .await?;
    assert_eq!(own.status(), 200);
    let own = own.json::<Value>().await?["team"]["can_stop"].clone();
    let team = f
        .get(
            &format!("/api/v1/conversations/{}/team", t.lead.0),
            &t.cookie,
        )
        .await?;
    assert_eq!(team.status(), 200);
    let team = team.json::<Value>().await?;
    let row = team["helpers"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["conversation"] == json!(helper.as_str()))
        })
        .map_or(Value::Null, |row| row["can_stop"].clone());
    Ok((own, row))
}

// TEST: GW-I-260
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_260_a_failed_run_makes_a_helper_failed_until_the_leads_next_message_starts_it(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // The fixture's researcher is at work on its run r-2 (seq 1).
        assign(f, &t, "r-1:c-1", "researcher").await?;
        assert_eq!(member_state(f, &t.researcher.0).await?, "working");
        // Its run fails: failed, and the page says so.
        event(f, &t.researcher.0, (2, "r-2"), failed("r-2")?).await?;
        assert_eq!(member_state(f, &t.researcher.0).await?, "failed");
        assert_eq!(helper_state(f, &t, &t.researcher.0).await?, "failed");
        assert_eq!(attention(f, &t).await?, json!({"stuck": 0, "failed": 1}));
        // Its seat is taken, so the person's page offers to stop it (GW-U-168).
        assert_eq!(
            can_stop(f, &t, &t.researcher.0).await?,
            (json!(true), json!(true))
        );
        // The lead's next message is taken, not refused member_gone, and wakes it; its run makes it working.
        assign(f, &t, "r-1:c-2", "researcher").await?;
        assert_eq!(
            wakes_settle(f, &t.researcher.0, 1).await?.len(),
            1,
            "a helper whose run failed is woken"
        );
        f.gateway
            .events()
            .append(&t.researcher.0, support::started(3, "r-3")?)
            .await?;
        assert_eq!(member_state(f, &t.researcher.0).await?, "working");
        assert_eq!(attention(f, &t).await?, json!({"stuck": 0, "failed": 0}));
        // It fails again: failed again. It still holds its seat, and stop_member stops it.
        event(f, &t.researcher.0, (4, "r-3"), failed("r-3")?).await?;
        assert_eq!(member_state(f, &t.researcher.0).await?, "failed");
        let failed_run: bool = sqlx::query_scalar(
            "SELECT failed_run FROM agent_team_members WHERE conversation_id = $1",
        )
        .bind(Uuid::parse_str(t.researcher.0.as_str())?)
        .fetch_one(&f.pool)
        .await?;
        assert!(failed_run);
        // The failed run r-3 was itself a wake and failed before reading, so its place hasn't moved: the lead's next
        // message still wakes it, as a new wake.
        assign(f, &t, "r-1:c-4", "researcher").await?;
        let woken = wakes_settle(f, &t.researcher.0, 2).await?;
        assert_eq!(woken.len(), 2, "a second wake: {woken:?}");
        assert_ne!(woken[0], woken[1]);
        f.gateway
            .events()
            .append(&t.researcher.0, support::started(5, "r-4")?)
            .await?;
        assert_eq!(member_state(f, &t.researcher.0).await?, "working");
        let stopped = f
            .client
            .post(f.url("/runtime/teams/members/researcher/stop"))
            .bearer_auth(&t.lead.1)
            .header("idempotency-key", "r-1:c-3")
            .header("x-tqa-run-id", "r-1")
            .json(&json!({"reason": "It keeps failing."}))
            .send()
            .await?;
        assert_eq!(stopped.status(), 200);
        assert_eq!(member_state(f, &t.researcher.0).await?, "stopped");
        assert_eq!(
            can_stop(f, &t, &t.researcher.0).await?,
            (json!(false), json!(false))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-260
// A failed helper keeps its seat: at the member cap, a new helper is refused until the failed one is stopped.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_260_a_helper_whose_run_failed_keeps_its_seat(pool: PgPool) -> TestResult {
    let config =
        format!("{ENABLED}[sandbox]\nmax_live = 16\n[admission]\nslots = 16\nper_user = 16\n");
    support::run_configured(pool, None, &config, async |f| {
        let t = team(f).await?;
        // The fixture's researcher is at work on its run r-2 (seq 1).
        assign(f, &t, "r-1:c-1", "researcher").await?;
        event(f, &t.researcher.0, (2, "r-2"), failed("r-2")?).await?;
        assert_eq!(member_state(f, &t.researcher.0).await?, "failed");
        let (status, answer) = start(f, &t, "r-1:c-10", "analyst").await?;
        assert_eq!(status, 201, "{answer}");
        let (status, refused) = start(f, &t, "r-1:c-11", "writer").await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (409, &json!("member_limit")),
            "{refused}"
        );
        Ok(())
    })
    .await
}

/// Waits until `team` has ended (the stop `done` leaves to the reconciler is carried out within a second, GW-I-261).
async fn until_ended(f: &Fixture, team: Uuid) -> TestResult {
    for _ in 0..50 {
        let state: String = sqlx::query_scalar("SELECT state FROM agent_teams WHERE id = $1")
            .bind(team)
            .fetch_one(&f.pool)
            .await?;
        if state == "ended" {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("the team didn't end".into())
}

// TEST: GW-I-301
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_301_a_helper_that_reported_is_done_when_the_teams_end_stops_it(
    pool: PgPool,
) -> TestResult {
    // Real-world use before 17.0 (17 Oct 2026): postgresql reported (39 % of the work, in compare.md) and was finishing
    // its turn when the lead called done; the team's end stopped it and All helpers showed it Stopped, grey, like a
    // failure.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        assign(f, &t, "r-1:c-1", "researcher").await?;
        let (status, answer) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "North: 4.1 M."}),
        )
        .await?;
        assert_eq!(status, 201, "{answer}");
        // Its run goes on after the report; the reviewer is at work with no report.
        assert_eq!(member_state(f, &t.researcher.0).await?, "working");
        assert_eq!(member_state(f, &t.reviewer.0).await?, "working");
        let (status, answer) =
            done(f, &t.lead.1, "r-1:c-2", &json!({"summary": "Totals in."})).await?;
        assert_eq!(status, 200, "{answer}");
        until_ended(f, t.id).await?;
        assert_eq!(
            member_state(f, &t.researcher.0).await?,
            "done",
            "it reported since its assignment: done, as a run that ends after a report makes it"
        );
        assert_eq!(
            member_state(f, &t.reviewer.0).await?,
            "stopped",
            "no report: stopped by the team's end"
        );
        Ok(())
    })
    .await
}

/// The gateway's reminders to the lead that its team still runs (GW-I-306), oldest first.
async fn reminders(f: &Fixture, team: Uuid) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT m.body->>'text' FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
         WHERE t.id = $1 AND m.type = 'notice' AND m.body->>'kind' = 'lead_unfinished' AND m.to_kind = 'lead' \
         ORDER BY m.seq",
    )
    .bind(team)
    .fetch_all(&f.pool)
    .await?)
}

const REMINDER: &str = "Your team is still running and your turn ended without done. If the work is finished, call \
                        done: your finish check runs then. Otherwise keep leading: read your helpers or assign.";

/// The lead's run `run` ends at `sequence` as its model finished it, then its next run starts, as a wake would.
async fn lead_turn(f: &Fixture, t: &Team, sequence: u64, run: &str) -> TestResult {
    run_ends(f, &t.lead.0, sequence, run).await
}

/// Both helpers' runs end with no report: `waiting`, quiet, nothing on its way to the lead.
async fn quiet(f: &Fixture, t: &Team) -> TestResult {
    run_ends(f, &t.researcher.0, 2, "r-2").await?;
    run_ends(f, &t.reviewer.0, 2, "r-3").await?;
    Ok(())
}

/// Sets a helper's state by hand: `queued` and `starting` are a first turn's, which these cases don't run.
async fn set_state(f: &Fixture, member: &ConversationId, state: &str) -> TestResult {
    sqlx::query("UPDATE agent_team_members SET state = $2 WHERE conversation_id = $1")
        .bind(Uuid::parse_str(member.as_str())?)
        .bind(state)
        .execute(&f.pool)
        .await?;
    Ok(())
}

/// The lead's next run starts at `sequence`, as a wake would start it.
async fn lead_starts(f: &Fixture, t: &Team, sequence: u64, run: &str) -> TestResult {
    f.gateway
        .events()
        .append(&t.lead.0, support::started(sequence, run)?)
        .await?;
    Ok(())
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_no_reminder_while_a_helper_works_is_queued_or_starting(
    pool: PgPool,
) -> TestResult {
    // The 16n integration (17 Oct 2026): a lead ended its turn to wait for its helper's report, as the read tool tells
    // it to; the reminder came first, the lead called done, and the helper's run was cancelled. A helper that will run
    // and report wakes the lead itself: the reminder is only for a lead nothing else can wake.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        lead_turn(f, &t, 2, "r-1").await?;
        assert!(reminders(f, t.id).await?.is_empty(), "both helpers work");
        run_ends(f, &t.researcher.0, 2, "r-2").await?;
        set_state(f, &t.reviewer.0, "queued").await?;
        lead_starts(f, &t, 3, "r-9").await?;
        lead_turn(f, &t, 4, "r-9").await?;
        assert!(
            reminders(f, t.id).await?.is_empty(),
            "a helper queued for a slot"
        );
        set_state(f, &t.reviewer.0, "starting").await?;
        lead_starts(f, &t, 5, "r-10").await?;
        lead_turn(f, &t, 6, "r-10").await?;
        assert!(reminders(f, t.id).await?.is_empty(), "a helper starting");
        // The starting helper's run starts and ends with no report: every helper is quiet, nothing else can wake the
        // idle lead, and it is reminded as that run ends.
        f.gateway
            .events()
            .append(&t.reviewer.0, support::started(2, "r-12")?)
            .await?;
        assert_eq!(member_state(f, &t.reviewer.0).await?, "working");
        run_ends(f, &t.reviewer.0, 3, "r-12").await?;
        assert_eq!(reminders(f, t.id).await?, [REMINDER], "all quiet");
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_the_last_helper_going_quiet_after_the_leads_turn_reminds_it(
    pool: PgPool,
) -> TestResult {
    // A helper reports, the lead reads it and ends its turn while the helper is still finishing its own: when that
    // helper's run ends, nothing else can wake the lead, so the reminder comes then (a case found in real-world use
    // before 17.0, reached later).
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        lead_turn(f, &t, 2, "r-1").await?;
        run_ends(f, &t.researcher.0, 2, "r-2").await?;
        assert!(
            reminders(f, t.id).await?.is_empty(),
            "one helper still works"
        );
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        assert_eq!(
            reminders(f, t.id).await?,
            [REMINDER],
            "the last one went quiet"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_helpers_going_quiet_dont_remind_a_lead_the_person_cancelled(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        event(
            f,
            &t.lead.0,
            (2, "r-1"),
            Event::RunCancelled {
                run_id: "r-1".parse()?,
            },
        )
        .await?;
        quiet(f, &t).await?;
        assert!(reminders(f, t.id).await?.is_empty(), "the person meant it");
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_no_reminder_while_the_leads_question_to_the_person_is_open(
    pool: PgPool,
) -> TestResult {
    // A lead that asked its person and ends its turn to wait for the answer (E2E-AGENTS-020's flow): the answer wakes
    // it, so it isn't reminded meanwhile, even with every helper quiet.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let (status, answer) = send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "person_question", "text": "Round half up or half even?"}),
        )
        .await?;
        assert_eq!(status, 201, "{answer}");
        quiet(f, &t).await?;
        lead_turn(f, &t, 2, "r-1").await?;
        assert!(
            reminders(f, t.id).await?.is_empty(),
            "the person's answer will wake it"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_no_reminder_with_a_turn_queued_on_the_lead(pool: PgPool) -> TestResult {
    // The person writes to the lead after its turn ended: an ordinary turn, which will start the lead anyway (the wake's
    // own guard). The helpers then go quiet; the lead isn't reminded on top of the person's words.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        lead_turn(f, &t, 2, "r-1").await?;
        let sent = f
            .client
            .post(f.url(&format!("/api/v1/conversations/{}/messages", t.lead.0)))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .header("idempotency-key", "p-1")
            .json(&json!({"text": "Add MySQL.", "attachments": []}))
            .send()
            .await?;
        assert_eq!(sent.status().as_u16(), 202);
        // On its way to the lead, in remind_lead's own terms: queued, or sending or delivered to its runtime and not yet
        // started. The delivery pass takes it from `queued` as soon as the lead's run has ended, which a slow
        // (instrumented) runner can let happen before this read.
        let on_its_way: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE conversation_id = $1::uuid \
             AND execution_status IN ('queued', 'sending', 'delivered')",
        )
        .bind(t.lead.0.as_str())
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(on_its_way, 1, "the person's turn is on its way to the lead");
        quiet(f, &t).await?;
        assert!(
            reminders(f, t.id).await?.is_empty(),
            "a turn is on its way to the lead"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_no_reminder_with_an_unread_report_when_all_are_quiet(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        // A report the lead hasn't read: its wake brings the lead back.
        let t = team(f).await?;
        let (status, answer) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "North: 4.1 M."}),
        )
        .await?;
        assert_eq!(status, 201, "{answer}");
        quiet(f, &t).await?;
        lead_turn(f, &t, 2, "r-1").await?;
        assert!(reminders(f, t.id).await?.is_empty(), "an unread report");
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_no_reminder_with_an_unread_person_message_when_all_are_quiet(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        // The person wrote to the lead while its run was active (GW-I-307): held, and copied for its next check.
        let t = team(f).await?;
        quiet(f, &t).await?;
        let sent = f
            .client
            .post(f.url(&format!("/api/v1/conversations/{}/messages", t.lead.0)))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .header("idempotency-key", "p-1")
            .json(&json!({"text": "Add MySQL.", "attachments": []}))
            .send()
            .await?;
        assert_eq!(sent.status().as_u16(), 202);
        lead_turn(f, &t, 2, "r-1").await?;
        assert!(
            reminders(f, t.id).await?.is_empty(),
            "an unread person message"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_a_lead_that_ends_its_turn_while_its_team_runs_is_reminded_twice_at_most(
    pool: PgPool,
) -> TestResult {
    // Real-world use before 17.0 (17 Oct 2026): the lead ended its turn with a full summary and never called done; the
    // page said Done, the team ran on, and the finish check never ran.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        quiet(f, &t).await?;
        lead_turn(f, &t, 2, "r-1").await?;
        assert_eq!(
            reminders(f, t.id).await?,
            [REMINDER],
            "the first unanswered end"
        );
        f.gateway
            .events()
            .append(&t.lead.0, support::started(3, "r-9")?)
            .await?;
        lead_turn(f, &t, 4, "r-9").await?;
        assert_eq!(reminders(f, t.id).await?.len(), 2, "the second");
        f.gateway
            .events()
            .append(&t.lead.0, support::started(5, "r-10")?)
            .await?;
        lead_turn(f, &t, 6, "r-10").await?;
        assert_eq!(
            reminders(f, t.id).await?.len(),
            2,
            "no third: a lead that ignores them can't loop"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_the_leads_own_post_resets_the_count(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        quiet(f, &t).await?;
        lead_turn(f, &t, 2, "r-1").await?;
        event(
            f,
            &t.lead.0,
            (3, "r-1"),
            Event::RunResumed {
                run_id: "r-1".parse()?,
                from_step: 0,
            },
        )
        .await?;
        lead_turn(f, &t, 4, "r-1").await?;
        assert_eq!(reminders(f, t.id).await?.len(), 2);
        // The lead leads again: an assignment is a team call of its own; its next unanswered end is reminded anew.
        event(
            f,
            &t.lead.0,
            (5, "r-1"),
            Event::RunResumed {
                run_id: "r-1".parse()?,
                from_step: 0,
            },
        )
        .await?;
        assign(f, &t, "r-1:c-7", "researcher").await?;
        lead_turn(f, &t, 6, "r-1").await?;
        assert_eq!(reminders(f, t.id).await?.len(), 3);
        Ok(())
    })
    .await
}

// TEST: GW-I-306
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_306_no_reminder_for_a_cancel_unread_messages_or_a_stopping_team(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // A person's cancel: they meant it, and the strip offers Stop team.
        event(
            f,
            &t.lead.0,
            (2, "r-1"),
            Event::RunCancelled {
                run_id: "r-1".parse()?,
            },
        )
        .await?;
        assert!(reminders(f, t.id).await?.is_empty(), "a cancel");
        // A helper's report the lead hasn't read: the wake brings it back anyway.
        event(
            f,
            &t.lead.0,
            (3, "r-1"),
            Event::RunResumed {
                run_id: "r-1".parse()?,
                from_step: 0,
            },
        )
        .await?;
        let (status, answer) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "North: 4.1 M."}),
        )
        .await?;
        assert_eq!(status, 201, "{answer}");
        lead_turn(f, &t, 4, "r-1").await?;
        assert!(reminders(f, t.id).await?.is_empty(), "unread messages");
        // Done: the team stops; nothing to remind.
        event(
            f,
            &t.lead.0,
            (5, "r-1"),
            Event::RunResumed {
                run_id: "r-1".parse()?,
                from_step: 0,
            },
        )
        .await?;
        let (status, answer) =
            done(f, &t.lead.1, "r-1:c-2", &json!({"summary": "North done."})).await?;
        assert_eq!(status, 200, "{answer}");
        lead_turn(f, &t, 6, "r-1").await?;
        assert!(reminders(f, t.id).await?.is_empty(), "a stopping team");
        Ok(())
    })
    .await
}
