//! GW-I-129: a message sent while the task's run is in progress waits on the gateway ("send after this") and starts
//! the next run when that run ends; every message held meanwhile goes out as one user turn, in order. Before, the
//! message went to the runtime at once, which refused it as busy, and it was lost while its record said delivered.
//! A runtime that still refuses the turn as busy gets it again once it can take it.
//! GW-I-130: a run that fails or is stopped drops the messages held behind it; they stay listed, and clients are told.
//! GW-I-131: a held message can be cancelled by its author or the task's owner until it goes out.
#[path = "support/fixture.rs"]
mod support;

use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use support::{Fixture, TestResult};
use tqa_types::{ClientFrame, ErrorCode, Event, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Long enough for the gateway to have delivered an action it was going to deliver.
const QUIET: Duration = Duration::from_secs(1);

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

async fn send(f: &Fixture, cookie: &str, id: &str, key: &str, text: &str) -> Result<Value> {
    let response = f
        .action(
            &format!("/api/v1/conversations/{id}/messages"),
            cookie,
            Some(key),
            &json!({"text": text}),
        )
        .await?;
    assert_eq!(response.status(), 202, "{key}");
    Ok(response.json().await?)
}

async fn queued(f: &Fixture, cookie: &str, id: &str) -> Result<Vec<(String, String)>> {
    let response = f
        .get(
            &format!("/api/v1/conversations/{id}/messages/queued"),
            cookie,
        )
        .await?;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await?;
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .map(|item| {
            (
                item["text"].as_str().unwrap_or_default().to_owned(),
                item["state"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect())
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(text, state)| ((*text).to_owned(), (*state).to_owned()))
        .collect()
}

// TEST: GW-I-129
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_129_messages_during_a_run_wait_and_go_out_as_one_turn_when_it_ends(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let created = f.conversation(owner.id).await?;
        let id = created.id.to_string();
        let mut runtime = f.runtime(&created).await?;
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "run-1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&created.id, 1).await?;

        // During the run: accepted and held, never handed to the runtime (which would refuse it as busy).
        let first = send(f, &cookie, &id, "held-1", "also check the tests").await?;
        assert_eq!(first["held"], true);
        assert!(first["message_id"].is_string(), "{first}");
        assert!(
            runtime.try_action(QUIET).await?.is_none(),
            "a message during a run must wait"
        );
        send(f, &cookie, &id, "held-2", "and update the changelog").await?;
        assert_eq!(
            queued(f, &cookie, &id).await?,
            pairs(&[
                ("also check the tests", "held"),
                ("and update the changelog", "held")
            ])
        );

        // The run ends: both go out as one user turn, in order, and are listed as sent.
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "run-1".parse()?,
                },
            ))
            .await?;
        let ClientFrame::UserMessage { text, .. } = runtime.action().await? else {
            return Err("expected the held messages as one user turn".into());
        };
        assert_eq!(text, "also check the tests\n\nand update the changelog");
        assert!(
            runtime.try_action(QUIET).await?.is_none(),
            "one turn, not two"
        );
        assert_eq!(
            queued(f, &cookie, &id).await?,
            pairs(&[
                ("also check the tests", "sent"),
                ("and update the changelog", "sent")
            ])
        );
        Ok(())
    })
    .await
}

/// A new task of `owner`'s with its runtime and a run in progress (`run-1`, sequence 1).
async fn running(
    f: &Fixture,
    owner: &tqa_gateway::User,
) -> Result<(String, support::runtime::Runtime)> {
    let created = f.conversation(owner.id).await?;
    let mut runtime = f.runtime(&created).await?;
    runtime
        .send(&lifecycle(
            1,
            Event::RunStarted {
                run_id: "run-1".parse()?,
            },
        ))
        .await?;
    f.until_sequence(&created.id, 1).await?;
    Ok((created.id.to_string(), runtime))
}

async fn user_turn(runtime: &mut support::runtime::Runtime) -> Result<String> {
    match runtime.action().await? {
        ClientFrame::UserMessage { text, .. } => Ok(text),
        other => Err(format!("expected a user turn, got {other:?}").into()),
    }
}

// TEST: GW-I-129
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_129_a_cancel_still_reaches_the_run_while_messages_wait(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (id, mut runtime) = running(f, &owner).await?;
        send(f, &cookie, &id, "held", "after that, the docs").await?;
        let cancel = f
            .action(
                &format!("/api/v1/conversations/{id}/runs/run-1/cancel"),
                &cookie,
                Some("stop"),
                &json!({}),
            )
            .await?;
        assert_eq!(cancel.status(), 202);
        assert!(
            matches!(runtime.action().await?, ClientFrame::RunCancel { .. }),
            "the cancel isn't held"
        );
        assert!(
            runtime.try_action(QUIET).await?.is_none(),
            "the message still waits"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-129
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_129_a_turn_the_runtime_refuses_as_busy_is_sent_again(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (id, mut runtime) = running(f, &owner).await?;
        send(f, &cookie, &id, "held", "then run the linter").await?;
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "run-1".parse()?,
                },
            ))
            .await?;
        assert_eq!(user_turn(&mut runtime).await?, "then run the linter");
        // The runtime's run task hasn't finished yet: it refuses the turn, as a real one does in that window.
        runtime
            .send(&ServerFrame::Error {
                seq: None,
                code: ErrorCode::Busy,
                message: "a run is already active".into(),
                retryable: true,
                target: None,
            })
            .await?;
        // Put back, then sent again after a short backoff: never lost.
        assert_eq!(user_turn(&mut runtime).await?, "then run the linter");
        assert_eq!(
            queued(f, &cookie, &id).await?,
            pairs(&[("then run the linter", "sent")])
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-130
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_130_a_failed_or_stopped_run_drops_what_waited_behind_it(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        for (end, reason) in [
            (
                Event::RunFailed {
                    run_id: "run-1".parse()?,
                    code: ErrorCode::Internal,
                    reason: "boom".into(),
                    provider_message: None,
                },
                "run_failed",
            ),
            (
                Event::RunCancelled {
                    run_id: "run-1".parse()?,
                },
                "run_cancelled",
            ),
        ] {
            let (id, mut runtime) = running(f, &owner).await?;
            let conversation: tqa_types::ConversationId = id.parse()?;
            let mut stream = f.stream(&conversation, &cookie, 0).await?;
            send(f, &cookie, &id, &format!("held-{reason}"), "follow-up").await?;
            runtime.send(&lifecycle(2, end.clone())).await?;
            // Nothing goes out; the message is listed as dropped, with the reason, and clients are told.
            assert!(runtime.try_action(QUIET).await?.is_none(), "{reason}");
            let body: Value = f
                .get(
                    &format!("/api/v1/conversations/{id}/messages/queued"),
                    &cookie,
                )
                .await?
                .json()
                .await?;
            assert_eq!(body["items"][0]["state"], "dropped");
            assert_eq!(body["items"][0]["reason"], reason);
            let dropped = body["items"][0]["id"].as_str().ok_or("id")?.to_owned();
            loop {
                if let (
                    None,
                    ServerFrame::MessageQueue {
                        held,
                        dropped: gone,
                        ..
                    },
                ) = stream.next().await?
                    && held.is_empty()
                    && gone == [dropped.clone()]
                {
                    break;
                }
            }
            // A message sent after the run's end is a new turn, not dropped.
            send(f, &cookie, &id, &format!("after-{reason}"), "start over").await?;
            assert_eq!(user_turn(&mut runtime).await?, "start over");
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-131
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_131_a_held_message_is_cancelled_until_it_goes_out(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (id, mut runtime) = running(f, &owner).await?;
        let kept = send(f, &cookie, &id, "kept", "keep this one").await?;
        let dropped = send(f, &cookie, &id, "dropped", "never mind this").await?;
        let remove = |message: &Value| {
            let path = format!(
                "/api/v1/conversations/{id}/messages/queued/{}",
                message["message_id"].as_str().unwrap_or_default()
            );
            f.client
                .delete(f.url(&path))
                .header("origin", f.origin())
                .header("cookie", &cookie)
                .send()
        };
        // Someone without access to the task is refused.
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        let path = format!(
            "/api/v1/conversations/{id}/messages/queued/{}",
            dropped["message_id"].as_str().unwrap_or_default()
        );
        let refused = f
            .client
            .delete(f.url(&path))
            .header("origin", f.origin())
            .header("cookie", &stranger)
            .send()
            .await?;
        assert_eq!(refused.status(), 403);
        // Its author cancels it: it never goes out, and replaying its key says so.
        assert_eq!(remove(&dropped).await?.status(), 204);
        let replay = f
            .action(
                &format!("/api/v1/conversations/{id}/messages"),
                &cookie,
                Some("dropped"),
                &json!({"text": "never mind this"}),
            )
            .await?;
        assert_eq!(
            replay
                .headers()
                .get("x-tqa-execution-status")
                .ok_or("status")?,
            "cancelled"
        );
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "run-1".parse()?,
                },
            ))
            .await?;
        assert_eq!(user_turn(&mut runtime).await?, "keep this one");
        // Once sent it can't be cancelled; an unknown id isn't found.
        assert_eq!(remove(&kept).await?.status(), 409);
        assert_eq!(
            remove(&json!({"message_id": "999999"})).await?.status(),
            404
        );
        assert_eq!(
            queued(f, &cookie, &id).await?,
            pairs(&[("keep this one", "sent")])
        );
        Ok(())
    })
    .await
}

/// One attempt at `io_timeout` 2 s: a turn nobody confirmed goes out again after 6 s of a ready link.
const RESTART_CONFIG: &str = "[limits]\nio_timeout=2\n";

/// Sends `text` as a turn held behind `run-1`, which then ends; returns once the runtime has the turn.
async fn turn_after_run(
    f: &Fixture,
    cookie: &str,
    id: &str,
    runtime: &mut support::runtime::Runtime,
    text: &str,
) -> TestResult {
    send(f, cookie, id, "restart-held", text).await?;
    runtime
        .send(&lifecycle(
            2,
            Event::RunCompleted {
                run_id: "run-1".parse()?,
            },
        ))
        .await?;
    assert_eq!(user_turn(runtime).await?, text);
    // Recorded as sent before anything else happens, so a restart meets a delivered row, not a queued one.
    tokio::time::timeout(support::DEADLINE, async {
        while queued(f, cookie, id).await? != pairs(&[(text, "sent")]) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await??;
    Ok(())
}

// TEST: GW-I-129
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_129_a_turn_lost_with_a_gateway_restart_goes_out_again(pool: PgPool) -> TestResult {
    support::run_restarting_with(pool, None, RESTART_CONFIG, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (id, mut runtime) = running(f, &owner).await?;
        turn_after_run(f, &cookie, &id, &mut runtime, "still to do").await?;
        // The gateway restarts before the runtime starts a run on it (or its busy refusal is lost with the link).
        drop(runtime);
        f.restart().await?;
        let mut runtime = f.runtime_for(&id.parse()?).await?;
        // Nothing confirmed the turn, so once the runtime is back and its log replayed, it goes out again.
        assert_eq!(user_turn(&mut runtime).await?, "still to do");
        assert_eq!(
            queued(f, &cookie, &id).await?,
            pairs(&[("still to do", "sent")])
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-129
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_129_a_turn_the_runtime_took_is_not_sent_twice_across_a_restart(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, RESTART_CONFIG, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (id, mut runtime) = running(f, &owner).await?;
        turn_after_run(f, &cookie, &id, &mut runtime, "the next step").await?;
        // The runtime started a run on it: that confirms the turn.
        runtime
            .send(&lifecycle(
                3,
                Event::RunStarted {
                    run_id: "run-2".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&id.parse()?, 3).await?;
        drop(runtime);
        f.restart().await?;
        let mut runtime = f.runtime_for(&id.parse()?).await?;
        assert!(
            runtime.try_action(Duration::from_secs(9)).await?.is_none(),
            "past the 6 s bound, nothing is sent again"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-131
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_131_a_message_being_sent_is_no_longer_cancelled(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (id, _runtime) = running(f, &owner).await?;
        let message = send(f, &cookie, &id, "claimed", "on its way").await?;
        // The worker has claimed it for a turn (in the same transaction that decides it goes out), not sent it yet.
        sqlx::query("UPDATE idempotency SET execution_status='sending' WHERE key='claimed'")
            .execute(&f.pool)
            .await?;
        let path = format!(
            "/api/v1/conversations/{id}/messages/queued/{}",
            message["message_id"].as_str().ok_or("id")?
        );
        let cancel = f
            .client
            .delete(f.url(&path))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(
            cancel.status(),
            409,
            "the agent gets it: never told 'cancelled'"
        );
        Ok(())
    })
    .await
}
