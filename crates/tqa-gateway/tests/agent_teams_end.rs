//! Agent teams from a run after their end, slice 3 (ADR-0034 point 10).
//! - GW-I-108: `done` stores the summary and sets the team stopping; after the end every tool route answers
//!   `team_ended`, the lead's first call carrying what it hadn't read by the read rule; a lead leads one team.
//! - GW-I-261: the stop `done` leaves to the reconciler is carried out within a second, not at its next pass.
//! - GW-I-302: a person's Stop team cancels the lead's own run too; `done` still leaves it running.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::TestResult;
use uuid::Uuid;

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_after_the_end_the_leads_first_team_call_carries_what_it_hadnt_read(
    pool: PgPool,
) -> TestResult {
    // ADR-0034 point 10: the end note reaches the lead on its next team call, by the read rule exactly. Until the
    // 'ended' type is in the store, an unread report stands in for it: the same cursor, one more message to the lead.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let (status, _) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "All done."}),
        )
        .await?;
        assert_eq!(status, 201);
        end(f, t.id).await?;
        let refused = |answer: &Value| answer["error"]["code"] == "team_ended";
        let (status, first) = send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "Still there?"}),
        )
        .await?;
        assert_eq!(status, 409, "{first}");
        assert!(refused(&first));
        assert_eq!(seqs(&first["error"]["details"]), [1], "{first}");
        // A repeat of the same call answers the same: the answer is recorded under its key.
        let (_, again) = send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "Still there?"}),
        )
        .await?;
        assert_eq!(again, first);
        // While that answer is outstanding (no tool.result stored yet), a new call doesn't repeat it: the read rule.
        // Had its run ended without one, the answer would be pruned and the next run's call would carry it.
        let (_, lost) = read(f, &t.lead.1, ("r-1:c-2", "r-1")).await?;
        assert!(refused(&lost));
        assert!(
            seqs(&lost["error"]["details"]).is_empty(),
            "outstanding answers aren't repeated by a new key"
        );
        // Once the first call's tool.result is stored, the seq has advanced, and later calls carry nothing.
        f.gateway
            .events()
            .append(&t.lead.0, result(2, "r-1", "c-1")?)
            .await?;
        let last: i64 =
            sqlx::query_scalar("SELECT lead_last_read_seq FROM agent_teams WHERE id = $1")
                .bind(t.id)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(last, 1);
        let (status, later) = read(f, &t.lead.1, ("r-1:c-3", "r-1")).await?;
        assert_eq!((status, refused(&later)), (409, true));
        assert!(later["error"].get("details").is_none(), "{later}");
        // A member's calls answer team_ended, carrying nothing.
        let (status, member) = send(
            f,
            &t.reviewer.1,
            "r-3:c-1",
            &json!({"type": "ask", "text": "?"}),
        )
        .await?;
        assert_eq!((status, refused(&member)), (409, true));
        assert!(member["error"].get("details").is_none());
        // A conversation never in a team still hears not_a_member.
        let (_, outsider) = read(f, &t.outsider.1, ("r-4:c-1", "r-4")).await?;
        assert_eq!(outsider["error"]["code"], "not_a_member");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_a_lead_whose_team_ended_never_leads_again(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        end(f, t.id).await?;
        let response = f
            .client
            .get(f.url("/runtime/teams/role"))
            .bearer_auth(&t.lead.1)
            .send()
            .await?;
        let role: Value = response.json().await?;
        assert_eq!(role["role"], "none", "the next run gets no team tools");
        let response = f
            .client
            .post(f.url("/runtime/teams/members"))
            .bearer_auth(&t.lead.1)
            .header("idempotency-key", "r-1:c-9")
            .header("x-tqa-run-id", "r-1")
            .json(&json!({"role": "r", "brief": {"objective": "o", "output": "p", "boundaries": "b"}}))
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 409);
        let refused: Value = response.json().await?;
        assert_eq!(refused["error"]["code"], "team_ended");
        let teams: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_teams WHERE lead_conversation = $1")
            .bind(Uuid::parse_str(t.lead.0.as_str())?)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(teams, 1, "no second team");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_done_stores_the_summary_and_sets_the_team_stopping(pool: PgPool) -> TestResult {
    // ADR-0034 point 10: `done` has no stop logic of its own; the stop carries out `stopping` and posts the end note.
    support::run_configured(pool, None, ENABLED, async |f| {
        // `done` nudges the reconciler to carry the stop out (GW-I-261): held, the team stays `stopping` to look at.
        f.gateway.agent_teams_hold(true);
        let t = team(f).await?;
        // A member can't, and the summary's rules name it.
        let (status, refused) = done(f, &t.researcher.1, "r-2:c-1", &json!({"summary": "x"})).await?;
        assert_eq!((status, &refused["error"]["code"]), (403, &json!("not_lead")));
        for body in [json!({"summary": ""}), json!({"summary": "x".repeat(8 * 1024 + 1)}), json!({})] {
            let (status, refused) = done(f, &t.lead.1, "r-1:c-9", &body).await?;
            assert_eq!(status, 400, "{refused}");
            assert_eq!(refused["error"]["details"][0]["field"], "summary", "{refused}");
        }
        let (status, unknown) = done(f, &t.lead.1, "r-1:c-9", &json!({"summary": "x", "tone": "y"})).await?;
        assert_eq!((status, &unknown["error"]["details"][0]["field"]), (400, &json!("tone")));
        let (status, answer) = done(f, &t.lead.1, "r-1:c-1", &json!({"summary": "Q3 table done."})).await?;
        assert_eq!((status, &answer), (200, &json!({"team": {"state": "stopping"}})));
        let (state, reason): (String, Option<String>) =
            sqlx::query_as("SELECT state, stop_reason FROM agent_teams WHERE id = $1")
                .bind(t.id)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!((state.as_str(), reason.as_deref()), ("stopping", Some("done")));
        // The summary is a `done` message to every member, once.
        let (status, read) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        assert_eq!(status, 200, "{read}");
        assert_eq!(read["messages"][0]["type"], "done");
        assert_eq!(read["messages"][0]["text"], "Q3 table done.");
        // A repeat, with the same key or another, answers the same and stores nothing more.
        for key in ["r-1:c-1", "r-1:c-2"] {
            let (status, again) = done(f, &t.lead.1, key, &json!({"summary": "Q3 table done."})).await?;
            assert_eq!((status, &again), (200, &answer), "{key}");
        }
        let stored: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
             WHERE t.id = $1 AND m.type = 'done'",
        )
        .bind(t.id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(stored, 1);
        // Stopping, posts are refused.
        let (status, refused) = send(f, &t.lead.1, "r-1:c-3", &json!({"type": "question", "text": "x"})).await?;
        assert_eq!((status, &refused["error"]["code"]), (409, &json!("team_stopped")));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_done_while_stopping_for_another_reason_is_team_stopped(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        sqlx::query(
            "UPDATE agent_teams SET state = 'stopping', stop_reason = 'budget' WHERE id = $1",
        )
        .bind(t.id)
        .execute(&f.pool)
        .await?;
        let (status, refused) = done(f, &t.lead.1, "r-1:c-1", &json!({"summary": "x"})).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (409, &json!("team_stopped"))
        );
        let reason: Option<String> =
            sqlx::query_scalar("SELECT stop_reason FROM agent_teams WHERE id = $1")
                .bind(t.id)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(reason.as_deref(), Some("budget"), "the first reason stays");
        Ok(())
    })
    .await
}

/// GW-I-261's bound: 3 s, scaled for slow runs but kept under 12 s, so the reconciler's 15 s pass can never meet it.
fn nudge_bound() -> std::time::Duration {
    tqa_testkit::deadline::scaled(std::time::Duration::from_secs(3))
        .min(std::time::Duration::from_secs(12))
}

/// How long `team` took to reach `ended` from now, polled every 10 ms up to [`nudge_bound`].
async fn ended_after(f: &support::Fixture, team: Uuid) -> Result<std::time::Duration> {
    let from = std::time::Instant::now();
    while from.elapsed() < nudge_bound() {
        let state: String = sqlx::query_scalar("SELECT state FROM agent_teams WHERE id = $1")
            .bind(team)
            .fetch_one(&f.pool)
            .await?;
        if state == "ended" {
            return Ok(from.elapsed());
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Err(format!("the team didn't end within {:?}", nudge_bound()).into())
}

// TEST: GW-I-261
/// Watch 2: `done` leaves the stop to the reconciler (it cancels the lead's own run), which is nudged after the commit,
/// so the team ends within seconds (about one here) rather than at the next 15 s pass.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_261_done_ends_the_team_at_once_not_at_the_next_pass(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let mut t = team(f).await?;
        let (status, answer) = done(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"summary": "Q3 table done."}),
        )
        .await?;
        assert_eq!(
            (status, &answer),
            (200, &json!({"team": {"state": "stopping"}}))
        );
        ended_after(f, t.id).await?;
        // The stop cancels the helpers' runs, never the lead's: it gets done's answer and goes on to its closing words.
        let cancels = |frames: &[tqa_types::ClientFrame]| {
            frames
                .iter()
                .filter(|frame| matches!(frame, tqa_types::ClientFrame::RunCancel { .. }))
                .count()
        };
        let mut seen = [Vec::new(), Vec::new()];
        for (at, runtime) in t.runtimes.iter_mut().take(2).enumerate() {
            while let Some(frame) = runtime
                .try_action(std::time::Duration::from_millis(300))
                .await?
            {
                seen[at].push(frame);
            }
        }
        assert_eq!(
            cancels(&seen[0]),
            0,
            "the lead's run isn't cancelled: {:?}",
            seen[0]
        );
        assert_eq!(
            cancels(&seen[1]),
            1,
            "the researcher's run is: {:?}",
            seen[1]
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-302
/// Real-world use before 17.0 (17 Oct 2026): Stop team stopped the helpers, and the lead went on working alone until
/// its person stopped it too. A person's stop is the whole team's, the lead's own run included.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_302_a_persons_stop_team_cancels_the_leads_run_too(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let mut t = team(f).await?;
        let response = f
            .client
            .post(f.url(&format!("/api/v1/conversations/{}/team/stop", t.lead.0)))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .header("idempotency-key", "s-1")
            .json(&json!({}))
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 202);
        ended_after(f, t.id).await?;
        let mut cancels = Vec::new();
        for runtime in t.runtimes.iter_mut().take(3) {
            let mut seen = 0;
            while let Some(frame) = runtime
                .try_action(std::time::Duration::from_millis(300))
                .await?
            {
                seen += usize::from(matches!(frame, tqa_types::ClientFrame::RunCancel { .. }));
            }
            cancels.push(seen);
        }
        assert_eq!(
            cancels,
            [1, 1, 1],
            "the lead's run and both helpers' are cancelled once each"
        );
        Ok(())
    })
    .await
}
