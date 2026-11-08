//! GW-I-031's turn order (066b; lease P2-2 and the lease-fifo review's P1, 7 Oct 2026): turns on one workspace go in
//! the order they were accepted, through the hand-over and admission alike. Split from `workspace_lease.rs` at the c16
//! merge, which would have passed 1000 lines (G-06); the helpers are that file's. GW-I-263: a restart in the middle of
//! a hand-over keeps the holder's queued turns. GW-I-309: a holder whose turn its runtime took, with its run not yet
//! started, keeps its sandbox until that run ends; the waiter gets the workspace then, and the message is journaled once.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, Event, QueueReason, SandboxState, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

/// A workspace with two tasks bound to it.
async fn shared_workspace(
    f: &Fixture,
    cookie: &str,
) -> Result<(String, ConversationId, ConversationId)> {
    let created: Value = call(f, cookie, Method::POST, "/api/v1/workspaces")
        .json(&json!({"name":"Leased"}))
        .send()
        .await?
        .json()
        .await?;
    let workspace = created["id"].as_str().ok_or("id")?.to_owned();
    let mut tasks = Vec::new();
    for key in ["first", "second"] {
        let task: Value = call(f, cookie, Method::POST, "/api/v1/conversations")
            .header("idempotency-key", key)
            .json(&json!({"title":"Task","model_route":"local","workspace_id":workspace}))
            .send()
            .await?
            .json()
            .await?;
        tasks.push(
            task["id"]
                .as_str()
                .ok_or("task id")?
                .parse::<ConversationId>()?,
        );
    }
    let second = tasks.pop().ok_or("second")?;
    let first = tasks.pop().ok_or("first")?;
    Ok((workspace, first, second))
}

async fn message(
    f: &Fixture,
    cookie: &str,
    id: &ConversationId,
    key: &str,
) -> Result<reqwest::Response> {
    Ok(call(
        f,
        cookie,
        Method::POST,
        &format!("/api/v1/conversations/{id}/messages"),
    )
    .header("idempotency-key", key)
    .json(&json!({"text":"work"}))
    .send()
    .await?)
}

async fn until(f: &Fixture, condition: impl Fn(&Fixture) -> bool) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while !condition(f) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

/// Never two live sandboxes on the workspace.
async fn one_mount(f: &Fixture, a: &ConversationId, b: &ConversationId) -> TestResult {
    let live = f.backend.list_conversations().await?;
    assert!(
        !(live.contains(a) && live.contains(b)),
        "two read-write sandboxes on one workspace"
    );
    Ok(())
}

// TEST: GW-I-248
// Lease P2-2 (7 Oct 2026): a holder whose own later turns keep coming must not starve a task waiting on its workspace.
// A lead keeps receiving its helpers' wakes, and a helper on the lead's files waited as long as the team was active.
// Turns go in the order they were accepted: once the holder's run ends, a waiter that queued before the holder's next
// turn gets the workspace first, and the holder's turn waits its own turn.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_248_a_waiter_that_queued_first_goes_before_the_holders_later_turns(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, holder, waiter) = shared_workspace(f, &cookie).await?;
        // The holder runs.
        assert_eq!(message(f, &cookie, &holder, "h1").await?.status(), 202);
        until(f, |f| f.backend.created(&holder).is_some()).await?;
        let first = f.backend.creates.load(Ordering::SeqCst);
        let mut runtime = f.runtime_for(&holder).await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "h1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&holder, 1).await?;
        // The waiter queues, then the holder gets another turn while its run goes (held until it ends).
        assert_eq!(message(f, &cookie, &waiter, "w1").await?.status(), 202);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(message(f, &cookie, &holder, "h2").await?.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        assert!(
            f.backend.created(&waiter).is_none(),
            "the holder's run is still going"
        );
        // The holder's run ends: its later turn must not go out ahead of the waiter's earlier one.
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "h1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&holder, 2).await?;
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(3) {
            f.gateway.sandboxes().admission_pass_now().await?;
            // Anything else (another action, or the link ending as the holder is retired for the waiter) is fine.
            if let Ok(Some(tqa_types::ClientFrame::UserMessage { .. })) = runtime
                .try_action(std::time::Duration::from_millis(100))
                .await
            {
                return Err(
                    "the holder's later turn went out before the waiter, which queued first".into(),
                );
            }
            if f.backend.created(&waiter).is_some() {
                break;
            }
        }
        until(f, |f| f.backend.created(&waiter).is_some()).await?;
        one_mount(f, &holder, &waiter).await?;
        // The waiter's is the only sandbox made since the holder's: the holder's worker, its later turn queued, isn't
        // let back in ahead of the waiter once the hand-over retired it (GW-I-251's loop, without a restart).
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            first + 1,
            "the holder was made again before the waiter, which queued first"
        );
        let mut second = f.runtime_for(&waiter).await?;
        assert!(matches!(
            second.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        Ok(())
    })
    .await
}

/// A workspace of `owner_cookie`'s person, shared for writing with `member`.
async fn shared_for_writing(f: &Fixture, owner_cookie: &str, member: uuid::Uuid) -> Result<String> {
    let created: Value = call(f, owner_cookie, Method::POST, "/api/v1/workspaces")
        .json(&json!({"name": "Shared for writing"}))
        .send()
        .await?
        .json()
        .await?;
    let workspace = created["id"].as_str().ok_or("id")?.to_owned();
    let shared = call(
        f,
        owner_cookie,
        Method::PUT,
        &format!("/api/v1/workspaces/{workspace}/shares/{member}"),
    )
    .json(&json!({"access": "write"}))
    .send()
    .await?;
    assert_eq!(shared.status(), 200);
    Ok(workspace)
}

/// A task of `cookie`'s person on `workspace`.
async fn task_on(f: &Fixture, cookie: &str, workspace: &str, key: &str) -> Result<ConversationId> {
    let task: Value = call(f, cookie, Method::POST, "/api/v1/conversations")
        .header("idempotency-key", key)
        .json(&json!({"title": "Task", "model_route": "local", "workspace_id": workspace}))
        .send()
        .await?
        .json()
        .await?;
    Ok(task["id"].as_str().ok_or("task id")?.parse()?)
}

/// Answers `runtime`'s next turn, if one came, with a run started and ended at once: whether it did.
async fn answered(
    runtime: &mut support::runtime::Runtime,
    seq: &mut u64,
    run: usize,
) -> Result<bool> {
    let Ok(Some(tqa_types::ClientFrame::UserMessage { .. })) = runtime
        .try_action(std::time::Duration::from_millis(20))
        .await
    else {
        return Ok(false);
    };
    let run = format!("run-{run}");
    *seq += 1;
    runtime
        .send(&lifecycle(
            *seq,
            Event::RunStarted {
                run_id: run.parse()?,
            },
        ))
        .await?;
    *seq += 1;
    runtime
        .send(&lifecycle(
            *seq,
            Event::RunCompleted {
                run_id: run.parse()?,
            },
        ))
        .await?;
    Ok(true)
}

/// Runs admission for `window`, answering every turn the two tasks get (a run started and ended at once), and counts
/// their sandboxes' creates; returns the order in which each first got a turn.
async fn serve_both(
    f: &Fixture,
    (a, b): (&ConversationId, &ConversationId),
    window: std::time::Duration,
) -> Result<(Vec<ConversationId>, usize)> {
    let before = f.backend.creates.load(Ordering::SeqCst);
    let mut served: Vec<ConversationId> = Vec::new();
    let mut runtimes: Vec<(ConversationId, String, support::runtime::Runtime)> = Vec::new();
    let mut seq: std::collections::HashMap<ConversationId, u64> = std::collections::HashMap::new();
    let started = std::time::Instant::now();
    while started.elapsed() < window && served.len() < 2 {
        f.gateway.sandboxes().admission_pass_now().await?;
        for id in [a, b] {
            let Some(token) = f
                .backend
                .token(id)
                .map(|token| token.expose_secret().to_owned())
            else {
                continue;
            };
            if !runtimes
                .iter()
                .any(|(known, seen, _)| known == id && *seen == token)
                && f.backend.list_conversations().await?.contains(id)
            {
                runtimes.retain(|(known, _, _)| known != id);
                runtimes.push((id.clone(), token, f.runtime_for(id).await?));
            }
        }
        for (id, _, runtime) in &mut runtimes {
            let next = seq.entry(id.clone()).or_insert(0);
            if answered(runtime, next, served.len()).await? && !served.contains(id) {
                served.push(id.clone());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Ok((served, f.backend.creates.load(Ordering::SeqCst) - before))
}

// TEST: GW-I-258
// The review of the FIFO hand-over (P1): admission orders owners by their live sandboxes first, the hand-over by
// acceptance. On a workspace shared for writing between two people they disagreed, and the two tasks took the workspace
// from each other every pass. Turns on one workspace now go in acceptance order in admission too.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_258_two_owners_on_a_workspace_shared_for_writing_go_in_acceptance_order(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, owner) = f.admin().await?;
        let (other, other_cookie) = f.member("other@example.invalid").await?;
        // The other person runs two sandboxes elsewhere, each with a run going: admission's fairness puts the owner
        // first. (Idle ones would be evicted for the other person's queued task, their live count would fall to the
        // owner's, and acceptance order would win anyway, with or without the fix.)
        let mut elsewhere = Vec::new();
        for _ in 0..2 {
            let task = f.conversation(other.id).await?;
            let mut runtime = f.runtime(&task).await?;
            runtime
                .send(&lifecycle(
                    1,
                    Event::RunStarted {
                        run_id: "busy".parse()?,
                    },
                ))
                .await?;
            f.until_sequence(&task.id, 1).await?;
            elsewhere.push(runtime);
        }
        let workspace = shared_for_writing(f, &owner, other.id).await?;
        // A third task holds the workspace with its run going, so both wait for it and its hand-over decides: with
        // the workspace free, the older one's first admission pass would start it alone before the newer queued.
        let holder = task_on(f, &owner, &workspace, "holder").await?;
        assert_eq!(message(f, &owner, &holder, "h1").await?.status(), 202);
        until(f, |f| f.backend.created(&holder).is_some()).await?;
        let mut held = f.runtime_for(&holder).await?;
        assert!(matches!(
            held.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        held.send(&lifecycle(
            1,
            Event::RunStarted {
                run_id: "h1".parse()?,
            },
        ))
        .await?;
        f.until_sequence(&holder, 1).await?;
        let older = task_on(f, &other_cookie, &workspace, "older").await?;
        let newer = task_on(f, &owner, &workspace, "newer").await?;
        assert_eq!(message(f, &other_cookie, &older, "o1").await?.status(), 202);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(message(f, &owner, &newer, "n1").await?.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        held.send(&lifecycle(
            2,
            Event::RunCompleted {
                run_id: "h1".parse()?,
            },
        ))
        .await?;
        let (served, creates) =
            serve_both(f, (&newer, &older), std::time::Duration::from_secs(8)).await?;
        assert_eq!(
            served,
            [older.clone(), newer.clone()],
            "both served, the older first"
        );
        assert!(
            creates <= 3,
            "{creates} creates for two turns: the workspace went back and forth"
        );
        drop(elsewhere);
        Ok(())
    })
    .await
}

// TEST: GW-I-258 A waiter its owner's cap holds back isn't admitted this pass, so the holder doesn't yield to it: the
// holder's turn goes out, and nothing loops.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_258_a_waiter_held_by_its_owners_cap_is_not_yielded_to(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[admission]\nper_user = 2", async |f| {
        let (_, owner) = f.admin().await?;
        let (other, other_cookie) = f.member("other@example.invalid").await?;
        // The other person is at their cap, with two runs going (an idle sandbox would be evicted for their queued
        // task).
        let mut elsewhere = Vec::new();
        for _ in 0..2 {
            let task = f.conversation(other.id).await?;
            let mut runtime = f.runtime(&task).await?;
            runtime
                .send(&lifecycle(
                    1,
                    Event::RunStarted {
                        run_id: "busy".parse()?,
                    },
                ))
                .await?;
            f.until_sequence(&task.id, 1).await?;
            elsewhere.push(runtime);
        }
        let workspace = shared_for_writing(f, &owner, other.id).await?;
        let capped = task_on(f, &other_cookie, &workspace, "capped").await?;
        let free = task_on(f, &owner, &workspace, "free").await?;
        assert_eq!(
            message(f, &other_cookie, &capped, "c1").await?.status(),
            202
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(message(f, &owner, &free, "f1").await?.status(), 202);
        let before = f.backend.creates.load(Ordering::SeqCst);
        let (served, _) =
            serve_both(f, (&free, &capped), std::time::Duration::from_secs(6)).await?;
        assert_eq!(
            served.first(),
            Some(&free),
            "the uncapped task's turn goes out"
        );
        assert!(
            !served.contains(&capped),
            "the capped one waits for its owner's cap"
        );
        let creates = f.backend.creates.load(Ordering::SeqCst) - before;
        assert_eq!(creates, 1, "one sandbox, kept: {creates} creates");
        drop(elsewhere);
        Ok(())
    })
    .await
}

// TEST: GW-I-263
/// A restart that interrupts a hand-over's retirement keeps the holder's queued turns. They were waiting their turn
/// behind a task that queued first on the workspace, not lost with the sandbox, so they stay queued and the waiter
/// gets the workspace. A lone task's interrupted retirement still fails its queued work visibly (GW-I-011).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_263_a_restart_mid_hand_over_keeps_the_holders_turns_queued(
    pool: PgPool,
) -> TestResult {
    support::run_restarting(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, holder, waiter) = shared_workspace(f, &cookie).await?;
        assert_eq!(message(f, &cookie, &holder, "h1").await?.status(), 202);
        until(f, |f| f.backend.created(&holder).is_some()).await?;
        let mut runtime = f.runtime_for(&holder).await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "h1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&holder, 1).await?;
        // The waiter queues first, then the holder's next turn, held while its run goes.
        assert_eq!(message(f, &cookie, &waiter, "w1").await?.status(), 202);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(message(f, &cookie, &holder, "h2").await?.status(), 202);
        // The gateway stops just as the holder's run has ended and the hand-over has begun retiring its sandbox.
        f.gateway.cancel();
        drop(runtime);
        let holder_id = uuid::Uuid::parse_str(holder.as_str())?;
        sqlx::query(
            "INSERT INTO events (conversation_id, seq, run_id, ts, frame, lifecycle) \
             VALUES ($1, 2, 'h1', CURRENT_TIMESTAMP, '{}'::jsonb, 'run.completed')",
        )
        .bind(holder_id)
        .execute(&f.pool)
        .await?;
        sqlx::query("UPDATE sandbox_instances SET phase = 'destroying' WHERE conversation_id = $1")
            .bind(holder_id)
            .execute(&f.pool)
            .await?;
        f.restart().await?;
        let h2: (String, Option<String>) = sqlx::query_as(
            "SELECT execution_status, execution_error FROM idempotency WHERE key = 'h2'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            h2,
            ("queued".to_owned(), None),
            "the holder's turn waits its turn"
        );
        // The waiter, which queued first, gets the workspace; the holder's turn goes after it.
        let started = std::time::Instant::now();
        while f.backend.created(&waiter).is_none() {
            assert!(
                f.backend.created(&holder).is_none()
                    || f.backend.creates.load(Ordering::SeqCst) < 2,
                "the holder took the workspace back before the waiter, which queued first"
            );
            if started.elapsed() > support::DEADLINE {
                return Err(format!(
                    "the waiter never got the workspace (holder has a sandbox: {})",
                    f.backend.list_conversations().await?.contains(&holder)
                )
                .into());
            }
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        one_mount(f, &holder, &waiter).await?;
        let mut second = f.runtime_for(&waiter).await?;
        assert!(matches!(
            second.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        Ok(())
    })
    .await
}

// TEST: GW-I-303
// Real-world use before 17.0 (17 Oct 2026): a lead waiting for its turn on its own files, behind a helper that queued
// first, read "Waiting for a free slot · next" with 4 of 8 slots free. A task behind an older waiter on its workspace
// waits for the workspace, not for a slot: its stream and its place say so, as the waiter's do.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_303_a_task_behind_an_older_waiter_waits_for_the_workspace_not_a_slot(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (workspace, holder, waiter) = shared_workspace(f, &cookie).await?;
        let task: Value = call(f, &cookie, Method::POST, "/api/v1/conversations")
            .header("idempotency-key", "third")
            .json(&json!({"title":"Task","model_route":"local","workspace_id":workspace}))
            .send()
            .await?
            .json()
            .await?;
        let behind: ConversationId = task["id"].as_str().ok_or("third id")?.parse()?;
        assert_eq!(message(f, &cookie, &holder, "h").await?.status(), 202);
        let mut runtime = f.runtime_for(&holder).await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "busy".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&holder, 1).await?;
        // The waiter queues first, then the third task: it is behind the waiter on the same workspace.
        assert_eq!(message(f, &cookie, &waiter, "w").await?.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        assert_eq!(message(f, &cookie, &behind, "b").await?.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        for id in [&waiter, &behind] {
            let stream = f.stream(id, &cookie, 0).await?;
            assert_eq!(
                (stream.initial, stream.initial_reason),
                (SandboxState::Queued, Some(QueueReason::Workspace)),
                "{id} waits for the workspace"
            );
        }
        assert!(f.backend.created(&behind).is_none(), "no second mount");
        Ok(())
    })
    .await
}

// TEST: GW-I-309
// A live check before 17.0 (18 Oct 2026): a my_files helper's first turn reached its runtime, which journaled the
// message; 70 ms later, before its run started, the waiting helper's admission handed its sandbox over, and the turn
// was never run.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_309_a_holder_whose_turn_was_taken_keeps_its_sandbox_until_its_run_ends(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, holder, waiter) = shared_workspace(f, &cookie).await?;
        assert_eq!(message(f, &cookie, &holder, "h1").await?.status(), 202);
        until(f, |f| f.backend.created(&holder).is_some()).await?;
        let mut runtime = f.runtime_for(&holder).await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        // The runtime journals the message; its run hasn't started yet.
        runtime
            .send(&lifecycle(
                1,
                Event::MessageUser {
                    text: "work".into(),
                    attachments: Vec::new(),
                    lead_brief: None,
                    team_wake: None,
                },
            ))
            .await?;
        f.until_sequence(&holder, 1).await?;
        assert_eq!(message(f, &cookie, &waiter, "w1").await?.status(), 202);
        for _ in 0..10 {
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(
            f.backend.created(&waiter).is_none(),
            "handed over while the holder's turn was taken but its run hadn't started"
        );
        assert!(
            f.backend.list_conversations().await?.contains(&holder),
            "the holder keeps its sandbox"
        );
        // The run starts and ends: the waiter gets the workspace.
        runtime
            .send(&lifecycle(2, Event::RunStarted { run_id: "r-1".parse()? }))
            .await?;
        runtime
            .send(&lifecycle(3, Event::RunCompleted { run_id: "r-1".parse()? }))
            .await?;
        f.until_sequence(&holder, 3).await?;
        let started = std::time::Instant::now();
        while f.backend.created(&waiter).is_none() {
            assert!(
                started.elapsed() < support::DEADLINE,
                "the waiter never got the workspace after the holder's run"
            );
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        one_mount(f, &holder, &waiter).await?;
        // Never twice: the holder's message is journaled once, and its turn went once.
        let holder_id = uuid::Uuid::parse_str(holder.as_str())?;
        let messages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM events WHERE conversation_id = $1 AND frame->'event'->>'type' = 'message.user'",
        )
        .bind(holder_id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(messages, 1, "the message shows once in the transcript");
        let turns: Vec<String> = sqlx::query_scalar(
            "SELECT execution_status FROM idempotency WHERE conversation_id = $1 ORDER BY ordinal",
        )
        .bind(holder_id)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(turns, ["delivered"], "one turn, delivered once");
        Ok(())
    })
    .await
}
