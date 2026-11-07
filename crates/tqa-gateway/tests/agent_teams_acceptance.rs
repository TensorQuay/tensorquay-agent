//! GW-I-198: the administrator's acceptance check on the lead's `done` (ADR-0034 point 10). The lead's runtime runs it
//! in the lead's workspace and sends what it did with `done`; the gateway holds `done` to it. Unset, `done` goes on and
//! a report is ignored. Set: a passing report for the set command lets `done` go on; a failing one is refused
//! `acceptance_failed` with the exit and the output's last 4 KiB, and nothing is stored, so the team keeps working; no
//! report (an older runtime) and a report for another command are refused in words. A retried `done` answers as its
//! first did, whatever a rerun of the check printed. `GET /runtime/teams/acceptance` gives the lead the command to run.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::TestResult;

async fn set_check(f: &support::Fixture, check: Option<&str>) -> TestResult {
    sqlx::query("UPDATE agent_team_settings SET acceptance_check = $1")
        .bind(check)
        .execute(&f.pool)
        .await?;
    Ok(())
}

async fn acceptance(f: &support::Fixture, token: &str) -> Result<(u16, Value)> {
    let response = f
        .client
        .get(f.url("/runtime/teams/acceptance"))
        .bearer_auth(token)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

async fn team_state(f: &support::Fixture, team: uuid::Uuid) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM agent_teams WHERE id = $1")
            .bind(team)
            .fetch_one(&f.pool)
            .await?,
    )
}

async fn done_messages(f: &support::Fixture, team: uuid::Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
         WHERE t.id = $1 AND m.type = 'done'",
    )
    .bind(team)
    .fetch_one(&f.pool)
    .await?)
}

fn report(command: &str, exit_code: Option<i32>, timed_out: bool, output: &str) -> Value {
    json!({"command": command, "exit_code": exit_code, "timed_out": timed_out, "output": output})
}

const CHECK: &str = "cargo test --workspace";

// TEST: GW-I-198
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_198_unset_done_goes_on_and_a_report_is_ignored(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        f.gateway.agent_teams_hold(true);
        let t = team(f).await?;
        assert_eq!(
            acceptance(f, &t.lead.1).await?,
            (200, json!({"check": null}))
        );
        // A report sent anyway, even a failing one, changes nothing while no check is set.
        let body = json!({"summary": "Done.", "check": report("false", Some(1), false, "no")});
        let (status, answer) = done(f, &t.lead.1, "r-1:c-1", &body).await?;
        assert_eq!(
            (status, &answer),
            (200, &json!({"team": {"state": "stopping"}}))
        );
        assert_eq!(team_state(f, t.id).await?, "stopping");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_198_the_lead_reads_the_check_and_a_helper_cant(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        set_check(f, Some(CHECK)).await?;
        assert_eq!(
            acceptance(f, &t.lead.1).await?,
            (200, json!({"check": CHECK}))
        );
        let (status, refused) = acceptance(f, &t.researcher.1).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (403, &json!("not_lead"))
        );
        let (status, refused) = acceptance(f, &t.outsider.1).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (403, &json!("not_a_member"))
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_198_a_failing_check_refuses_done_and_the_team_keeps_working(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        f.gateway.agent_teams_hold(true);
        let t = team(f).await?;
        set_check(f, Some(CHECK)).await?;
        // 5 KiB of output, its last 4 KiB recognisable: the refusal carries exactly those.
        let output = format!("{}{}", "a".repeat(1024), "b".repeat(4096));
        let body = json!({"summary": "Done.", "check": report(CHECK, Some(1), false, &output)});
        let (status, refused) = done(f, &t.lead.1, "r-1:c-1", &body).await?;
        assert_eq!(status, 409, "{refused}");
        assert_eq!(refused["error"]["code"], "acceptance_failed");
        assert_eq!(refused["error"]["retryable"], false);
        let details = &refused["error"]["details"];
        assert_eq!(
            (&details["exit_code"], &details["timed_out"]),
            (&json!(1), &json!(false))
        );
        assert_eq!(details["output"], "b".repeat(4096));
        assert_eq!(team_state(f, t.id).await?, "running");
        assert_eq!(done_messages(f, t.id).await?, 0, "nothing stored");
        // The team works on: a post goes through.
        let (status, _) = send(
            f,
            &t.lead.1,
            "r-1:c-2",
            &json!({"type": "question", "text": "Fix it?"}),
        )
        .await?;
        assert_eq!(status, 201);
        // A timed-out check fails too, whatever its exit.
        let body = json!({"summary": "Done.", "check": report(CHECK, None, true, "")});
        let (status, refused) = done(f, &t.lead.1, "r-1:c-3", &body).await?;
        assert_eq!(
            (status, &refused["error"]["details"]["timed_out"]),
            (409, &json!(true))
        );
        // Once the work passes, `done` goes on.
        let body = json!({"summary": "Done.", "check": report(CHECK, Some(0), false, "ok")});
        let (status, answer) = done(f, &t.lead.1, "r-1:c-4", &body).await?;
        assert_eq!(
            (status, &answer),
            (200, &json!({"team": {"state": "stopping"}}))
        );
        assert_eq!(done_messages(f, t.id).await?, 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_198_no_report_or_one_for_another_command_is_refused_in_words(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        f.gateway.agent_teams_hold(true);
        let t = team(f).await?;
        set_check(f, Some(CHECK)).await?;
        // An older runtime sends none: the words tell the person what to do.
        let (status, refused) = done(f, &t.lead.1, "r-1:c-1", &json!({"summary": "Done."})).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (409, &json!("acceptance_failed")),
            "{refused}"
        );
        let words = refused["error"]["message"].as_str().unwrap_or_default();
        assert!(
            words.contains("older than the server") && words.contains("Stop the task"),
            "{words}"
        );
        assert!(refused["error"].get("details").is_none());
        // The check changed while it ran: run it again.
        let body = json!({"summary": "Done.", "check": report("make check", Some(0), false, "")});
        let (status, refused) = done(f, &t.lead.1, "r-1:c-2", &body).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (409, &json!("acceptance_failed"))
        );
        let words = refused["error"]["message"].as_str().unwrap_or_default();
        assert!(
            words.contains("changed") && words.contains("call done again"),
            "{words}"
        );
        // A report with a field it doesn't have is refused naming it.
        let mut odd = report(CHECK, Some(0), false, "");
        odd["passed"] = json!(true);
        let (status, refused) = done(
            f,
            &t.lead.1,
            "r-1:c-3",
            &json!({"summary": "Done.", "check": odd}),
        )
        .await?;
        assert_eq!(status, 400, "{refused}");
        assert_eq!(team_state(f, t.id).await?, "running");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_198_a_retried_done_answers_as_its_first_did(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        f.gateway.agent_teams_hold(true);
        let t = team(f).await?;
        set_check(f, Some(CHECK)).await?;
        let body = json!({"summary": "Done.", "check": report(CHECK, Some(0), false, "ok")});
        let (status, first) = done(f, &t.lead.1, "r-1:c-1", &body).await?;
        assert_eq!(status, 200, "{first}");
        // The setting changes after the 200: the retry still answers the first answer.
        set_check(f, Some("make check")).await?;
        assert_eq!(
            done(f, &t.lead.1, "r-1:c-1", &body).await?,
            (200, first.clone())
        );
        // A rerun after a restart prints otherwise (timings): the key holds the summary and the command, not the output.
        let rerun =
            json!({"summary": "Done.", "check": report(CHECK, Some(0), false, "ok in 2.1 s")});
        assert_eq!(done(f, &t.lead.1, "r-1:c-1", &rerun).await?, (200, first));
        // Another summary under that key is still another request.
        let other = json!({"summary": "Other.", "check": report(CHECK, Some(0), false, "ok")});
        let (status, refused) = done(f, &t.lead.1, "r-1:c-1", &other).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (422, &json!("idempotency_key_reused"))
        );
        assert_eq!(done_messages(f, t.id).await?, 1);
        Ok(())
    })
    .await
}
