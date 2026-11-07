//! GW-I-173: a task's first message still waiting for a slot (queued, not held behind a run) is listed as
//! `waiting_for_slot` and can be cancelled by its author or the task's owner. The cancel withdraws the task from the
//! slot queue: no sandbox starts for it, open streams re-read the list and see the task leave the queue. A cancel
//! racing the slot grant either wins, and the message never goes out, or finds its turn claimed: `409 message_sent`.
#[path = "support/fixture.rs"]
mod support;

use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ClientFrame, ConversationId, Event, SandboxState, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn send(f: &Fixture, cookie: &str, id: &ConversationId, key: &str) -> Result<Value> {
    let response = f
        .action(
            &format!("/api/v1/conversations/{id}/messages"),
            cookie,
            Some(key),
            &json!({"text": key}),
        )
        .await?;
    assert_eq!(response.status(), 202, "{key}");
    let body: Value = response.json().await?;
    assert_eq!(
        body["held"], false,
        "{key} waits for a slot, not behind a run"
    );
    Ok(body)
}

async fn cancel(
    f: &Fixture,
    cookie: &str,
    id: &ConversationId,
    sent: &Value,
) -> Result<(u16, Value)> {
    let path = format!(
        "/api/v1/conversations/{id}/messages/queued/{}",
        sent["message_id"].as_str().ok_or("message id")?
    );
    let response = f
        .client
        .delete(f.url(&path))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .send()
        .await?;
    let status = response.status().as_u16();
    let body = response.json().await.unwrap_or(Value::Null);
    Ok((status, body))
}

async fn listed(f: &Fixture, cookie: &str, id: &ConversationId) -> Result<Vec<(String, String)>> {
    let body: Value = f
        .get(
            &format!("/api/v1/conversations/{id}/messages/queued"),
            cookie,
        )
        .await?
        .json()
        .await?;
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .map(|item| {
            let field = |name: &str| item[name].as_str().unwrap_or_default().to_owned();
            (field("text"), field("state"))
        })
        .collect())
}

async fn status(f: &Fixture, key: &str) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT execution_status FROM idempotency WHERE key=$1")
            .bind(key)
            .fetch_one(&f.pool)
            .await?,
    )
}

async fn user_turn(runtime: &mut support::runtime::Runtime) -> Result<String> {
    match runtime.action().await? {
        ClientFrame::UserMessage { text, .. } => Ok(text),
        other => Err(format!("expected a user turn, got {other:?}").into()),
    }
}

/// Reads the stream until it has both re-read the list (`message.queue`) and left the queue (`destroyed`, no queue).
async fn left_the_queue(stream: &mut support::stream::Stream) -> TestResult {
    let (mut queue, mut state) = (false, false);
    for _ in 0..16 {
        match stream.next().await? {
            (None, ServerFrame::MessageQueue { held, .. }) => queue = held.is_empty(),
            (
                None,
                ServerFrame::SessionReady {
                    sandbox,
                    queue: position,
                    ..
                },
            ) => {
                state = sandbox.state == SandboxState::Destroyed && position.is_none();
            }
            _ => {}
        }
        if queue && state {
            return Ok(());
        }
    }
    Err("the stream was not told the task left the queue".into())
}

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

/// Occupies the only slot with another person's running task.
async fn full_host(f: &Fixture) -> Result<support::runtime::Runtime> {
    let (other, _) = f.member("busy@example.invalid").await?;
    let busy = f.conversation(other.id).await?;
    let mut runtime = f.runtime(&busy).await?;
    runtime
        .send(&lifecycle(
            1,
            Event::RunStarted {
                run_id: "busy".parse()?,
            },
        ))
        .await?;
    f.until_sequence(&busy.id, 1).await?;
    Ok(runtime)
}

// TEST: GW-I-173
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_173_a_first_message_waiting_for_a_slot_is_listed_and_its_cancel_withdraws_the_task(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=1\nper_user=1", async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut busy = full_host(f).await?;
        let first = f.conversation(owner.id).await?;
        let sent = send(f, &cookie, &first.id, "first").await?;
        f.gateway.sandboxes().admission_pass_now().await?;
        let mut stream = f.stream(&first.id, &cookie, 0).await?;
        assert_eq!(stream.initial, SandboxState::Queued);
        assert_eq!(
            listed(f, &cookie, &first.id).await?,
            [("first".to_owned(), "waiting_for_slot".to_owned())]
        );
        // Someone without access to the task is refused.
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        assert_eq!(cancel(f, &stranger, &first.id, &sent).await?.0, 403);
        assert_eq!(cancel(f, &cookie, &first.id, &sent).await?.0, 204);
        left_the_queue(&mut stream).await?;
        assert_eq!(status(f, "first").await?, "cancelled");
        assert!(listed(f, &cookie, &first.id).await?.is_empty());
        // The task left the slot queue: a later task of the same person gets the next slot, and the cancelled one
        // (earlier, so served first had it stayed) never gets a sandbox.
        let later = f.conversation(owner.id).await?;
        send(f, &cookie, &later.id, "later").await?;
        f.gateway.sandboxes().admission_pass_now().await?;
        let stream = f.stream(&first.id, &cookie, 0).await?;
        assert_eq!(stream.initial, SandboxState::Destroyed);
        // The busy run ends; its idle sandbox makes room for the next waiting task.
        busy.send(&lifecycle(
            2,
            Event::RunCompleted {
                run_id: "busy".parse()?,
            },
        ))
        .await?;
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.created(&later.id).is_none() {
                f.gateway.sandboxes().admission_pass_now().await?;
                tokio::task::yield_now().await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        assert!(
            f.backend.created(&first.id).is_none(),
            "a withdrawn task never starts"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-173
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_173_a_cancel_after_the_slot_is_granted_finds_the_message_sent(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        // Granted: the slot is the task's and its sandbox is starting, the message not sent yet. A 204 means no
        // sandbox, so the cancel is too late: the sandbox starts and the agent gets the message.
        let task = f.conversation(owner.id).await?;
        let sent = send(f, &cookie, &task.id, "granted").await?;
        tokio::time::timeout(support::DEADLINE, async {
            while reservations(f, &task.id).await? == 0 {
                tokio::task::yield_now().await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        let (code, body) = cancel(f, &cookie, &task.id, &sent).await?;
        assert_eq!(
            (code, &body["error"]["code"]),
            (409, &json!("message_sent")),
            "{body}"
        );
        let mut runtime = f.runtime(&task).await?;
        assert_eq!(user_turn(&mut runtime).await?, "granted");
        // The runtime has the turn: too late, and the agent gets it.
        let started = f.conversation(owner.id).await?;
        let sent = send(f, &cookie, &started.id, "started").await?;
        let mut runtime = f.runtime(&started).await?;
        assert_eq!(user_turn(&mut runtime).await?, "started");
        let (code, body) = cancel(f, &cookie, &started.id, &sent).await?;
        assert_eq!(
            (code, &body["error"]["code"]),
            (409, &json!("message_sent")),
            "{body}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-173
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_173_a_cancel_racing_the_start_never_cancels_a_message_the_agent_gets(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=4\nper_user=4", async |f| {
        let (owner, cookie) = f.admin().await?;
        for round in 0..4 {
            let task = f.conversation(owner.id).await?;
            let key = format!("race-{round}");
            let sent = send(f, &cookie, &task.id, &key).await?;
            let (cancelled, runtime) =
                tokio::join!(cancel(f, &cookie, &task.id, &sent), f.runtime(&task));
            let (mut runtime, (code, body)) = (runtime?, cancelled?);
            match code {
                // Won: never sent, so the runtime's first turn is the next message.
                204 => {
                    assert_eq!(status(f, &key).await?, "cancelled");
                    send(f, &cookie, &task.id, &format!("after-{round}")).await?;
                    assert_eq!(user_turn(&mut runtime).await?, format!("after-{round}"));
                }
                // Lost: the agent gets it.
                409 => assert_eq!(user_turn(&mut runtime).await?, key, "{body}"),
                other => return Err(format!("round {round}: {other} {body}").into()),
            }
        }
        Ok(())
    })
    .await
}

/// Whether a session waits for the admission lock (the one `manager::store` takes).
/// The task's slot reservations: 1 once the admission granted it a slot, until its sandbox is gone.
async fn reservations(f: &Fixture, id: &ConversationId) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM sandbox_instances WHERE conversation_id=$1::uuid")
            .bind(id.as_str())
            .fetch_one(&f.pool)
            .await?,
    )
}

/// Whether a grant waits for the task's row (the admission's first lock), having read the queue.
async fn pass_waits(f: &Fixture) -> Result<bool> {
    let waiting: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type='Lock' \
         AND query LIKE 'SELECT state FROM conversations WHERE id=$1 FOR SHARE%'",
    )
    .fetch_one(&f.pool)
    .await?;
    Ok(waiting > 0)
}

// TEST: GW-I-173
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_173_a_pass_that_read_the_queue_before_the_cancel_grants_no_slot(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        // No subnet: every pass admits the task, its create fails and the slot is given back, so it keeps waiting.
        f.backend
            .pool_exhausted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let task = f.conversation(owner.id).await?;
        let sent = send(f, &cookie, &task.id, "stale").await?;
        let mut stream = f.stream(&task.id, &cookie, 0).await?;
        // A pass reads the queue, then waits for the task's row this test holds, before it takes the admission lock. The
        // row is held only with no grant standing: a granted task's worker needs the row too (its runtime token), so
        // its slot would never be given back. Held so, no grant can complete and nothing is left to create.
        let lock = tokio::time::timeout(support::DEADLINE, async {
            loop {
                while reservations(f, &task.id).await? > 0 {
                    tokio::task::yield_now().await;
                }
                let mut lock = f.pool.begin().await?;
                sqlx::query("SELECT id FROM conversations WHERE id=$1::uuid FOR UPDATE")
                    .bind(task.id.as_str())
                    .execute(&mut *lock)
                    .await?;
                if reservations(f, &task.id).await? == 0 {
                    return Ok::<_, Box<dyn std::error::Error>>(lock);
                }
                lock.rollback().await?;
            }
        })
        .await??;
        let pass = tokio::spawn({
            let manager = f.gateway.sandboxes().clone();
            async move { manager.admission_pass_now().await }
        });
        tokio::time::timeout(support::DEADLINE, async {
            while !pass_waits(f).await? {
                tokio::task::yield_now().await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        let creates = f.backend.creates.load(std::sync::atomic::Ordering::SeqCst);
        // The cancel takes the admission lock and commits while that pass waits; its notice waits for the pass to end.
        let cancelled = tokio::join!(cancel(f, &cookie, &task.id, &sent), async {
            tokio::time::timeout(support::DEADLINE, async {
                while status(f, "stale").await? != "cancelled" {
                    tokio::task::yield_now().await;
                }
                Ok::<_, Box<dyn std::error::Error>>(())
            })
            .await??;
            lock.rollback().await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        });
        cancelled.1?;
        assert_eq!(cancelled.0?.0, 204);
        pass.await??;
        f.gateway.sandboxes().admission_pass_now().await?;
        left_the_queue(&mut stream).await?;
        assert_eq!(
            f.backend.creates.load(std::sync::atomic::Ordering::SeqCst),
            creates,
            "the pass gave the withdrawn task no slot"
        );
        Ok(())
    })
    .await
}

/// The list's raw items of `id`.
async fn items(f: &Fixture, cookie: &str, id: &ConversationId) -> Result<Vec<Value>> {
    let body: Value = f
        .get(
            &format!("/api/v1/conversations/{id}/messages/queued"),
            cookie,
        )
        .await?
        .json()
        .await?;
    Ok(body["items"].as_array().ok_or("items")?.clone())
}

/// Stores message `key`'s frame as the gateway's wake of a lead whose place is `after`, the way `send_turn` stores a
/// real wake (`teams::wake::team`, under the team's owner on the message route).
async fn as_wake(f: &Fixture, key: &str, after: u64) -> TestResult {
    let wake = ClientFrame::UserMessage {
        text: format!(
            "New messages from your helpers since message {after}. Read them with read_reports; end your turn when \
             you are waiting for more."
        ),
        attachments: Vec::new(),
        host_keys: Vec::new(),
        lead_brief: None,
        team_wake: Some(tqa_types::TeamWake { after }),
    };
    sqlx::query("UPDATE idempotency SET queued_frame=$1 WHERE key=$2")
        .bind(serde_json::to_vec(&wake)?)
        .bind(key)
        .execute(&f.pool)
        .await?;
    Ok(())
}

// TEST: GW-I-298
// The c16j team demo (finding 1): the lead's wake, waiting for a slot, was listed as the person's own message, with
// nothing to tell a client it was the gateway's. The wake is queued under the team's owner on the message route
// (teams::wake::team → send_turn); here its frame is stored as send_turn stores it (`as_wake`), so the list reads what
// a real wake leaves. The list says `team_wake` for it, and nothing for a person's own message.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_298_a_waiting_team_wake_is_listed_as_the_gateways(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=1\nper_user=1", async |f| {
        let (owner, cookie) = f.admin().await?;
        let _busy = full_host(f).await?;
        let lead = f.conversation(owner.id).await?;
        send(f, &cookie, &lead.id, "wake").await?;
        as_wake(f, "wake", 4).await?;
        let own = f.conversation(owner.id).await?;
        send(f, &cookie, &own.id, "own").await?;
        f.gateway.sandboxes().admission_pass_now().await?;
        let listed = items(f, &cookie, &lead.id).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["state"], "waiting_for_slot");
        assert_eq!(listed[0]["team_wake"], json!({"after": 4}));
        let listed = items(f, &cookie, &own.id).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["text"], "own");
        assert!(
            listed[0].get("team_wake").is_none(),
            "a person's own message is theirs: {listed:?}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-299
// A team's wake is no one's to cancel: cancelled, its key (the reader's place) would replay as cancelled, and the lead
// wouldn't be woken again for the messages that wait. Its DELETE, by the task's owner who is the wake's author, is
// `409 team_turn`; the wake stays queued and listed, and goes out when the slot is free. A person's own message on the
// same host is still theirs to cancel (GW-I-173).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_299_a_waiting_team_wake_cannot_be_cancelled(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[admission]\nslots=1\nper_user=1", async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut busy = full_host(f).await?;
        let lead = f.conversation(owner.id).await?;
        let sent = send(f, &cookie, &lead.id, "wake").await?;
        as_wake(f, "wake", 4).await?;
        f.gateway.sandboxes().admission_pass_now().await?;
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        assert_eq!(cancel(f, &stranger, &lead.id, &sent).await?.0, 403);
        let (code, body) = cancel(f, &cookie, &lead.id, &sent).await?;
        assert_eq!(
            (code, &body["error"]["code"]),
            (409, &json!("team_turn")),
            "{body}"
        );
        assert_eq!(status(f, "wake").await?, "queued");
        let listed = items(f, &cookie, &lead.id).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["team_wake"], json!({"after": 4}));
        // The busy run ends: the wake goes out as the lead's turn.
        busy.send(&lifecycle(
            2,
            Event::RunCompleted {
                run_id: "busy".parse()?,
            },
        ))
        .await?;
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.created(&lead.id).is_none() {
                f.gateway.sandboxes().admission_pass_now().await?;
                tokio::task::yield_now().await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        let mut runtime = f.runtime(&lead).await?;
        assert!(user_turn(&mut runtime).await?.contains("read_reports"));
        Ok(())
    })
    .await
}

/// A running team of `owner`'s `lead` and one `own_copy` helper still before its first turn, as `teams::start` leaves it.
async fn team_of(
    f: &Fixture,
    owner: uuid::Uuid,
    lead: &ConversationId,
    helper: &ConversationId,
) -> Result<uuid::Uuid> {
    let (team, channel) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
    sqlx::query("INSERT INTO channels (id, owner_id, name, rule, kind) VALUES ($1, $2, 'Agent team', 'human', 'agent_team')")
        .bind(channel)
        .bind(owner)
        .execute(&f.pool)
        .await?;
    sqlx::query(
        "INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, token_budget, deadline_at) \
         VALUES ($1, $2, $3, $4, 2000000, CURRENT_TIMESTAMP + INTERVAL '2 hours')",
    )
    .bind(team)
    .bind(channel)
    .bind(uuid::Uuid::parse_str(lead.as_str())?)
    .bind(owner)
    .execute(&f.pool)
    .await?;
    sqlx::query(
        "INSERT INTO agent_team_members (conversation_id, team_id, name, role, workspace, state) \
         VALUES ($1, $2, 'tester', 'tester', 'own_copy', 'starting')",
    )
    .bind(uuid::Uuid::parse_str(helper.as_str())?)
    .bind(team)
    .execute(&f.pool)
    .await?;
    Ok(team)
}

/// Renames queued message `key` to the team turn `team_key`, as the gateway stores a wake or a first turn.
async fn as_team_turn(f: &Fixture, key: &str, team_key: &str) -> TestResult {
    sqlx::query("UPDATE idempotency SET key=$1 WHERE key=$2")
        .bind(team_key)
        .bind(key)
        .execute(&f.pool)
        .await?;
    Ok(())
}

/// Reads the stream until it is told the task left the queue (`destroyed`, no queue).
async fn told_it_left(stream: &mut support::stream::Stream) -> TestResult {
    for _ in 0..16 {
        if let (None, ServerFrame::SessionReady { sandbox, queue, .. }) = stream.next().await?
            && sandbox.state == SandboxState::Destroyed
            && queue.is_none()
        {
            return Ok(());
        }
    }
    Err("the stream was not told the task left the queue".into())
}

// TEST: GW-I-312
// The 16p live W3 check: a person's Stop team dropped the lead's waiting wake, but the lead's open page went on reading
// "Waiting for the workspace" until reloaded, as the stop never withdrew the task from the queue (GW-I-173's notice);
// and a helper whose sandbox was starting when its first turn was dropped kept that sandbox, and its lease, up for its
// whole idle time. Here the lead's wake waits for a slot, and the helper's first turn has the only free one, its runtime
// not yet registered. After the stop the lead's open stream is told it left the queue, a stream opened then reads no
// queue, and the helper's sandbox goes without waiting for its readiness bound (30 s, past the test's 10 s).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_312_a_stop_withdraws_the_teams_waiting_turns_and_retires_a_starting_helper(
    pool: PgPool,
) -> TestResult {
    let config = "[agent_teams]\nenabled = true\n[admission]\nslots=2\nper_user=2";
    support::run_configured(pool, None, config, async |f| {
        let (owner, cookie) = f.admin().await?;
        let _busy = full_host(f).await?;
        let (lead, helper) = (
            f.conversation(owner.id).await?,
            f.conversation(owner.id).await?,
        );
        let team = team_of(f, owner.id, &lead.id, &helper.id).await?;
        // The helper's first turn first, so it takes the free slot; the lead's wake waits behind it.
        send(f, &cookie, &helper.id, "start").await?;
        as_team_turn(f, "start", &format!("team-start:{}", helper.id.as_str())).await?;
        send(f, &cookie, &lead.id, "wake").await?;
        let wake = format!("team-wake:{}:4", lead.id.as_str());
        as_team_turn(f, "wake", &wake).await?;
        f.gateway.sandboxes().admission_pass_now().await?;
        assert!(
            f.backend.created(&helper.id).is_some(),
            "the helper's sandbox is starting"
        );
        let mut stream = f.stream(&lead.id, &cookie, 0).await?;
        assert_eq!(stream.initial, SandboxState::Queued);
        sqlx::query(
            "UPDATE agent_teams SET state = 'stopping', stop_reason = 'stopped' WHERE id = $1",
        )
        .bind(team)
        .execute(&f.pool)
        .await?;
        f.gateway.reconcile_agent_teams_now().await?;
        assert_eq!(status(f, &wake).await?, "cancelled");
        told_it_left(&mut stream).await?;
        let stream = f.stream(&lead.id, &cookie, 0).await?;
        assert_eq!(stream.initial, SandboxState::Destroyed);
        assert_eq!(stream.initial_queue, None);
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.list_conversations().await?.contains(&helper.id) {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await
        .map_err(|_| "the stopped helper's starting sandbox is still up")??;
        Ok(())
    })
    .await
}
