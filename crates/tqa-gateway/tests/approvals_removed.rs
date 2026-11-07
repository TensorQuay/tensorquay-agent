//! ADR-0035: per-call permissions, approvals and parking are gone. GW-I-134: their API surfaces answer as unknown, and
//! nothing the gateway sends names them. GW-I-135: an installation upgraded with runs waiting for an approval (parked,
//! or asking) ends them failed with a notice and drops what was held behind them, strips the stored policy, rules and
//! class with audit rows, and a runtime that sends a retired approval or park frame is refused.
//! GW-I-139 (not ADR-0035, the same refusal path): a runtime that sends the gateway's task.titled notice is refused.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{TestResult, call};
use tqa_types::{ErrorCode, Event, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
/// An audit row: actor kind, area, item, and the versions before and after.
type Audit = (String, String, String, Option<i64>, Option<i64>);

fn reason(body: &Value) -> Value {
    body["error"]["details"][0]["reason"].clone()
}

// TEST: GW-I-134
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_134_the_approval_push_setting_and_park_surfaces_are_gone(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        // The approval endpoint.
        let decided = f
            .action(
                &format!("/api/v1/conversations/{id}/permissions/req-1"),
                &cookie,
                Some("decide"),
                &json!({"decision": "allow_once", "action_hash": "ab".repeat(32)}),
            )
            .await?;
        assert_eq!(decided.status(), 404);
        // The park delay.
        let (status, body) = call(
            f,
            Method::PUT,
            "/api/v1/admin/limits/sandbox.park_after_seconds",
            &cookie,
            (Some(json!({"value": 60, "version": 1})), None),
        )
        .await?;
        assert_eq!((status, reason(&body)), (400, json!("limit_unknown")), "{body}");
        // A profile's policy and rules, and a connector's class.
        for removed in [json!({"policy": "read_only"}), json!({"rules": []})] {
            let mut profile = json!({"name": "old", "display_name": "Old"});
            profile
                .as_object_mut()
                .ok_or("profile")?
                .extend(removed.as_object().ok_or("field")?.clone());
            let (status, body) =
                call(f, Method::POST, "/api/v1/profiles", &cookie, (Some(profile), None)).await?;
            assert_eq!(status, 400, "{body}");
        }
        let connector = json!({"url": "https://mcp.tracker.example/mcp", "auth": {"scheme": "none"}, "class": "read"});
        let (status, body) = call(
            f,
            Method::PUT,
            "/api/v1/admin/connectors/tracker",
            &cookie,
            (Some(connector), None),
        )
        .await?;
        assert_eq!(status, 400, "{body}");
        // Nothing the conversation shows names them.
        let (_, listed) = call(f, Method::GET, "/api/v1/conversations", &cookie, (None, None)).await?;
        let (_, events) = call(
            f,
            Method::GET,
            &format!("/api/v1/conversations/{id}/events?after=0&limit=10"),
            &cookie,
            (None, None),
        )
        .await?;
        for shown in [listed, events] {
            let text = shown.to_string();
            assert!(!text.contains("approval") && !text.contains("parked"), "{text}");
        }
        Ok(())
    })
    .await
}

/// A frame the runtime of revision 8 stored: its run parked for an approval.
fn parked(seq: i64) -> Value {
    json!({"type": "event", "seq": seq, "run_id": "r-1", "step": null, "ts": "2026-09-01T00:00:00Z",
        "event": {"type": "run.parked", "run_id": "r-1", "request_id": "req-1"}})
}

/// An event of run `r-1` at `seq`, as a revision-8 runtime stored it.
fn stored(seq: i64, event: &Value) -> Value {
    json!({"type": "event", "seq": seq, "run_id": "r-1", "step": null, "ts": "2026-09-01T00:00:00Z", "event": event})
}

/// A `permission.request` as a revision-8 runtime stored it.
fn request(seq: i64) -> Value {
    json!({"type": "permission.request", "seq": seq, "request_id": "req-1", "action_hash": "ab".repeat(32),
        "tool": "shell", "summary": "Run a command", "risk": "medium", "expires_at": "2099-01-01T00:00:00Z"})
}

/// Stores frames for a conversation, one per sequence from 1, with their lifecycle column as the event store keeps it.
async fn history(
    pool: &PgPool,
    conversation: uuid::Uuid,
    frames: &[(Option<&str>, Value)],
) -> TestResult {
    for (seq, (lifecycle, frame)) in (1_i64..).zip(frames) {
        sqlx::query(
            "INSERT INTO events (conversation_id, seq, run_id, ts, frame, lifecycle) VALUES ($1, $2, 'r-1', CURRENT_TIMESTAMP, $3, $4)",
        )
        .bind(conversation)
        .bind(seq)
        .bind(frame)
        .bind(lifecycle)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// The tasks an installation has when it upgrades.
struct Tasks {
    owner: uuid::Uuid,
    /// Its run parked for an approval, with a message held behind it.
    parked: uuid::Uuid,
    /// Its run asked for an approval and had not parked yet.
    asking: uuid::Uuid,
    /// Its run's approval was answered; the run goes on.
    answered: uuid::Uuid,
    finished: uuid::Uuid,
}

/// The migrations before `version`.
fn before(version: i64) -> sqlx::migrate::Migrator {
    sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            tqa_gateway::MIGRATOR
                .migrations
                .iter()
                .filter(|migration| migration.version < version)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    }
}

/// The migrations before the removal, then an installation's tasks as revision 8 left them.
async fn before_removal(pool: &PgPool) -> Result<Tasks> {
    let removal = tqa_gateway::MIGRATOR
        .migrations
        .iter()
        .find(|migration| migration.description.contains("approvals removed"))
        .map(|migration| migration.version)
        .ok_or("no removal migration")?;
    // Conversations are made the way the oldest schema made them; a later migration gives each its workspace.
    before(26).run(pool).await?;
    let owner = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO users (id,email,password_hash,role) VALUES ($1,'old@example.invalid','fixture-hash','admin')")
        .bind(owner)
        .execute(pool)
        .await?;
    let tasks = Tasks {
        owner,
        parked: uuid::Uuid::now_v7(),
        asking: uuid::Uuid::now_v7(),
        answered: uuid::Uuid::now_v7(),
        finished: uuid::Uuid::now_v7(),
    };
    for task in [tasks.parked, tasks.asking, tasks.answered, tasks.finished] {
        sqlx::query("INSERT INTO conversations (id,owner_id,title,model_route,backend,workspace_dir) VALUES ($1,$2,'Task','local','docker',$3)")
            .bind(task)
            .bind(owner)
            .bind(format!("/storage/workspaces/{task}"))
            .execute(pool)
            .await?;
    }
    before(removal).run(pool).await?;
    let started = (
        Some("run.started"),
        stored(1, &json!({"type": "run.started", "run_id": "r-1"})),
    );
    history(
        pool,
        tasks.parked,
        &[started.clone(), (Some("run.parked"), parked(2))],
    )
    .await?;
    history(pool, tasks.asking, &[started.clone(), (None, request(2))]).await?;
    let resolved = stored(
        3,
        &json!({"type": "permission.resolved", "request_id": "req-1", "action_hash": "ab".repeat(32),
        "decision": "allow_once"}),
    );
    history(
        pool,
        tasks.answered,
        &[started.clone(), (None, request(2)), (None, resolved)],
    )
    .await?;
    let completed = stored(2, &json!({"type": "run.completed", "run_id": "r-1"}));
    history(
        pool,
        tasks.finished,
        &[started, (Some("run.completed"), completed)],
    )
    .await?;
    held(pool, &tasks).await?;
    Ok(tasks)
}

/// A message sent while the parked run waited, held behind it (GW-I-129).
async fn held(pool: &PgPool, tasks: &Tasks) -> TestResult {
    let frame = json!({"type": "user.message", "text": "also this", "attachments": []}).to_string();
    sqlx::query(
        "INSERT INTO idempotency (key,user_id,route,response_status,response_body,conversation_id,queued_frame,execution_status,held,created_at) \
         VALUES ('held-1',$1,$2,202,'\\x7b7d',$3,$4,'queued',true,CURRENT_TIMESTAMP - interval '1 minute')",
    )
    .bind(tasks.owner)
    .bind(format!("/api/v1/conversations/{}/messages", tasks.parked))
    .bind(tasks.parked)
    .bind(frame.into_bytes())
    .execute(pool)
    .await?;
    Ok(())
}

/// The last stored frame of `task`, parsed, with its lifecycle column.
async fn last(pool: &PgPool, task: uuid::Uuid) -> Result<(Option<String>, ServerFrame)> {
    let (lifecycle, frame): (Option<String>, Value) = sqlx::query_as(
        "SELECT lifecycle, frame FROM events WHERE conversation_id=$1 ORDER BY seq DESC LIMIT 1",
    )
    .bind(task)
    .fetch_one(pool)
    .await?;
    Ok((lifecycle, ServerFrame::parse(&serde_json::to_vec(&frame)?)?))
}

/// Asserts that `task`'s run `r-1` ended with the upgrade's notice at `seq`.
async fn ended(pool: &PgPool, task: uuid::Uuid, at: u64) -> TestResult {
    let (lifecycle, frame) = last(pool, task).await?;
    let ServerFrame::Event {
        seq,
        run_id,
        event:
            Event::RunFailed {
                run_id: failed,
                reason,
                code,
                ..
            },
        ..
    } = frame
    else {
        return Err(format!("not a run.failed frame: {frame:?}").into());
    };
    assert_eq!(lifecycle.as_deref(), Some("run.failed"));
    assert_eq!(
        (
            seq,
            run_id.as_ref().map(tqa_types::RunId::as_str),
            failed.as_str()
        ),
        (tqa_types::Seq::from(at), Some("r-1"), "r-1")
    );
    assert_eq!(
        (reason.as_str(), code),
        (
            "approvals were removed; send a message to continue",
            ErrorCode::ApprovalsRemoved
        )
    );
    Ok(())
}

async fn events(pool: &PgPool, task: uuid::Uuid) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM events WHERE conversation_id=$1")
            .bind(task)
            .fetch_one(pool)
            .await?,
    )
}

// TEST: GW-I-135
#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_135_an_upgrade_ends_runs_that_waited_for_an_approval(pool: PgPool) -> TestResult {
    support::require_database()?;
    let tasks = before_removal(&pool).await?;
    tqa_gateway::MIGRATOR.run(&pool).await?;
    // A parked run, and one whose request was still open, end failed with the notice at the next sequence.
    ended(&pool, tasks.parked, 3).await?;
    ended(&pool, tasks.asking, 3).await?;
    // An answered request, and a finished run, are left as they were.
    assert_eq!(
        (
            events(&pool, tasks.answered).await?,
            events(&pool, tasks.finished).await?
        ),
        (3, 2)
    );
    // The message held behind the parked run is dropped with it once the gateway runs, and never reaches a runtime.
    support::run(pool.clone(), async |f| {
        let parked: tqa_types::ConversationId = tasks.parked.to_string().parse()?;
        let mut runtime = f.runtime_for(&parked).await?;
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let state: (String, Option<String>) = sqlx::query_as(
                    "SELECT execution_status, execution_error FROM idempotency WHERE key='held-1'",
                )
                .fetch_one(&f.pool)
                .await?;
                if state.0 != "queued" {
                    return Ok::<_, Box<dyn std::error::Error>>(state);
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await??
        .eq(&("failed".to_owned(), Some("run_failed".to_owned())))
        .then_some(())
        .ok_or("the held message was not dropped as run_failed")?;
        assert!(
            runtime
                .try_action(std::time::Duration::from_millis(300))
                .await?
                .is_none()
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-135
#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_135_an_upgrade_strips_the_permission_settings(pool: PgPool) -> TestResult {
    support::require_database()?;
    let tasks = before_removal(&pool).await?;
    let profile = json!({"name": "team", "display_name": "Team", "skills": "all", "mcp_servers": [],
        "connectors": [], "limits": {}, "policy": "read_only", "rules": [{"tool": "shell", "decision": "deny"}]});
    sqlx::query("INSERT INTO agent_profiles (id,owner_id,name,document,last_set_by) VALUES ($1,NULL,'team',$2,'admin'), ($3,$4,'mine',$2,'owner')")
        .bind(uuid::Uuid::now_v7())
        .bind(&profile)
        .bind(uuid::Uuid::now_v7())
        .bind(tasks.owner)
        .execute(&pool)
        .await?;
    sqlx::query(
        "UPDATE conversations SET profile=$1, profile_id=$2, profile_version=1 WHERE id=$3",
    )
    .bind(&profile)
    .bind(uuid::Uuid::now_v7())
    .bind(tasks.parked)
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO connectors (name,document) VALUES ('tracker', $1)")
        .bind(json!({"name": "tracker", "url": "https://mcp.tracker.example/mcp", "class": "network"}))
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO settings (id,key,value,last_set_by) VALUES ($1,'sandbox.park_after_seconds',120,'admin')")
        .bind(uuid::Uuid::now_v7())
        .execute(&pool)
        .await?;
    tqa_gateway::MIGRATOR.run(&pool).await?;
    stripped(&pool).await?;
    // The approval tables, the park timer and the workspace push setting are gone.
    let columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema=current_schema() AND \
         ((table_name='workspaces' AND column_name LIKE 'push_%') OR column_name='park_after_seconds')",
    )
    .fetch_one(&pool)
    .await?;
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables WHERE table_schema=current_schema() AND \
         table_name IN ('permission_bindings','connector_tool_hints')",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!((columns, tables), (0, 0));
    Ok(())
}

/// Policy, rules and class are gone from what is stored; the shared profile, the connector and the removed limit are
/// audited, a personal profile is not.
async fn stripped(pool: &PgPool) -> TestResult {
    let documents: Vec<Value> = sqlx::query_scalar(
        "SELECT document FROM agent_profiles UNION ALL SELECT profile FROM conversations WHERE profile IS NOT NULL \
         UNION ALL SELECT document FROM connectors",
    )
    .fetch_all(pool)
    .await?;
    assert_eq!(documents.len(), 4);
    for document in &documents {
        for key in ["policy", "rules", "class"] {
            assert!(document.get(key).is_none(), "{key} in {document}");
        }
    }
    let audited: Vec<Audit> = sqlx::query_as(
        "SELECT actor_kind, area, item, before_version, after_version FROM admin_audit ORDER BY area",
    )
    .fetch_all(pool)
    .await?;
    let row = |area: &str, item: &str, after: Option<i64>| -> Audit {
        ("seed".into(), area.into(), item.into(), Some(1), after)
    };
    assert_eq!(
        audited,
        [
            row("connectors", "tracker", Some(2)),
            row("limits", "sandbox.park_after_seconds", None),
            row("profiles", "team", Some(2)),
        ],
        "one row per shared setting changed; a personal profile is its owner's"
    );
    Ok(())
}

// TEST: GW-I-135
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_135_a_runtime_that_sends_a_retired_frame_is_refused(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, _) = f.admin().await?;
        let retired = [
            json!({"type": "permission.request", "seq": 1}),
            json!({"type": "event", "seq": 1, "run_id": null, "step": null, "ts": "2026-09-01T00:00:00Z",
                "event": {"type": "permission.resolved"}}),
            json!({"type": "event", "seq": 1, "run_id": null, "step": null, "ts": "2026-09-01T00:00:00Z",
                "event": {"type": "permission.expired"}}),
            parked(1),
            json!({"type": "error", "seq": null, "code": "permission_binding", "message": "old", "retryable": false}),
        ];
        // One task: after each refusal its runtime redials, as a refused runtime does.
        let conversation = f.conversation(owner.id).await?;
        let id = uuid::Uuid::parse_str(conversation.id.as_str())?;
        for frame in retired {
            let mut runtime = f.runtime(&conversation).await?;
            runtime
                .send(&ServerFrame::parse(&serde_json::to_vec(&frame)?)?)
                .await?;
            runtime.error().await?;
            runtime.closed().await?;
            let stored: i64 =
                sqlx::query_scalar("SELECT count(*) FROM events WHERE conversation_id=$1")
                    .bind(id)
                    .fetch_one(&f.pool)
                    .await?;
            assert_eq!(stored, 0, "nothing of {frame} was stored");
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-139
/// task.titled is the gateway's own notice to clients (PROTO-U-028); a runtime that sends one is refused, as for
/// message.queue, and nothing is stored.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_139_a_runtime_that_sends_task_titled_is_refused(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, _) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = uuid::Uuid::parse_str(conversation.id.as_str())?;
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&ServerFrame::TaskTitled {
                title: "Sales CSV cleanup".to_owned(),
            })
            .await?;
        runtime.error().await?;
        runtime.closed().await?;
        let stored: i64 =
            sqlx::query_scalar("SELECT count(*) FROM events WHERE conversation_id=$1")
                .bind(id)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(stored, 0, "nothing of task.titled was stored");
        let title: String = sqlx::query_scalar("SELECT title FROM conversations WHERE id=$1")
            .bind(id)
            .fetch_one(&f.pool)
            .await?;
        assert_ne!(title, "Sales CSV cleanup", "a runtime cannot set the title");
        Ok(())
    })
    .await
}

// TEST: GW-I-139
/// team.changed is the gateway's own notice to clients (PROTO-U-034, ADR-0034's S6 contract); a runtime that sends one
/// is refused, as for task.titled, and nothing is stored.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_139_a_runtime_that_sends_team_changed_is_refused(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, _) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = uuid::Uuid::parse_str(conversation.id.as_str())?;
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&ServerFrame::TeamChanged {
                team: uuid::Uuid::now_v7().to_string(),
                version: 1,
                last_seq: 0,
            })
            .await?;
        runtime.error().await?;
        runtime.closed().await?;
        let stored: i64 =
            sqlx::query_scalar("SELECT count(*) FROM events WHERE conversation_id=$1")
                .bind(id)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(stored, 0, "nothing of team.changed was stored");
        Ok(())
    })
    .await
}
