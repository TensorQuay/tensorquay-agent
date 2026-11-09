//! GW-I-031: one read-write sandbox per workspace. The write lease is the workspace's live sandbox: a second
//! task waits (or is refused) while the holder runs, an idle holder hands over at once, and a dead holder's lease
//! expires.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult};
use tqa_sandbox::SandboxBackend;
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

/// As [`shared_workspace`], with names and keys of its own, for a test that makes several.
async fn shared_workspace_named(
    f: &Fixture,
    cookie: &str,
    tag: &str,
) -> Result<(String, ConversationId, ConversationId)> {
    let created: Value = call(f, cookie, Method::POST, "/api/v1/workspaces")
        .json(&json!({"name": format!("Leased {tag}")}))
        .send()
        .await?
        .json()
        .await?;
    let workspace = created["id"].as_str().ok_or("id")?.to_owned();
    let mut tasks = Vec::new();
    for key in ["first", "second"] {
        let key = format!("{key}-{tag}");
        let task: Value = call(f, cookie, Method::POST, "/api/v1/conversations")
            .header("idempotency-key", key.as_str())
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

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_031_a_second_task_waits_while_the_holder_runs_and_starts_when_it_ends(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, a, b) = shared_workspace(f, &cookie).await?;
        assert_eq!(message(f, &cookie, &a, "a").await?.status(), 202);
        let mut runtime_a = f.runtime_for(&a).await?;
        assert!(matches!(
            runtime_a.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        runtime_a
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "busy".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&a, 1).await?;
        // B is accepted and waits for the workspace, not for a slot; reads work throughout.
        assert_eq!(message(f, &cookie, &b, "b").await?.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        let waiting = f.stream(&b, &cookie, 0).await?;
        assert_eq!(
            (waiting.initial, waiting.initial_reason),
            (SandboxState::Queued, Some(QueueReason::Workspace))
        );
        assert_eq!(
            f.get(
                &format!("/api/v1/conversations/{b}/events?after=0&limit=10"),
                &cookie
            )
            .await?
            .status(),
            200
        );
        assert!(
            f.backend.created(&b).is_none(),
            "no second read-write mount while the holder runs"
        );
        one_mount(f, &a, &b).await?;
        // The holder's run ends: it hands over at once, without waiting for its idle window.
        runtime_a
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "busy".parse()?,
                },
            ))
            .await?;
        until(f, |f| f.backend.created(&b).is_some()).await?;
        one_mount(f, &a, &b).await?;
        assert!(
            !f.backend.list_conversations().await?.contains(&a),
            "the idle holder was retired for the waiter"
        );
        assert_eq!(
            f.backend.mounted(&b),
            f.backend.mounted(&a),
            "the same workspace directory, one mount at a time"
        );
        let mut runtime_b = f.runtime_for(&b).await?;
        assert!(matches!(
            runtime_b.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_031_with_busy_policy_refuse_a_second_task_is_refused_while_the_holder_runs(
    pool: PgPool,
) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[workspaces]\nbusy_policy='refuse'",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let (_, a, b) = shared_workspace(f, &cookie).await?;
            assert_eq!(message(f, &cookie, &a, "a").await?.status(), 202);
            let mut runtime_a = f.runtime_for(&a).await?;
            assert!(matches!(
                runtime_a.action().await?,
                tqa_types::ClientFrame::UserMessage { .. }
            ));
            runtime_a
                .send(&lifecycle(
                    1,
                    Event::RunStarted {
                        run_id: "busy".parse()?,
                    },
                ))
                .await?;
            f.until_sequence(&a, 1).await?;
            let refused = message(f, &cookie, &b, "refused").await?;
            assert_eq!(refused.status(), 409);
            assert_eq!(
                refused.json::<Value>().await?["error"]["code"],
                "workspace_busy"
            );
            let queued: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM idempotency WHERE execution_status='queued'",
            )
            .fetch_one(&f.pool)
            .await?;
            assert_eq!(queued, 0, "a refused message is not queued");
            // Idle holder: the next message takes the workspace.
            runtime_a
                .send(&lifecycle(
                    2,
                    Event::RunCompleted {
                        run_id: "busy".parse()?,
                    },
                ))
                .await?;
            f.until_sequence(&a, 2).await?;
            assert_eq!(message(f, &cookie, &b, "accepted").await?.status(), 202);
            until(f, |f| f.backend.created(&b).is_some()).await?;
            one_mount(f, &a, &b).await?;
            Ok(())
        },
    )
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_031_a_dead_holders_lease_expires_and_the_waiter_starts(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (workspace, a, b) = {
            let (_, cookie) = f.admin().await?;
            let (workspace, a, b) = shared_workspace(f, &cookie).await?;
            assert_eq!(message(f, &cookie, &a, "a").await?.status(), 202);
            let mut runtime_a = f.runtime_for(&a).await?;
            assert!(matches!(runtime_a.action().await?, tqa_types::ClientFrame::UserMessage { .. }));
            runtime_a.send(&lifecycle(1, Event::RunStarted { run_id: "busy".parse()? })).await?;
            f.until_sequence(&a, 1).await?;
            // The holder's runtime dies mid-run: no more heartbeats, no link.
            drop(runtime_a);
            f.forget_runtime(&a)?;
            assert_eq!(message(f, &cookie, &b, "b").await?.status(), 202);
            (workspace, a, b)
        };
        // Its lease runs out (the injected expiry stands in for missed heartbeats).
        sqlx::query("UPDATE workspaces SET lease_expires_at=CURRENT_TIMESTAMP-INTERVAL '1 second' WHERE id=$1")
            .bind(uuid::Uuid::parse_str(&workspace)?).execute(&f.pool).await?;
        f.gateway.sandboxes().admission_pass_now().await?;
        until(f, |f| f.backend.created(&b).is_some()).await?;
        one_mount(f, &a, &b).await?;
        assert!(f.backend.destroys.load(Ordering::SeqCst) >= 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_031_the_lease_is_renewed_by_heartbeats_and_released_with_the_sandbox(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (workspace, a, _) = shared_workspace(f, &cookie).await?;
        let workspace = uuid::Uuid::parse_str(&workspace)?;
        assert_eq!(message(f, &cookie, &a, "a").await?.status(), 202);
        let mut runtime_a = f.runtime_for(&a).await?;
        assert!(matches!(runtime_a.action().await?, tqa_types::ClientFrame::UserMessage { .. }));
        let (holder, first): (Option<uuid::Uuid>, Option<time::OffsetDateTime>) =
            sqlx::query_as("SELECT lease_conversation_id, lease_expires_at FROM workspaces WHERE id=$1").bind(workspace).fetch_one(&f.pool).await?;
        assert_eq!(holder, Some(uuid::Uuid::parse_str(a.as_str())?));
        // A new sandbox's lease covers its start (GW-I-247): the default create budget (120 s), readiness wait (30 s)
        // and 30 s more, above the default time to live of 90 s; a heartbeat then renews it for the time to live.
        let ttl = first.ok_or("expiry")? - time::OffsetDateTime::now_utc();
        assert!((170..=180).contains(&ttl.whole_seconds()), "{ttl}");
        sqlx::query("UPDATE workspaces SET lease_expires_at=CURRENT_TIMESTAMP+INTERVAL '5 seconds' WHERE id=$1").bind(workspace).execute(&f.pool).await?;
        runtime_a.send(&ServerFrame::Heartbeat { run_id: "busy".parse()? }).await?;
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let expiry: Option<time::OffsetDateTime> = sqlx::query_scalar("SELECT lease_expires_at FROM workspaces WHERE id=$1").bind(workspace).fetch_one(&f.pool).await?;
                if expiry.is_some_and(|expiry| expiry - time::OffsetDateTime::now_utc() > time::Duration::seconds(60)) {
                    return Ok::<(), Box<dyn std::error::Error>>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await??;
        // Released when the sandbox record goes (here: the idle window, advanced until it has passed).
        f.clock.freeze();
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let live: i64 = sqlx::query_scalar("SELECT count(*) FROM sandbox_instances").fetch_one(&f.pool).await?;
                if live == 0 {
                    return Ok::<(), Box<dyn std::error::Error>>(());
                }
                f.clock.advance(std::time::Duration::from_mins(10));
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await??;
        let holder: Option<uuid::Uuid> = sqlx::query_scalar("SELECT lease_conversation_id FROM workspaces WHERE id=$1").bind(workspace).fetch_one(&f.pool).await?;
        assert_eq!(holder, None);
        Ok(())
    })
    .await
}

// TEST: GW-I-247
// A stall in a soak (7 Oct 2026): a sandbox made for a queued turn has no active run until its runtime registers and
// the turn is delivered. A waiter on the same workspace must not hand it over in that window, or the two tasks take the
// workspace from each other and neither turn is delivered. Ordinary tasks, no agent team.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_247_two_tasks_queued_on_one_workspace_are_both_delivered(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, a, b) = shared_workspace(f, &cookie).await?;
        assert_eq!(message(f, &cookie, &a, "a").await?.status(), 202);
        assert_eq!(message(f, &cookie, &b, "b").await?.status(), 202);
        // Admission runs (its own task every second and on each nudge, and these passes) while the first sandbox's
        // runtime hasn't registered: the holder has no active run yet.
        for _ in 0..20 {
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let creates = f.backend.creates.load(Ordering::SeqCst);
        let destroys = f.backend.destroys.load(Ordering::SeqCst);
        assert_eq!(
            (creates, destroys),
            (1, 0),
            "a holder whose queued turn wasn't delivered yet was handed over ({creates} creates, {destroys} destroys)"
        );
        let live = f.backend.list_conversations().await?;
        let (first, second) = if live.contains(&a) { (a, b) } else { (b, a) };
        one_mount(f, &first, &second).await?;
        // The holder's turn is delivered and its run ends: the waiter's sandbox follows and its turn is delivered.
        let mut runtime_first = f.runtime_for(&first).await?;
        assert!(matches!(
            runtime_first.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        runtime_first
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "first".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&first, 1).await?;
        runtime_first
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "first".parse()?,
                },
            ))
            .await?;
        until(f, |f| f.backend.created(&second).is_some()).await?;
        one_mount(f, &first, &second).await?;
        let mut runtime_second = f.runtime_for(&second).await?;
        assert!(matches!(
            runtime_second.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        Ok(())
    })
    .await
}

// TEST: GW-I-247
// The lease is taken at admission, before the container exists, and nothing renews it until the
// runtime's first heartbeat. A start may take up to `create_budget_seconds` plus `ready_seconds`, so a new sandbox's
// lease lasts at least that long: with a 10 s lease, a start still registering at 12 s isn't handed over.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_247_a_slow_start_keeps_its_workspace_past_the_lease_time(pool: PgPool) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[workspaces]\nlease_ttl_seconds=10",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let (_, a, b) = shared_workspace(f, &cookie).await?;
            assert_eq!(message(f, &cookie, &a, "a").await?.status(), 202);
            assert_eq!(message(f, &cookie, &b, "b").await?.status(), 202);
            f.gateway.sandboxes().admission_pass_now().await?;
            // Past the 10 s lease, the first sandbox's runtime still hasn't registered.
            tokio::time::sleep(std::time::Duration::from_secs(12)).await;
            for _ in 0..5 {
                f.gateway.sandboxes().admission_pass_now().await?;
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            let creates = f.backend.creates.load(Ordering::SeqCst);
            let destroys = f.backend.destroys.load(Ordering::SeqCst);
            assert_eq!(
                (creates, destroys),
                (1, 0),
                "a start slower than the lease was handed over ({creates} creates, {destroys} destroys)"
            );
            let live = f.backend.list_conversations().await?;
            let first = if live.contains(&a) { a } else { b };
            let mut runtime = f.runtime_for(&first).await?;
            assert!(matches!(
                runtime.action().await?,
                tqa_types::ClientFrame::UserMessage { .. }
            ));
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-247
// A holder whose queued turn is never delivered (its runtime never registers) keeps the
// workspace only until its lease lapses; then the waiter's pass hands it over, the queued turn notwithstanding.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_247_a_dead_holder_with_a_queued_turn_hands_over_once_its_lease_lapses(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (workspace, a, b) = shared_workspace(f, &cookie).await?;
        assert_eq!(message(f, &cookie, &a, "a").await?.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        until(f, |f| f.backend.created(&a).is_some()).await?;
        assert_eq!(message(f, &cookie, &b, "b").await?.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        assert!(
            f.backend.created(&b).is_none(),
            "the holder's turn is undelivered: it keeps the workspace"
        );
        // Its runtime never registers, and its lease runs out (the injected expiry stands in for the time passing).
        sqlx::query(
            "UPDATE workspaces SET lease_expires_at=CURRENT_TIMESTAMP-INTERVAL '1 second' WHERE id=$1",
        )
        .bind(uuid::Uuid::parse_str(&workspace)?)
        .execute(&f.pool)
        .await?;
        f.gateway.sandboxes().admission_pass_now().await?;
        // The lapsed holder was handed over: its sandbox is retired, and the workspace goes to the oldest queued turn,
        // which is still the dead holder's own (a fresh sandbox, whose runtime may register this time) or the waiter's.
        until(f, |f| f.backend.destroys.load(Ordering::SeqCst) >= 1).await?;
        until(f, |f| f.backend.creates.load(Ordering::SeqCst) >= 2).await?;
        let live = f.backend.list_conversations().await?;
        one_mount(f, &a, &b).await?;
        let (first, second) = if live.contains(&a) { (a, b) } else { (b, a) };
        // Both turns are then delivered, one mount at a time (a fresh sandbox has fresh credentials).
        f.forget_runtime(&first)?;
        let mut runtime_first = f.runtime_for(&first).await?;
        assert!(matches!(
            runtime_first.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        runtime_first
            .send(&lifecycle(1, Event::RunStarted { run_id: "first".parse()? }))
            .await?;
        f.until_sequence(&first, 1).await?;
        runtime_first
            .send(&lifecycle(2, Event::RunCompleted { run_id: "first".parse()? }))
            .await?;
        until(f, |f| f.backend.created(&second).is_some()).await?;
        one_mount(f, &first, &second).await?;
        let mut runtime_second = f.runtime_for(&second).await?;
        assert!(matches!(
            runtime_second.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        Ok(())
    })
    .await
}

/// A short lease, so a lapse shows in seconds; the readiness wait at its least.
const RESTART_CONFIG: &str =
    "[workspaces]\nlease_ttl_seconds=10\n[sandbox]\nready_seconds=2\n[limits]\nio_timeout=2";
/// How long GW-I-251 serves the two tasks after a restart, from `RESTART_CONFIG`: the waiter's lease (10 s), while the
/// holder's later turn waits its turn; a turn the restart caught mid-delivery, put back after 3 x `io_timeout` (6 s); a
/// new sandbox's start (`ready_seconds`, 2); and a margin.
const SERVE_WINDOW: std::time::Duration = std::time::Duration::from_secs(10 + 3 * 2 + 2 + 5);

/// The holder's sandbox started and its runtime registered and took its turn: `run` true leaves that run going (no
/// heartbeat yet, as in a run's first seconds), false ends it. Returns the holder's runtime.
async fn holding(
    f: &mut Fixture,
    cookie: &str,
    holder: &ConversationId,
    run: bool,
) -> Result<support::runtime::Runtime> {
    assert_eq!(
        message(f, cookie, holder, &format!("h1-{holder}"))
            .await?
            .status(),
        202
    );
    until(f, |f| f.backend.created(holder).is_some()).await?;
    let mut runtime = f.runtime_for(holder).await?;
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
    f.until_sequence(holder, 1).await?;
    if !run {
        runtime
            .send(&lifecycle(
                2,
                Event::RunCompleted {
                    run_id: "h1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(holder, 2).await?;
    }
    Ok(runtime)
}

/// The waiter's sandbox starts within `bound` of the restart, the admission pass running as it does.
async fn waiter_starts_within(
    f: &Fixture,
    waiter: &ConversationId,
    bound: std::time::Duration,
) -> TestResult {
    let started = std::time::Instant::now();
    while f.backend.created(waiter).is_none() {
        assert!(
            started.elapsed() < bound,
            "the waiter still waits for the workspace {:?} after the restart",
            started.elapsed()
        );
        f.gateway.sandboxes().admission_pass_now().await?;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    Ok(())
}

// TEST: GW-I-249
// After a gateway restart, an idle holder still hands its workspace over at once.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_249_after_a_restart_an_idle_holder_hands_over_at_once(pool: PgPool) -> TestResult {
    support::run_restarting_with(pool, None, RESTART_CONFIG, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, holder, waiter) = shared_workspace(f, &cookie).await?;
        let runtime = holding(f, &cookie, &holder, false).await?;
        drop(runtime);
        f.restart().await?;
        let (_, cookie) = f.login("admin@example.invalid").await?;
        // Its runtime comes back, as a live one does.
        let _back = f.runtime_for(&holder).await?;
        assert_eq!(message(f, &cookie, &waiter, "w1").await?.status(), 202);
        waiter_starts_within(f, &waiter, std::time::Duration::from_secs(8)).await?;
        one_mount(f, &holder, &waiter).await
    })
    .await
}

// TEST: GW-I-249
// A run cut by the restart whose runtime never comes back reads as active: its holder keeps the workspace only until
// its lease lapses (`lease_ttl`), never for the start lease it took when its sandbox was made.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_249_a_run_cut_by_a_restart_holds_the_workspace_no_longer_than_its_lease(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, RESTART_CONFIG, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, holder, waiter) = shared_workspace(f, &cookie).await?;
        let runtime = holding(f, &cookie, &holder, true).await?;
        drop(runtime);
        f.restart().await?;
        let (_, cookie) = f.login("admin@example.invalid").await?;
        assert_eq!(message(f, &cookie, &waiter, "w1").await?.status(), 202);
        // The lease (10 s) plus the readiness wait (2 s) and a margin: never the 180 s start lease.
        waiter_starts_within(f, &waiter, std::time::Duration::from_secs(25)).await?;
        one_mount(f, &holder, &waiter).await
    })
    .await
}

// TEST: GW-I-249
// A holder whose sandbox is lost across the restart is made again with `lease_ttl`, not a new start lease: a waiter
// isn't held behind it for minutes.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_249_a_holder_lost_at_a_restart_holds_the_workspace_no_longer_than_its_lease(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, RESTART_CONFIG, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, holder, waiter) = shared_workspace(f, &cookie).await?;
        let runtime = holding(f, &cookie, &holder, true).await?;
        drop(runtime);
        let lost = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        f.backend.lose(&lost).await?;
        f.restart().await?;
        let (_, cookie) = f.login("admin@example.invalid").await?;
        assert_eq!(message(f, &cookie, &waiter, "w1").await?.status(), 202);
        waiter_starts_within(f, &waiter, std::time::Duration::from_secs(25)).await?;
        one_mount(f, &holder, &waiter).await
    })
    .await
}

/// The conversation's queued actions that failed, with their error.
/// The two tasks' turns not gone out yet: still queued. A turn sent stays `sending` until a lifecycle event after it
/// confirms the runtime took it, and this harness's runtimes never send one, so `sending` counts as gone out.
async fn pending(f: &Fixture, a: &ConversationId, b: &ConversationId) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM idempotency WHERE conversation_id = ANY($1) AND execution_status = 'queued'",
    )
    .bind(vec![uuid::Uuid::parse_str(a.as_str())?, uuid::Uuid::parse_str(b.as_str())?])
    .fetch_one(&f.pool)
    .await?)
}

async fn failed_actions(f: &Fixture, id: &ConversationId) -> Result<Vec<(String, Option<String>)>> {
    Ok(sqlx::query_as(
        "SELECT route, execution_error FROM idempotency WHERE conversation_id = $1 AND execution_status = 'failed'",
    )
    .bind(uuid::Uuid::parse_str(id.as_str())?)
    .fetch_all(&f.pool)
    .await?)
}

// TEST: GW-I-251
// E2E-AGENTS-020's try 5 (7 Oct 2026): a gateway restart caught a workspace hand-over half done (the holder retired, the
// waiter's sandbox starting), and after the restart the holder's queued turn failed with `sandbox_token_exists`. With
// ordinary tasks, at several points of the hand-over: no queued turn may fail, and each task gets its sandbox.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_251_a_restart_during_a_hand_over_fails_no_queued_turn(pool: PgPool) -> TestResult {
    support::run_restarting_with(pool, None, RESTART_CONFIG, async |f| {
        let (_, cookie) = f.admin().await?;
        let mut cookie = cookie;
        for delay in [0_u64, 5, 20, 50, 100, 200] {
            let (_, holder, waiter) =
                shared_workspace_named(f, &cookie, &format!("d{delay}")).await?;
            let mut runtime = holding(f, &cookie, &holder, true).await?;
            assert_eq!(
                message(f, &cookie, &waiter, &format!("w-{delay}"))
                    .await?
                    .status(),
                202
            );
            assert_eq!(
                message(f, &cookie, &holder, &format!("h2-{delay}"))
                    .await?
                    .status(),
                202
            );
            // The holder's run ends: the hand-over begins, and the gateway restarts part way through it.
            runtime
                .send(&lifecycle(
                    2,
                    Event::RunCompleted {
                        run_id: "h1".parse()?,
                    },
                ))
                .await?;
            f.until_sequence(&holder, 2).await?;
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            drop(runtime);
            let created = f.backend.creates.load(Ordering::SeqCst);
            f.restart().await?;
            cookie = f.login("admin@example.invalid").await?.1;
            // Each task's runtime comes back whenever it has a sandbox (a live runtime re-registers after a restart,
            // a new sandbox's registers when made), and takes what is sent to it; nothing queued may have failed.
            let mut live: Vec<(ConversationId, String, support::runtime::Runtime)> = Vec::new();
            // Until both tasks' turns have gone out (none left queued). In acceptance order (GW-I-248) the holder's
            // later turn waits for the waiter, whose run the harness never ends, so it may go only once the waiter's
            // lease runs out (`lease_ttl_seconds`, 10): a fixed 6 s window stopped dialling while its sandbox still
            // started. Within SERVE_WINDOW every turn must have gone out.
            let started = std::time::Instant::now();
            while started.elapsed() < SERVE_WINDOW && pending(f, &holder, &waiter).await? > 0 {
                f.gateway.sandboxes().admission_pass_now().await?;
                for id in [&holder, &waiter] {
                    let Some(token) = f
                        .backend
                        .live_token(id)
                        .await?
                        .map(|token| token.expose_secret().to_owned())
                    else {
                        continue;
                    };
                    if !live
                        .iter()
                        .any(|(known, seen, _)| known == id && *seen == token)
                        && f.backend.list_conversations().await?.contains(id)
                    {
                        live.retain(|(known, _, _)| known != id);
                        // Each sandbox's own token: since turns on one workspace go in the order they were accepted
                        // (GW-I-248), the hand-over can pass the workspace to the waiter and back, retiring a sandbox
                        // and making a new one, so the token the fixture kept may be a retired sandbox's. One the
                        // gateway refuses (being retired; its token revoked) is tried again next tick, and a dial is
                        // bounded well inside `ready_seconds` (2), so a refused one never holds the loop past a new
                        // sandbox's start.
                        if let Ok(Ok(runtime)) = tokio::time::timeout(
                            std::time::Duration::from_millis(500),
                            f.runtime_of_sandbox(id),
                        )
                        .await
                        {
                            live.push((id.clone(), token, runtime));
                        }
                    }
                }
                for (_, _, runtime) in &mut live {
                    let _ = runtime
                        .try_action(std::time::Duration::from_millis(10))
                        .await;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            drop(live);
            // At most the waiter's sandbox and the holder's back: a holder re-admitted ahead of the waiter, then handed
            // over again, made a sandbox every pass (115 in one run).
            let creates = f.backend.creates.load(Ordering::SeqCst) - created;
            assert!(creates <= 3, "delay {delay} ms: {creates} sandboxes made after the restart");
            // Every turn went out within the window: one still pending would fail only after this delay's checks.
            let left: Vec<(uuid::Uuid, String, String, bool)> = sqlx::query_as(
                "SELECT conversation_id, key, execution_status, held FROM idempotency \
                 WHERE conversation_id = ANY($1) AND execution_status = 'queued' ORDER BY created_at",
            )
            .bind(vec![uuid::Uuid::parse_str(holder.as_str())?, uuid::Uuid::parse_str(waiter.as_str())?])
            .fetch_all(&f.pool)
            .await?;
            assert!(
                left.is_empty(),
                "delay {delay} ms: a turn never went out (holder {holder}, waiter {waiter}): {left:?}; holder sandbox: {}, waiter sandbox: {}",
                f.backend.list_conversations().await?.contains(&holder),
                f.backend.list_conversations().await?.contains(&waiter)
            );
            for (who, id) in [("holder", &holder), ("waiter", &waiter)] {
                let failed = failed_actions(f, id).await?;
                assert!(
                    failed.is_empty(),
                    "delay {delay} ms: the {who} {id}'s queued actions failed: {failed:?}"
                );
            }
        }
        Ok(())
    })
    .await
}
