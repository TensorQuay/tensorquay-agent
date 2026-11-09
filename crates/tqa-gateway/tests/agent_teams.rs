//! Agent teams from a run, slice 1 (ADR-0034, the slice S1 contract): `/runtime/teams/messages`.
//! - GW-I-109: delivery: a message reaches its addressee only; a reader gets what is for it after its last-read seq;
//!   a send is stored with its seq or refused with its reason; a retried send answers the first seq; a repeated read
//!   answers the same; agent teams are off unless the install switch is on.
//! - GW-I-195: a read is delivered exactly once at the crash point: its `tool.result` and the seq's advance commit
//!   together in the batch that stores the result, or neither does.
//! - SEC-038: a member can't read or address another member, and the author is the run's conversation.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use support::TestResult;
use uuid::Uuid;

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_agent_teams_are_off_unless_the_install_switch_is_on(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, _) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        let token = f
            .backend
            .token(&conversation.id)
            .ok_or("token")?
            .expose_secret()
            .to_owned();
        let (status, refused) =
            send(f, &token, "r-1:c-1", &json!({"type": "ask", "text": "hi"})).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (403, &json!("teams_disabled"))
        );
        let (status, refused) = read(f, &token, ("r-1:c-2", "r-1")).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (403, &json!("teams_disabled"))
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_each_message_reaches_its_addressees_only(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let (status, assigned) = send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "assign", "to": "Researcher", "text": "North region."}),
        )
        .await?;
        assert_eq!((status, &assigned["seq"]), (201, &json!(1)), "{assigned}");
        let (status, asked) = send(
            f,
            &t.lead.1,
            "r-1:c-2",
            &json!({"type": "question", "text": "Which format?"}),
        )
        .await?;
        assert_eq!(
            (status, &asked["seq"], &asked["to"]),
            (201, &json!(2), &json!(["researcher", "reviewer"])),
            "{asked}"
        );
        let (_, researcher) = read(f, &t.researcher.1, ("r-2:c-1", "r-2")).await?;
        assert_eq!(seqs(&researcher), [1, 2], "{researcher}");
        assert_eq!(researcher["messages"][0]["from"], json!({"kind": "lead"}));
        assert_eq!(
            researcher["messages"][0]["to"],
            json!({"kind": "member", "name": "researcher"})
        );
        let (_, reviewer) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        assert_eq!(
            seqs(&reviewer),
            [2],
            "the reviewer doesn't get the researcher's assignment"
        );
        let (_, lead) = read(f, &t.lead.1, ("r-1:c-3", "r-1")).await?;
        assert_eq!(
            seqs(&lead),
            Vec::<u64>::new(),
            "the lead doesn't get its own messages"
        );
        let (status, answered) = send(
            f,
            &t.researcher.1,
            "r-2:c-2",
            &json!({"type": "answer", "question_seq": 2, "text": "CSV."}),
        )
        .await?;
        assert_eq!((status, &answered["seq"]), (201, &json!(3)), "{answered}");
        let (_, lead) = read(f, &t.lead.1, ("r-1:c-4", "r-1")).await?;
        assert_eq!(seqs(&lead), [3]);
        assert_eq!(
            lead["messages"][0]["from"],
            json!({"kind": "member", "name": "researcher"})
        );
        assert_eq!(
            (
                &lead["messages"][0]["type"],
                &lead["messages"][0]["question_seq"]
            ),
            (&json!("answer"), &json!(2))
        );
        let (_, reviewer) = read(f, &t.reviewer.1, ("r-3:c-2", "r-3")).await?;
        assert_eq!(
            seqs(&reviewer),
            Vec::<u64>::new(),
            "a new read starts after the one still outstanding"
        );
        let (_, repeat) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        assert_eq!(
            seqs(&repeat),
            [2],
            "the first read's key answers its messages again"
        );
        // A team's log is never listed among channels (ADR-0034 point 12).
        let (status, listed) =
            support::call(f, Method::GET, "/api/v1/channels", &t.cookie, (None, None)).await?;
        assert_eq!((status, &listed["items"]), (200, &json!([])), "{listed}");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_send_is_stored_or_refused_with_its_reason(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let refusals = [
            (&t.researcher.1, json!({"type": "assign", "to": "reviewer", "text": "x"}), 403, "not_lead"),
            (&t.lead.1, json!({"type": "ask", "text": "x"}), 403, "members_only"),
            (&t.outsider.1, json!({"type": "ask", "text": "x"}), 403, "not_a_member"),
            (&t.lead.1, json!({"type": "assign", "to": "nobody", "text": "x"}), 404, "unknown_member"),
            (&t.lead.1, json!({"type": "notice", "kind": "budget_80", "text": "x"}), 400, "invalid_request"),
            (&t.researcher.1, json!({"type": "answer", "question_seq": 99, "text": "x"}), 422, "unknown_reference"),
        ];
        for (n, (token, body, status, code)) in refusals.into_iter().enumerate() {
            let (got, refused) = send(f, token, &format!("r-9:c-{n}"), &body).await?;
            assert_eq!((got, &refused["error"]["code"]), (status, &json!(code)), "{body}");
        }
        // On purpose, as the stop leaves a helper (the stop itself is GW-I-111's): a send to it is refused.
        sqlx::query("UPDATE agent_team_members SET state = 'stopped' WHERE name = 'reviewer'").execute(&f.pool).await?;
        let (status, refused) = send(f, &t.lead.1, "r-9:c-20", &json!({"type": "assign", "to": "reviewer", "text": "x"})).await?;
        assert_eq!((status, &refused["error"]["code"]), (409, &json!("member_gone")));
        let missing_key = f
            .client
            .post(f.url("/runtime/teams/messages"))
            .bearer_auth(&t.lead.1)
            .header("x-tqa-run-id", "r-1")
            .json(&json!({"type": "question", "text": "x"}))
            .send()
            .await?;
        assert_eq!(missing_key.status().as_u16(), 400);
        // Nothing refused was stored.
        let stored: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id WHERE t.id = $1",
        )
        .bind(t.id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(stored, 0);
        sqlx::query("UPDATE agent_teams SET state = 'stopping' WHERE id = $1").bind(t.id).execute(&f.pool).await?;
        let (status, refused) = send(f, &t.lead.1, "r-9:c-21", &json!({"type": "question", "text": "x"})).await?;
        assert_eq!((status, &refused["error"]["code"]), (409, &json!("team_stopped")));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_retried_send_and_a_repeated_read_answer_the_same(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let body = json!({"type": "question", "text": "Once."});
        let (_, first) = send(f, &t.lead.1, "r-1:c-1", &body).await?;
        let (_, again) = send(f, &t.lead.1, "r-1:c-1", &body).await?;
        assert_eq!(first, again, "the retry answers the first seq");
        let (_, read_first) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-2",
            &json!({"type": "question", "text": "Twice."}),
        )
        .await?;
        let (_, read_again) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        assert_eq!(seqs(&read_first), [1]);
        assert_eq!(
            read_first["messages"], read_again["messages"],
            "the repeat answers the same, newer or not"
        );
        for (key, run) in [("", "r-3"), ("r-3:c-9", "")] {
            let response = f
                .client
                .get(f.url("/runtime/teams/messages"))
                .bearer_auth(&t.reviewer.1)
                .header("idempotency-key", key)
                .header("x-tqa-run-id", run)
                .send()
                .await?;
            assert_eq!(response.status().as_u16(), 400, "{key:?} {run:?}");
        }
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_195_a_read_is_delivered_exactly_once_at_the_crash_point(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "Which format?"}),
        )
        .await?;
        let reviewer = Uuid::parse_str(t.reviewer.0.as_str())?;
        let (_, first) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        assert_eq!(seqs(&first), [1]);

        // Before the commit: the batch that stores the read's result stops at the conversation's lock and dies.
        let mut holder = f.pool.begin().await?;
        sqlx::query("SELECT 1 FROM conversations WHERE id=$1 FOR UPDATE")
            .bind(reviewer)
            .execute(&mut *holder)
            .await?;
        let events = f.gateway.events().clone();
        let (id, batch) = (
            t.reviewer.0.clone(),
            vec![support::frame(2, "thinking"), result(3, "r-3", "c-1")?],
        );
        let in_flight = tokio::spawn(async move { events.append_batch(&id, batch).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !in_flight.is_finished(),
            "the batch is inside its transaction"
        );
        in_flight.abort();
        assert!(in_flight.await.is_err_and(|error| error.is_cancelled()));
        holder.rollback().await?;
        let (seq, recorded): (i64, i64) = sqlx::query_as(
            "SELECT m.last_read_seq, (SELECT count(*) FROM agent_team_reads WHERE reader = $1) \
             FROM agent_team_members m WHERE m.conversation_id = $1",
        )
        .bind(reviewer)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            (seq, recorded),
            (0, 1),
            "neither the result nor the advance is stored"
        );
        let (_, again) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        assert_eq!(seqs(&again), [1], "the read comes again");

        // After the commit: the batch with the result stores both, and nothing old comes again.
        f.gateway
            .events()
            .append_batch(
                &t.reviewer.0,
                vec![support::frame(2, "thinking"), result(3, "r-3", "c-1")?],
            )
            .await?;
        let (seq, recorded): (i64, i64) = sqlx::query_as(
            "SELECT m.last_read_seq, (SELECT count(*) FROM agent_team_reads WHERE reader = $1) \
             FROM agent_team_members m WHERE m.conversation_id = $1",
        )
        .bind(reviewer)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            (seq, recorded),
            (1, 0),
            "the seq advanced and the recorded answer was pruned"
        );
        let (_, later) = read(f, &t.reviewer.1, ("r-3:c-2", "r-3")).await?;
        assert_eq!(seqs(&later), Vec::<u64>::new(), "no later read returns it");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_038_a_member_cant_read_or_address_another_member(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "assign", "to": "researcher", "text": "Private brief."}),
        )
        .await?;
        let (_, reviewer) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        assert_eq!(
            seqs(&reviewer),
            Vec::<u64>::new(),
            "another member's assignment never reaches it"
        );
        // A member has no way to address another member: an addressee in its post is refused.
        let (status, refused) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "ask", "to": "reviewer", "text": "psst"}),
        )
        .await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (400, &json!("invalid_request"))
        );
        // The author is the run's conversation, whatever a body could say.
        let (status, _) = send(
            f,
            &t.researcher.1,
            "r-2:c-2",
            &json!({"type": "ask", "text": "From me."}),
        )
        .await?;
        assert_eq!(status, 201);
        let (_, lead) = read(f, &t.lead.1, ("r-1:c-2", "r-1")).await?;
        assert_eq!(
            lead["messages"][0]["from"],
            json!({"kind": "member", "name": "researcher"})
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_038_a_member_addressee_is_only_ever_a_member_row(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let channel: Uuid = sqlx::query_scalar("SELECT channel_id FROM agent_teams WHERE id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        let researcher = Uuid::parse_str(t.researcher.0.as_str())?;
        // A member is named only by `to_kind = 'member'`: the store refuses a member on any other kind, and a member
        // row without one, so a person's or the lead's message can never carry a member's conversation.
        for (seq, (kind, to)) in (1_i64..).zip([
            (Some("person"), Some(researcher)),
            (Some("lead"), Some(researcher)),
            (Some("members"), Some(researcher)),
            (Some("team"), Some(researcher)),
            (None, Some(researcher)),
            (Some("member"), None),
        ]) {
            let refused = sqlx::query(
                "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, \
                 to_conversation, body) VALUES ($1, $2, 'conversation', $3, 'lead', 'question', $4, $5, '{}')",
            )
            .bind(channel)
            .bind(seq)
            .bind(Uuid::parse_str(t.lead.0.as_str())?)
            .bind(kind)
            .bind(to)
            .execute(&f.pool)
            .await
            .err()
            .and_then(sqlx::Error::into_database_error)
            .and_then(|error| error.code().map(std::borrow::Cow::into_owned));
            assert_eq!(refused.as_deref(), Some("23514"), "{kind:?} {to:?}");
        }
        let (_, answer) = read(f, &t.researcher.1, ("r-2:c-1", "r-2")).await?;
        assert_eq!(seqs(&answer), Vec::<u64>::new());
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_195_a_second_read_before_the_first_result_never_answers_the_same(
    pool: PgPool,
) -> TestResult {
    // Two reads with different keys, neither result stored yet.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "One?"}),
        )
        .await?;
        let (_, first) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-2",
            &json!({"type": "question", "text": "Two?"}),
        )
        .await?;
        let (_, second) = read(f, &t.reviewer.1, ("r-3:c-2", "r-3")).await?;
        assert_eq!(
            (seqs(&first), seqs(&second)),
            (vec![1], vec![2]),
            "disjoint, in order"
        );
        let (_, third) = read(f, &t.reviewer.1, ("r-3:c-3", "r-3")).await?;
        assert_eq!(seqs(&third), Vec::<u64>::new());
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_reads_run_and_key_are_checked(pool: PgPool) -> TestResult {
    // The run header is a run id and the conversation's current run; the key is a bounded value.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let long_run = "r".repeat(65);
        let (status, refused) = read(f, &t.reviewer.1, ("r-3:c-1", &long_run)).await?;
        assert_eq!(
            (status, &refused["error"]["details"][0]["field"]),
            (400, &json!("X-TQA-Run-ID")),
            "{refused}"
        );
        let (status, refused) = read(f, &t.reviewer.1, ("r-3:c-2", "r-9")).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (409, &json!("not_current_run")),
            "{refused}"
        );
        let long_key = "k".repeat(129);
        let (status, _) = read(f, &t.reviewer.1, (&long_key, "r-3")).await?;
        assert_eq!(status, 400);
        let (status, _) = send(
            f,
            &t.lead.1,
            &long_key,
            &json!({"type": "question", "text": "x"}),
        )
        .await?;
        assert_eq!(status, 400);
        let recorded: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_team_reads")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(recorded, 0, "a refused read records nothing");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_two_reads_with_one_key_at_once_answer_the_same(pool: PgPool) -> TestResult {
    // Neither is a 500.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "Once?"}),
        )
        .await?;
        let (a, b) = tokio::join!(
            read(f, &t.reviewer.1, ("r-3:c-1", "r-3")),
            read(f, &t.reviewer.1, ("r-3:c-1", "r-3"))
        );
        let ((status_a, a), (status_b, b)) = (a?, b?);
        assert_eq!((status_a, status_b), (200, 200), "{a} {b}");
        assert_eq!(a, b);
        assert_eq!(seqs(&a), [1]);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_a_malformed_post_names_its_field(pool: PgPool) -> TestResult {
    // The contract's invalid_request with details naming the field.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        for (n, (body, field)) in [
            (
                json!({"type": "question", "text": "x", "tone": "loud"}),
                "tone",
            ),
            (json!({"type": "answer", "text": "x"}), "question_seq"),
            (json!({"type": "assign", "text": "x"}), "to"),
        ]
        .into_iter()
        .enumerate()
        {
            let (status, refused) = send(f, &t.lead.1, &format!("r-1:c-{n}"), &body).await?;
            assert_eq!(
                (
                    status,
                    &refused["error"]["code"],
                    &refused["error"]["details"][0]["field"]
                ),
                (400, &json!("invalid_request"), &json!(field)),
                "{body}"
            );
        }
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_an_empty_answer_never_reports_a_seq_below_an_earlier_one(
    pool: PgPool,
) -> TestResult {
    // With an answer outstanding, a new read is empty and its last_seq is where it started, not behind.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "One?"}),
        )
        .await?;
        let (_, first) = read(f, &t.reviewer.1, ("r-3:c-1", "r-3")).await?;
        let (_, empty) = read(f, &t.reviewer.1, ("r-3:c-2", "r-3")).await?;
        assert_eq!(
            (&first["last_seq"], seqs(&empty)),
            (&json!(1), Vec::<u64>::new())
        );
        assert_eq!(empty["last_seq"], json!(1), "{empty}");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_195_a_result_stored_right_after_its_answer_finds_its_read(
    pool: PgPool,
) -> TestResult {
    // The ordering behind exactly-once: a read is indexed before its answer leaves, so its tool.result, stored the
    // moment the answer arrives, always advances the seq (ADR-0034, the S1 contract).
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "question", "text": "Now?"}),
        )
        .await?;
        for round in 0..20_u64 {
            let key = format!("r-3:c-{round}");
            read(f, &t.reviewer.1, (&key, "r-3")).await?;
            let call = format!("c-{round}");
            f.gateway
                .events()
                .append(&t.reviewer.0, result(2 + round, "r-3", &call)?)
                .await?;
        }
        let (seq, recorded): (i64, i64) = sqlx::query_as(
            "SELECT m.last_read_seq, (SELECT count(*) FROM agent_team_reads WHERE reader = $1) \
             FROM agent_team_members m WHERE m.conversation_id = $1",
        )
        .bind(Uuid::parse_str(t.reviewer.0.as_str())?)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!((seq, recorded), (1, 0), "every result found its read");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_109_reads_left_from_an_ended_run_are_pruned_at_the_next_start(
    pool: PgPool,
) -> TestResult {
    // A read recorded for a run that ended while it couldn't be pruned (agent teams switched off, say) goes at the next
    // start with agent teams on; a read of the current run stays.
    support::run_restarting_with(pool, None, ENABLED, async |f| {
        let (reviewer, team_id) = {
            let t = team(f).await?;
            (Uuid::parse_str(t.reviewer.0.as_str())?, t.id)
        };
        for (key, run) in [("r-0:c-1", "r-0"), ("r-3:c-1", "r-3")] {
            sqlx::query(
                "INSERT INTO agent_team_reads (reader, call_key, team_id, run_id, seqs) VALUES ($1, $2, $3, $4, '{}')",
            )
            .bind(reviewer)
            .bind(key)
            .bind(team_id)
            .bind(run)
            .execute(&f.pool)
            .await?;
        }
        f.restart().await?;
        let left: Vec<String> =
            sqlx::query_scalar("SELECT call_key FROM agent_team_reads WHERE reader = $1 ORDER BY call_key")
                .bind(reviewer)
                .fetch_all(&f.pool)
                .await?;
        assert_eq!(left, ["r-3:c-1"], "the ended run's read went; the current run's stayed");
        Ok(())
    })
    .await
}
