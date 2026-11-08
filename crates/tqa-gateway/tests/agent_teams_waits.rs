//! Agent teams from a run, slice 3 (ADR-0034, the S1 contract): waits, their hints, the calling run, and waking.
//! - GW-I-109: a read waits up to its `wait` (its whole wait when nothing comes) and answers as soon as a message is
//!   posted, a repeat at once; every tool
//!   route takes `X-TQA-Run-ID`; a message wakes a reader whose run isn't active, once for several, a stopped member
//!   never; a decision naming a member's `ask` is stored and wakes the asker, one naming a report is refused.
//! - GW-I-196: a waiting read answers `yield_slot` while another task waits for a slot, and a lead `yield_files` while a
//!   `my_files` helper waits for its files.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use support::TestResult;
use uuid::Uuid;

/// GW-I-196's waits need a task that really waits for its turn: once the team's four sandboxes are up, its owner's
/// cap is set to the three that stay when `queue` takes one away, so admission (each second) can neither start the
/// queued task nor evict another for it (each has an active run), and the hint holds however slowly the test runs.
async fn owner_at_cap(f: &support::Fixture, t: &Team) -> TestResult {
    let (status, body) = support::call(
        f,
        reqwest::Method::PUT,
        "/api/v1/admin/limits/admission.per_user",
        &t.cookie,
        (Some(json!({"value": 3, "version": 1})), None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_read_with_nothing_to_read_holds_its_whole_wait_and_answers_empty(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // Longer than the test client's own deadline: the read is answered, not cut off by the client.
        let started = std::time::Instant::now();
        let (status, answer) = read_waiting(f, &t.reviewer.1, ("r-3:c-1", "r-3"), 11).await?;
        assert_eq!(status, 200, "{answer}");
        assert!(seqs(&answer).is_empty(), "{answer}");
        assert!(answer.get("hint").is_none(), "{answer}");
        assert!(started.elapsed() >= Duration::from_secs(11));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_waiting_read_answers_as_soon_as_a_message_is_posted(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let started = std::time::Instant::now();
        let (waited, posted) = tokio::join!(
            read_waiting(f, &t.reviewer.1, ("r-3:c-1", "r-3"), 20),
            async {
                tokio::time::sleep(Duration::from_millis(500)).await;
                send(
                    f,
                    &t.lead.1,
                    "r-1:c-1",
                    &json!({"type": "question", "text": "Ready?"}),
                )
                .await
            }
        );
        let ((status, answer), (posted, _)) = (waited?, posted?);
        assert_eq!((status, posted), (200, 201), "{answer}");
        assert_eq!(seqs(&answer), [1]);
        assert!(answer.get("hint").is_none(), "{answer}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "woken by the post, not the bound"
        );
        // A repeat of the call answers the same at once: the wait came before the answer was recorded.
        let repeated = std::time::Instant::now();
        let (_, again) = read_waiting(f, &t.reviewer.1, ("r-3:c-1", "r-3"), 20).await?;
        assert_eq!(again, answer);
        assert!(
            repeated.elapsed() < Duration::from_secs(5),
            "a repeat never waits"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_wait_with_nothing_new_ends_at_its_bound(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let started = std::time::Instant::now();
        let (status, answer) = read_waiting(f, &t.researcher.1, ("r-2:c-1", "r-2"), 2).await?;
        assert_eq!(status, 200, "{answer}");
        assert!(seqs(&answer).is_empty());
        assert!(answer.get("hint").is_none(), "{answer}");
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_secs(2) && waited < Duration::from_secs(8),
            "{waited:?}"
        );
        // Beyond 30 seconds is refused, as before.
        let (status, _) = read_waiting(f, &t.researcher.1, ("r-2:c-2", "r-2"), 31).await?;
        assert_eq!(status, 400);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_196_a_waiting_read_yields_to_a_task_queued_for_a_slot(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        owner_at_cap(f, &t).await?;
        queue(f, t.id, &t.outsider.0).await?;
        let started = std::time::Instant::now();
        let (status, answer) = read_waiting(f, &t.reviewer.1, ("r-3:c-1", "r-3"), 20).await?;
        assert_eq!(status, 200, "{answer}");
        assert_eq!(answer["hint"], json!({"kind": "yield_slot"}));
        assert!(seqs(&answer).is_empty());
        assert!(started.elapsed() < Duration::from_secs(10));
        // Without a wait there is no hint: only a waiting read yields.
        let (_, at_once) = read(f, &t.reviewer.1, ("r-3:c-2", "r-3")).await?;
        assert!(at_once.get("hint").is_none(), "{at_once}");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_196_a_lead_yields_its_files_to_a_queued_my_files_helper(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // On purpose: the fixture's helpers work in their own copies, and this one works in the lead's files.
        sqlx::query(
            "UPDATE agent_team_members SET workspace = 'my_files' WHERE name = 'researcher'",
        )
        .execute(&f.pool)
        .await?;
        owner_at_cap(f, &t).await?;
        queue(f, t.id, &t.researcher.0).await?;
        let (status, answer) = read_waiting(f, &t.lead.1, ("r-1:c-1", "r-1"), 20).await?;
        assert_eq!(status, 200, "{answer}");
        assert_eq!(
            answer["hint"],
            json!({"kind": "yield_files", "member": "researcher"}),
            "the lead holds the files it waits for"
        );
        // Another member yields its slot to the same queued task, never the lead's files.
        let (_, member) = read_waiting(f, &t.reviewer.1, ("r-3:c-1", "r-3"), 20).await?;
        assert_eq!(member["hint"], json!({"kind": "yield_slot"}));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_every_tool_route_takes_the_calling_run_and_refuses_without_it(
    pool: PgPool,
) -> TestResult {
    // The S1 contract's Headers: missing or malformed, 400 naming X-TQA-Run-ID; not the current run, 409.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let routes = [
            ("/runtime/teams/messages", json!({"type": "question", "text": "x"})),
            ("/runtime/teams/members", json!({"role": "r", "brief": {"objective": "o", "output": "p", "boundaries": "b"}})),
        ];
        for (path, body) in &routes {
            for (run, status) in [(None, 400), (Some("not a run id!"), 400), (Some("r-0"), 409)] {
                let mut request = f
                    .client
                    .post(f.url(path))
                    .bearer_auth(&t.lead.1)
                    .header("idempotency-key", "r-1:c-1");
                if let Some(run) = run {
                    request = request.header("x-tqa-run-id", run);
                }
                let response = request.json(body).send().await?;
                assert_eq!(response.status().as_u16(), status, "{path} {run:?}");
                let refused: Value = response.json().await?;
                if status == 400 {
                    assert_eq!(refused["error"]["details"][0]["field"], "X-TQA-Run-ID", "{path} {run:?}");
                } else {
                    assert_eq!(refused["error"]["code"], "not_current_run", "{path}");
                }
            }
        }
        let (status, refused) = read(f, &t.lead.1, ("r-0:c-1", "r-0")).await?;
        assert_eq!((status, &refused["error"]["code"]), (409, &json!("not_current_run")));
        // Nothing was stored by any of them.
        let stored: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id WHERE t.id = $1",
        )
        .bind(t.id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(stored, 0);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_message_wakes_a_reader_whose_run_isnt_active_once(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // Both members are past their first turn; the reviewer's run has ended, the researcher's goes on.
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        let (status, _) = send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "Ready?"}),
        )
        .await?;
        assert_eq!(status, 201);
        let reviewer = Uuid::parse_str(t.reviewer.0.as_str())?;
        assert_eq!(
            wakes_settle(f, &t.reviewer.0, 1).await?,
            [format!("team-wake:{reviewer}:0")]
        );
        // Several messages before it reads make one wake.
        send(
            f,
            &t.lead.1,
            "r-1:c-2",
            &json!({"type": "decision", "text": "Go"}),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(wakes(f, &t.reviewer.0).await?.len(), 1);
        // An active run isn't woken: it reads with its own tools.
        assert!(wakes(f, &t.researcher.0).await?.is_empty());
        // The lead is woken by what is for it: a member's report, once its run has ended.
        run_ends(f, &t.lead.0, 2, "r-1").await?;
        let (status, _) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "Done."}),
        )
        .await?;
        assert_eq!(status, 201);
        let lead = Uuid::parse_str(t.lead.0.as_str())?;
        let woken = wakes_settle(f, &t.lead.0, 1).await?;
        assert_eq!(woken, [format!("team-wake:{lead}:0")]);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_decision_on_a_members_ask_wakes_the_asker_and_a_report_is_no_question(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let (status, asked) = send(
            f,
            &t.reviewer.1,
            "r-3:c-1",
            &json!({"type": "ask", "text": "Round half up or half even?"}),
        )
        .await?;
        assert_eq!(status, 201);
        // The asker ends its run to wait for its lead, as a helper does after asking.
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        let (status, decided) = send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "decision", "text": "Round half up.", "question_seq": asked["seq"]}),
        )
        .await?;
        assert_eq!(status, 201, "{decided}");
        let reviewer = Uuid::parse_str(t.reviewer.0.as_str())?;
        assert_eq!(
            wakes_settle(f, &t.reviewer.0, 1).await?,
            [format!("team-wake:{reviewer}:0")]
        );
        let (status, reported) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "Done."}),
        )
        .await?;
        assert_eq!(status, 201);
        let (status, refused) = send(
            f,
            &t.lead.1,
            "r-1:c-2",
            &json!({"type": "decision", "text": "Accepted.", "question_seq": reported["seq"]}),
        )
        .await?;
        assert_eq!(
            (
                status,
                &refused["error"]["code"],
                &refused["error"]["details"][0]["field"]
            ),
            (422, &json!("unknown_reference"), &json!("question_seq")),
            "{refused}"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_stopped_member_is_never_woken(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        run_ends(f, &t.reviewer.0, 2, "r-3").await?;
        // A stopped member isn't woken. On purpose, as the stop leaves it (the stop itself is GW-I-111's).
        sqlx::query("UPDATE agent_team_members SET state = 'stopped' WHERE name = 'reviewer'")
            .execute(&f.pool)
            .await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "Ready?"}),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(wakes(f, &t.reviewer.0).await?.is_empty());
        Ok(())
    })
    .await
}
