//! The lead's routes on one helper, slice 4 (ADR-0034, the S1 contract's routes table).
//! - GW-I-197: `read_helper` pages a helper's messages, text, tool calls and results, each event capped by size; the
//!   lead only, its own helpers.
//! - GW-I-111: `stop_member` stops one helper, tells the lead once, and drops its queued wake and its first turn not
//!   yet started; a repeat answers the same, a failed helper is `member_gone` (a done one is stopped: GW-I-212).
//! - GW-I-277: `read_helper`'s answer says where the helper is now: its state and, mid-step, the step, the running
//!   tool, how long, and about how many tokens of thinking and text the step has streamed (slow, not stuck).
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use tqa_types::{Event, ServerFrame, ToolStatus};
use uuid::Uuid;

/// A frame of `run` at `sequence` carrying `event`, as a runtime sends it.
fn frame(sequence: u64, run: &str, event: Event) -> Result<ServerFrame> {
    Ok(ServerFrame::Event {
        seq: sequence.into(),
        run_id: Some(run.parse()?),
        step: Some(1),
        ts: "2026-01-01T00:00:02Z".into(),
        event,
    })
}

/// A frame of `run`'s step 1 at `sequence`, stamped `at`.
fn frame_at(sequence: u64, run: &str, at: OffsetDateTime, event: Event) -> Result<ServerFrame> {
    Ok(ServerFrame::Event {
        seq: sequence.into(),
        run_id: Some(run.parse()?),
        step: Some(1),
        ts: at.format(&Rfc3339)?,
        event,
    })
}

async fn transcript(f: &Fixture, token: &str, name: &str, query: &str) -> Result<(u16, Value)> {
    let mut request = f
        .client
        .get(f.url(&format!("/runtime/teams/members/{name}/transcript{query}")))
        .bearer_auth(token);
    if let Some(run) = run_of(token) {
        request = request.header("x-tqa-run-id", run);
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

async fn stop_member(f: &Fixture, token: &str, key: &str, name: &str) -> Result<(u16, Value)> {
    let mut request = f
        .client
        .post(f.url(&format!("/runtime/teams/members/{name}/stop")))
        .bearer_auth(token)
        .header("idempotency-key", key);
    if let Some(run) = run_of(token) {
        request = request.header("x-tqa-run-id", run);
    }
    let response = request
        .json(&json!({"reason": "Off track."}))
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_197_the_lead_reads_its_helpers_transcript_in_pages(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let events = f.gateway.events();
        for (seq, event) in [
            (
                2,
                Event::MessageUser {
                    text: "Find the Q3 revenue.".into(),
                    attachments: Vec::new(),
                    lead_brief: None,
                    team_wake: None,
                },
            ),
            (
                3,
                Event::TextDelta {
                    text: "Look".into(),
                },
            ),
            (
                4,
                Event::TextDelta {
                    text: "ing.".into(),
                },
            ),
            (
                5,
                Event::ToolCall {
                    call_id: "c-1".parse()?,
                    name: "read_file".into(),
                    arguments: json!({"path": "q3.csv"}),
                },
            ),
            (
                6,
                Event::ToolResult {
                    call_id: "c-1".parse()?,
                    status: ToolStatus::Ok,
                    content: "region,revenue".into(),
                    target: None,
                    evidence: None,
                },
            ),
        ] {
            events
                .append(&t.researcher.0, frame(seq, "r-2", event)?)
                .await?;
        }
        let (status, page) = transcript(f, &t.lead.1, "researcher", "").await?;
        assert_eq!(status, 200, "{page}");
        let kinds: Vec<&str> = page["events"]
            .as_array()
            .ok_or("events")?
            .iter()
            .filter_map(|e| e["type"].as_str())
            .collect();
        assert_eq!(
            kinds,
            ["message", "text", "tool_call", "tool_result"],
            "run.started is left out"
        );
        assert_eq!(
            page["events"][1],
            json!({"seq": 3, "type": "text", "text": "Looking."}),
            "deltas make one event"
        );
        assert_eq!(page["events"][2]["arguments"], json!({"path": "q3.csv"}));
        assert_eq!(
            (&page["last_seq"], &page["more"]),
            (&json!(6), &json!(false))
        );
        // Pages: two events, then the rest from where they stopped.
        let (_, first) = transcript(f, &t.lead.1, "researcher", "?limit=2").await?;
        assert_eq!(
            (first["events"].as_array().map(Vec::len), &first["more"]),
            (Some(2), &json!(true))
        );
        assert_eq!(first["last_seq"], 4, "the merged text ends at 4");
        let (_, rest) = transcript(f, &t.lead.1, "researcher", "?since=4").await?;
        let seqs: Vec<u64> = rest["events"]
            .as_array()
            .ok_or("events")?
            .iter()
            .filter_map(|e| e["seq"].as_u64())
            .collect();
        assert_eq!(seqs, [5, 6]);
        // A large tool result reaches the lead cut to 8 KiB, marked, with its full size.
        let large = Event::ToolResult {
            call_id: "c-2".parse()?,
            status: ToolStatus::Ok,
            content: "x".repeat(200_000),
            target: None,
            evidence: None,
        };
        events
            .append(&t.researcher.0, frame(7, "r-2", large)?)
            .await?;
        let (_, capped) = transcript(f, &t.lead.1, "researcher", "?since=6").await?;
        let event = &capped["events"][0];
        assert_eq!(event["content"].as_str().map(str::len), Some(8 * 1024));
        assert_eq!(
            (&event["truncated"], &event["bytes"]),
            (&json!(true), &json!(200_000))
        );
        // A helper with more frames than one look reads (500) still pages through all of them.
        for seq in 8..608 {
            events
                .append(
                    &t.researcher.0,
                    frame(seq, "r-2", Event::TextDelta { text: ".".into() })?,
                )
                .await?;
        }
        let (_, long) = transcript(f, &t.lead.1, "researcher", "?since=7").await?;
        assert_eq!(long["events"][0]["text"].as_str().map(str::len), Some(600));
        assert_eq!(
            (&long["last_seq"], &long["more"]),
            (&json!(607), &json!(false))
        );
        // The lead only, its own helpers only.
        let (status, refused) = transcript(f, &t.reviewer.1, "researcher", "").await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (403, &json!("not_lead"))
        );
        let (status, refused) = transcript(f, &t.lead.1, "nobody", "").await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (404, &json!("unknown_member"))
        );
        let (status, refused) = transcript(f, &t.outsider.1, "researcher", "").await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (403, &json!("not_a_member"))
        );
        let (status, _) = transcript(f, &t.lead.1, "researcher", "?limit=201").await?;
        assert_eq!(status, 400);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_277_read_helper_says_where_the_helper_is_now(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let events = f.gateway.events();
        // Night 2's case: the brief, then a first step that has only thought so far, 222 s in. A page shows only the
        // brief; `now` says the helper is working and how far it has got.
        let began = OffsetDateTime::now_utc() - Duration::seconds(222);
        let thought = "x".repeat(3500);
        for (seq, event) in [
            (
                2,
                Event::MessageUser {
                    text: "Write the tests.".into(),
                    attachments: Vec::new(),
                    lead_brief: None,
                    team_wake: None,
                },
            ),
            (
                3,
                Event::StepStarted {
                    step: 1,
                    attempt: 1,
                },
            ),
            (
                4,
                Event::ReasoningDelta {
                    text: thought.clone(),
                },
            ),
            (5, Event::ReasoningDelta { text: thought }),
            (
                6,
                Event::TextDelta {
                    text: "The first test reads the CSV.".into(),
                },
            ),
        ] {
            events
                .append(&t.researcher.0, frame_at(seq, "r-2", began, event)?)
                .await?;
        }
        let (status, page) = transcript(f, &t.lead.1, "researcher", "").await?;
        assert_eq!(status, 200, "{page}");
        let now = &page["now"];
        assert_eq!(
            (&now["state"], &now["step"], &now["tool"]),
            (&json!("working"), &json!(1), &Value::Null),
            "{page}"
        );
        // 7,000 bytes of thinking and 29 of text, at the runtime's estimate (ceil(bytes / 3.5)).
        assert_eq!(
            (&now["thinking_tokens"], &now["text_tokens"]),
            (&json!(2000), &json!(9)),
            "{page}"
        );
        let seconds = now["seconds"].as_i64().ok_or("seconds")?;
        assert!((222..300).contains(&seconds), "{page}");
        assert!(now["since"].is_string(), "{page}");
        // A tool started and not ended: its name, and the time since it started.
        let call = OffsetDateTime::now_utc() - Duration::seconds(40);
        for (seq, event) in [
            (
                7,
                Event::ToolCall {
                    call_id: "c-9".parse()?,
                    name: "shell".into(),
                    arguments: json!({"command": "pytest"}),
                },
            ),
            (
                8,
                Event::ToolStart {
                    call_id: "c-9".parse()?,
                },
            ),
        ] {
            events
                .append(&t.researcher.0, frame_at(seq, "r-2", call, event)?)
                .await?;
        }
        let (_, page) = transcript(f, &t.lead.1, "researcher", "?since=6").await?;
        assert_eq!(page["now"]["tool"], json!("shell"), "{page}");
        let seconds = page["now"]["seconds"].as_i64().ok_or("seconds")?;
        assert!((40..120).contains(&seconds), "{page}");
        // The tool ends and the step completes: between steps no step's time runs, so the state alone.
        for (seq, event) in [
            (
                9,
                Event::ToolEnd {
                    call_id: "c-9".parse()?,
                    status: ToolStatus::Ok,
                    exit_code: Some(0),
                    failure: None,
                },
            ),
            (
                10,
                Event::StepCompleted {
                    step: 1,
                    duration_ms: 262_000,
                },
            ),
        ] {
            events
                .append(&t.researcher.0, frame_at(seq, "r-2", call, event)?)
                .await?;
        }
        let (_, page) = transcript(f, &t.lead.1, "researcher", "?since=8").await?;
        assert_eq!(page["now"], json!({"state": "working"}), "{page}");
        // A helper waiting for the lead, with no step begun: its state alone.
        sqlx::query("UPDATE agent_team_members SET state = 'waiting' WHERE name = 'reviewer'")
            .execute(&f.pool)
            .await?;
        let (status, page) = transcript(f, &t.lead.1, "reviewer", "").await?;
        assert_eq!(status, 200, "{page}");
        assert_eq!(page["now"], json!({"state": "waiting_lead"}), "{page}");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_111_stop_member_stops_one_helper_tells_the_lead_and_drops_its_wake(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // A wake queued for the reviewer, as a post leaves one for a reader without a sandbox.
        queue(f, t.id, &t.reviewer.0).await?;
        sqlx::query("UPDATE idempotency SET key = 'team-wake:x:0' WHERE conversation_id = $1")
            .bind(Uuid::parse_str(t.reviewer.0.as_str())?)
            .execute(&f.pool)
            .await?;
        // The researcher is still waiting for its first turn.
        queue(f, t.id, &t.researcher.0).await?;
        let start_key = format!("team-start:{}", t.researcher.0.as_str());
        sqlx::query("UPDATE idempotency SET key = $1 WHERE conversation_id = $2")
            .bind(&start_key)
            .bind(Uuid::parse_str(t.researcher.0.as_str())?)
            .execute(&f.pool)
            .await?;
        let (status, refused) = stop_member(f, &t.researcher.1, "r-2:c-1", "reviewer").await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (403, &json!("not_lead"))
        );
        let (status, refused) = stop_member(f, &t.lead.1, "r-1:c-9", "nobody").await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (404, &json!("unknown_member"))
        );
        let (status, answer) = stop_member(f, &t.lead.1, "r-1:c-1", "reviewer").await?;
        assert_eq!(
            (status, &answer),
            (
                200,
                &json!({"member": {"name": "reviewer", "state": "stopped"}})
            )
        );
        let state: String =
            sqlx::query_scalar("SELECT state FROM agent_team_members WHERE name = 'reviewer'")
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(state, "stopped");
        let wake: String = sqlx::query_scalar(
            "SELECT execution_status FROM idempotency WHERE key = 'team-wake:x:0'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(wake, "cancelled", "its queued wake is dropped");
        // The lead is told once; a repeat, with any key, answers the same.
        for key in ["r-1:c-1", "r-1:c-2"] {
            let (status, again) = stop_member(f, &t.lead.1, key, "reviewer").await?;
            assert_eq!((status, &again), (200, &answer), "{key}");
        }
        let (_, read) = read(f, &t.lead.1, ("r-1:c-3", "r-1")).await?;
        let notices: Vec<&Value> = read["messages"]
            .as_array()
            .ok_or("messages")?
            .iter()
            .filter(|m| m["type"] == "notice")
            .collect();
        assert_eq!(notices.len(), 1, "{read}");
        assert_eq!(notices[0]["kind"], "member_stopped");
        assert_eq!(notices[0]["text"], "reviewer was stopped. Its task stays.");
        // A stopped helper's transcript stays readable; a failed one can't be stopped.
        let (status, _) = transcript(f, &t.lead.1, "reviewer", "").await?;
        assert_eq!(status, 200);
        // A helper stopped while it still waited for its first turn never gets it.
        let (status, _) = stop_member(f, &t.lead.1, "r-1:c-5", "researcher").await?;
        assert_eq!(status, 200);
        let first_turn: String =
            sqlx::query_scalar("SELECT execution_status FROM idempotency WHERE key = $1")
                .bind(&start_key)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(first_turn, "cancelled");
        // On purpose: a helper that failed, which only its state tells (how one fails is GW-I-106's).
        sqlx::query("UPDATE agent_team_members SET state = 'failed' WHERE name = 'researcher'")
            .execute(&f.pool)
            .await?;
        let (status, refused) = stop_member(f, &t.lead.1, "r-1:c-4", "researcher").await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (409, &json!("member_gone"))
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_197_the_helper_routes_refuse_a_bad_caller_run_or_ended_team(
    pool: PgPool,
) -> TestResult {
    // The S1 contract's Headers and refusals, on both of the lead's helper routes.
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let routes = [
            ("GET", "/runtime/teams/members/researcher/transcript"),
            ("POST", "/runtime/teams/members/researcher/stop"),
        ];
        for (method, path) in routes {
            let call = |token: Option<&str>, run: Option<&str>| {
                let mut request = match method {
                    "GET" => f.client.get(f.url(path)),
                    _ => f
                        .client
                        .post(f.url(path))
                        .json(&json!({"reason": "Off track."})),
                }
                .header("idempotency-key", "r-1:c-1");
                if let Some(token) = token {
                    request = request.bearer_auth(token);
                }
                if let Some(run) = run {
                    request = request.header("x-tqa-run-id", run);
                }
                request.send()
            };
            assert_eq!(
                call(None, Some("r-1")).await?.status().as_u16(),
                401,
                "{path}: no token"
            );
            let missing: Value = call(Some(&t.lead.1), None).await?.json().await?;
            assert_eq!(
                missing["error"]["details"][0]["field"], "X-TQA-Run-ID",
                "{path}"
            );
            let stale: Value = call(Some(&t.lead.1), Some("r-0")).await?.json().await?;
            assert_eq!(stale["error"]["code"], "not_current_run", "{path}");
        }
        // Once the team has ended, both answer team_ended.
        sqlx::query(
            "UPDATE agent_teams SET state = 'ended', ended_at = CURRENT_TIMESTAMP WHERE id = $1",
        )
        .bind(t.id)
        .execute(&f.pool)
        .await?;
        let (status, ended) = transcript(f, &t.lead.1, "researcher", "").await?;
        assert_eq!(
            (status, &ended["error"]["code"]),
            (409, &json!("team_ended"))
        );
        let (status, ended) = stop_member(f, &t.lead.1, "r-1:c-9", "researcher").await?;
        assert_eq!(
            (status, &ended["error"]["code"]),
            (409, &json!("team_ended"))
        );
        Ok(())
    })
    .await
}
