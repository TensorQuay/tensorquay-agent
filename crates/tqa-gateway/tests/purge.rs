//! Delete means gone (PRD §12 item 18): a deleted task or workspace answers `404` at once and is purged in the
//! background within minutes; only a content-free audit row stays.
//! - GW-I-182: the purge never touches a live task: an active one, or a deleted one with a sandbox, until it goes.
//! - GW-I-183: a purged task leaves no row naming it but its detached usage and the audit row: transcript (inline and
//!   large frames), held messages, stored answers (create, rename, its channel post), snapshots, lease, files; its
//!   channel post stays with no author name, and its usage still counts for its owner.
//! - GW-I-184: a deleted task answers `404` to every read, stream, files call and action at the delete, and an open
//!   stream ends; a repeat delete answers `204` and records nothing more until the purge, then `404`.
//! - GW-I-185: a stored answer about a task never hides its failure: a rename after a failed start keeps the notice.
//! - GW-I-186: a retried create after its task was deleted and purged makes a new task.
//! - GW-I-187: a purged workspace leaves no files, changes, snapshots, shares or row, and its forks stop naming it.
//! - GW-I-188: the purge is bounded: at most 500 rows a statement, on one connection; resumable after a failed step.
//! - GW-I-189: the purge is observable: targets counted by outcome, and the oldest waiting deletion's age.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call, run};
use tqa_gateway::OpenPoint;
use tqa_types::{ConversationId, ErrorCode, SandboxState, ServerFrame};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn uuid(id: &ConversationId) -> Result<Uuid> {
    Ok(Uuid::parse_str(id.as_str())?)
}

async fn delete(f: &Fixture, cookie: &str, path: &str) -> Result<u16> {
    Ok(call(f, Method::DELETE, path, cookie, (None, None)).await?.0)
}

/// How many rows `sql` (one `$1`) counts.
async fn count(f: &Fixture, sql: &'static str, id: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(sql).bind(id).fetch_one(&f.pool).await?)
}

/// How many rows in every table that names a task still name `id` (usage and the audit are checked on their own).
async fn naming(f: &Fixture, id: Uuid) -> Result<i64> {
    count(
        f,
        "SELECT (SELECT count(*) FROM conversations WHERE id=$1) + (SELECT count(*) FROM events WHERE conversation_id=$1) \
         + (SELECT count(*) FROM idempotency WHERE conversation_id=$1 OR about_conversation=$1) \
         + (SELECT count(*) FROM workspace_snapshots WHERE conversation_id=$1) \
         + (SELECT count(*) FROM runtime_tokens WHERE conversation_id=$1) \
         + (SELECT count(*) FROM runtime_token_reservations WHERE conversation_id=$1) \
         + (SELECT count(*) FROM channel_members WHERE conversation_id=$1) \
         + (SELECT count(*) FROM workspaces WHERE lease_conversation_id=$1)",
        id,
    )
    .await
}

async fn audits(
    f: &Fixture,
    area: &str,
    item: Uuid,
) -> Result<Vec<(String, Option<Uuid>, String)>> {
    Ok(sqlx::query_as(
        "SELECT actor_kind, actor_user_id, action FROM admin_audit WHERE area=$1 AND item=$2 ORDER BY at",
    )
    .bind(area)
    .bind(item.to_string())
    .fetch_all(&f.pool)
    .await?)
}

async fn metrics(f: &Fixture) -> Result<String> {
    use tower::ServiceExt;
    let response = f
        .gateway
        .metrics_router()
        .oneshot(axum::http::Request::get("/metrics").body(axum::body::Body::empty())?)
        .await?;
    Ok(String::from_utf8(
        http_body_util::BodyExt::collect(response.into_body())
            .await?
            .to_bytes()
            .to_vec(),
    )?)
}

/// A sandbox record, as the manager keeps for a live sandbox; no backend sandbox stands behind it.
async fn sandbox_record(f: &Fixture, id: Uuid) -> TestResult {
    sqlx::query("INSERT INTO sandbox_instances (conversation_id, sandbox_id, phase) VALUES ($1, $2, 'live')")
        .bind(id)
        .bind(format!("sandbox-{id}"))
        .execute(&f.pool)
        .await?;
    Ok(())
}

// TEST: GW-I-182
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_182_the_purge_never_touches_a_live_task(pool: PgPool) -> TestResult {
    run(pool, async |f| {
        f.gateway.purge_hold(true);
        let (owner, cookie) = f.admin().await?;
        let active = f.conversation(owner.id).await?;
        let sandboxed = f.conversation(owner.id).await?;
        let plain = f.conversation(owner.id).await?;
        for task in [&active, &sandboxed, &plain] {
            f.gateway
                .events()
                .append(&task.id, support::frame(1, "kept"))
                .await?;
        }
        for task in [&sandboxed, &plain] {
            assert_eq!(
                delete(f, &cookie, &format!("/api/v1/conversations/{}", task.id)).await?,
                204
            );
        }
        sandbox_record(f, uuid(&sandboxed.id)?).await?;
        let before = naming(f, uuid(&active.id)?).await?;
        assert!(before >= 2, "the active task has its row and its event");

        let cycle = f.gateway.purge_now().await?;
        assert_eq!(
            (cycle.tasks, cycle.deferred, cycle.failed),
            (1, 1, 0),
            "{cycle:?}"
        );
        assert_eq!(
            naming(f, uuid(&plain.id)?).await?,
            0,
            "the deleted task without a sandbox is purged"
        );
        assert_eq!(
            count(
                f,
                "SELECT count(*) FROM events WHERE conversation_id=$1",
                uuid(&sandboxed.id)?
            )
            .await?,
            1,
            "a deleted task with a sandbox keeps everything while it runs"
        );
        assert_eq!(
            naming(f, uuid(&active.id)?).await?,
            before,
            "the active task is untouched"
        );

        // Its sandbox gone, the next cycle purges it; the active task is still untouched.
        sqlx::query("DELETE FROM sandbox_instances WHERE conversation_id=$1")
            .bind(uuid(&sandboxed.id)?)
            .execute(&f.pool)
            .await?;
        let cycle = f.gateway.purge_now().await?;
        assert_eq!((cycle.tasks, cycle.deferred), (1, 0), "{cycle:?}");
        assert_eq!(naming(f, uuid(&sandboxed.id)?).await?, 0);
        assert_eq!(naming(f, uuid(&active.id)?).await?, before);
        assert_eq!(
            f.gateway
                .events()
                .read(&active.id, 0.into(), 10)
                .await?
                .len(),
            1
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-183
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_183_a_purged_task_leaves_nothing_but_its_usage_and_the_audit(
    pool: PgPool,
) -> TestResult {
    run(pool, async |f| {
        f.gateway.purge_hold(true);
        let (owner, cookie) = f.admin().await?;
        // Created and renamed with keys: both stored answers carry the title.
        let (status, created) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (Some(json!({"title": "Quarterly secret plan", "model_route": "local"})), Some("create-plan")),
        )
        .await?;
        assert_eq!(status, 201, "{created}");
        let id: ConversationId = created["id"].as_str().ok_or("id")?.parse()?;
        let task = uuid(&id)?;
        let (status, renamed) = call(
            f,
            Method::PATCH,
            &format!("/api/v1/conversations/{id}"),
            &cookie,
            (Some(json!({"title": "Renamed secret plan"})), Some("rename-plan")),
        )
        .await?;
        assert_eq!(status, 200, "{renamed}");
        let workspace = Uuid::parse_str(created["workspace_id"].as_str().ok_or("workspace")?)?;

        // A channel the task is a member of.
        let (status, channel) = call(
            f,
            Method::POST,
            "/api/v1/channels",
            &cookie,
            (Some(json!({"name": "plans", "purpose": "plans", "rule": "human"})), None),
        )
        .await?;
        assert_eq!(status, 201, "{channel}");
        let channel = channel["id"].as_str().ok_or("channel")?.to_owned();
        let (status, _) = call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{channel}/members/conversation/{id}"),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        assert_eq!(status, 200);
        // A channel post from its run, with a key, while its runtime is connected; then its sandbox retires idle with
        // the runtime still connected. Dropped at once, the runtime could be gone before the start's readiness check
        // (one poll every 100 ms on the fake backend) saw it, and the start then waits out its real-time ready bound
        // (30 s) instead of retiring on the fake clock: 5 runs in 9 did, against none with the runtime held 250 ms.
        let runtime = f.runtime_for(&id).await?;
        let token = secrecy::ExposeSecret::expose_secret(&f.backend.token(&id).ok_or("token")?).to_owned();
        let response = f
            .client
            .post(f.url(&format!("/runtime/channels/{channel}/messages")))
            .bearer_auth(&token)
            .header("idempotency-key", "post-1")
            .json(&json!({"body": {"type": "text", "text": "the plan is ready"}}))
            .send()
            .await?;
        assert_eq!(response.status(), 201);
        f.clock.freeze();
        tokio::time::timeout(support::DEADLINE, async {
            while count(f, "SELECT count(*) FROM sandbox_instances WHERE conversation_id=$1", task).await? > 0 {
                f.clock.advance(std::time::Duration::from_mins(10));
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        drop(runtime);

        // Its transcript: an inline frame and a large one kept as a file; a held message; a snapshot; the lease; usage.
        f.gateway.events().append(&id, support::frame(1, "inline words")).await?;
        let storage = f.config_storage_root();
        let blob = storage.join("blobs").join(task.to_string());
        std::fs::create_dir_all(&blob)?;
        std::fs::write(blob.join("2"), b"{\"large\":\"frame\"}")?;
        sqlx::query("INSERT INTO events (conversation_id, seq, ts, blob_key) VALUES ($1, 2, CURRENT_TIMESTAMP, $2)")
            .bind(task)
            .bind(format!("{task}/2"))
            .execute(&f.pool)
            .await?;
        sqlx::query(
            "INSERT INTO idempotency (key,user_id,route,response_status,response_body,conversation_id,queued_frame,execution_status,held) \
             VALUES ('held-1',$2,'POST /api/v1/conversations/'||$1||'/messages',202,'{}'::bytea,$1,'{\"text\":\"held words\"}'::bytea,'queued',true)",
        )
        .bind(task)
        .bind(owner.id)
        .execute(&f.pool)
        .await?;
        let snapshot = Uuid::now_v7();
        sqlx::query("INSERT INTO workspace_snapshots (id, workspace_id, conversation_id, after_seq, files, truncated) VALUES ($1,$2,$3,0,0,false)")
            .bind(snapshot)
            .bind(workspace)
            .bind(task)
            .execute(&f.pool)
            .await?;
        let manifest = storage.join("changes").join(workspace.to_string()).join("snapshots").join(format!("{snapshot}.json"));
        std::fs::create_dir_all(manifest.parent().ok_or("parent")?)?;
        std::fs::write(&manifest, b"{}")?;
        sqlx::query("UPDATE workspaces SET lease_conversation_id=$1, lease_expires_at=CURRENT_TIMESTAMP + interval '1 hour' WHERE id=$2")
            .bind(task)
            .bind(workspace)
            .execute(&f.pool)
            .await?;
        // An older usage row, from before `user_id` was recorded.
        sqlx::query("INSERT INTO usage (id, conversation_id, route, prompt_tokens, completion_tokens) VALUES ($1,$2,'local',30,12)")
            .bind(Uuid::now_v7())
            .bind(task)
            .execute(&f.pool)
            .await?;
        let state = storage.join("runtime-state").join(task.to_string());
        std::fs::create_dir_all(&state)?;
        std::fs::write(state.join("journal"), b"journal words")?;

        assert_eq!(delete(f, &cookie, &format!("/api/v1/conversations/{id}")).await?, 204);
        assert_eq!(audits(f, "tasks", task).await?, [("user".to_owned(), Some(owner.id), "delete".to_owned())]);
        // A deleted task waits for its sandbox's teardown before its rows go: a loaded CI runner needed more than one
        // cycle for that (14 Nov 2026), so a bounded few, each without a failure.
        for _ in 0..50 {
            let cycle = f.gateway.purge_now().await?;
            assert!(cycle.failed == 0, "{cycle:?}");
            if naming(f, task).await? == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        assert_eq!(naming(f, task).await?, 0, "no row names the purged task");
        assert!(!blob.exists() && !state.exists() && !manifest.exists(), "its files are gone");
        assert!(storage.join("workspaces").join(workspace.to_string()).exists() || f.backend.mounted(&id).is_none(),
                "a task delete keeps its workspace's files");
        let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT response_body FROM idempotency").fetch_all(&f.pool).await?;
        for body in &stored {
            let text = String::from_utf8_lossy(body);
            assert!(!text.contains("secret plan") && !text.contains(id.as_str()), "a stored answer keeps it: {text}");
        }
        let post: (String, Option<String>, Value) =
            sqlx::query_as("SELECT author_id, author_name, body FROM channel_messages WHERE channel_id=$1::uuid")
                .bind(&channel)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!((post.0.as_str(), post.1, &post.2["text"]), (id.as_str(), None, &json!("the plan is ready")),
                   "the channel's post stays, with no author name");
        // Usage stays, detached, and still counts for the owner.
        let usage: Vec<(Option<Uuid>, Option<Uuid>)> =
            sqlx::query_as("SELECT conversation_id, user_id FROM usage").fetch_all(&f.pool).await?;
        assert!(!usage.is_empty() && usage.iter().all(|row| *row == (None, Some(owner.id))), "{usage:?}");
        let (status, page) = call(f, Method::GET, "/api/v1/admin/usage?group=user", &cookie, (None, None)).await?;
        assert_eq!(status, 200, "{page}");
        assert!(page.to_string().contains(&owner.id.to_string()), "the owner's usage is still listed: {page}");
        assert_eq!(audits(f, "tasks", task).await?.len(), 1, "the audit row stays, and the purge adds none");
        Ok(())
    })
    .await
}

// TEST: GW-I-184
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_184_a_deleted_task_is_gone_at_once_and_a_repeat_delete_answers_as_the_first(
    pool: PgPool,
) -> TestResult {
    run(pool, async |f| {
        f.gateway.purge_hold(true);
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let id = &task.id;
        f.gateway
            .events()
            .append(id, support::frame(1, "words"))
            .await?;
        let mut open = f.stream(id, &cookie, 0).await?;
        let (_, first) = open.durable().await?;
        assert!(matches!(first, ServerFrame::Event { .. }));

        let path = format!("/api/v1/conversations/{id}");
        assert_eq!(delete(f, &cookie, &path).await?, 204);
        // The open stream ends: a last, non-retryable "not found" error, then nothing.
        let last = tokio::time::timeout(support::DEADLINE, async {
            loop {
                if let (
                    None,
                    ServerFrame::Error {
                        retryable, message, ..
                    },
                ) = open.next().await?
                {
                    break Ok::<_, Box<dyn std::error::Error>>((retryable, message));
                }
            }
        })
        .await??;
        assert!(!last.0 && last.1.contains("not found"), "{last:?}");
        assert!(
            tokio::time::timeout(support::DEADLINE, open.next())
                .await?
                .is_err(),
            "the stream ended"
        );
        for read in [
            path.clone(),
            format!("{path}/events"),
            format!("{path}/events?stream=1&protocol=1"),
            format!("{path}/files"),
            format!("{path}/messages/queued"),
        ] {
            assert_eq!(f.get(&read, &cookie).await?.status(), 404, "{read}");
        }
        for (method, action, body) in [
            (
                Method::POST,
                format!("{path}/messages"),
                json!({"text": "again", "attachments": []}),
            ),
            (Method::PATCH, path.clone(), json!({"title": "Back"})),
            (
                Method::PUT,
                format!("{path}/environment"),
                json!({"template": "minimal"}),
            ),
        ] {
            let (status, _) = call(
                f,
                method,
                &action,
                &cookie,
                (Some(body), Some("after-delete")),
            )
            .await?;
            assert_eq!(status, 404, "{action}");
        }
        // A retry after a timeout sees the first answer and records nothing more.
        assert_eq!(delete(f, &cookie, &path).await?, 204);
        assert_eq!(audits(f, "tasks", uuid(id)?).await?.len(), 1);
        // Someone else still can't, deleted or not.
        let (_, other) = f.member("other@example.invalid").await?;
        assert_eq!(delete(f, &other, &path).await?, 403);
        f.gateway.purge_now().await?;
        assert_eq!(
            delete(f, &cookie, &path).await?,
            404,
            "once purged, it is not found"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-184
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_184_a_stream_opened_across_the_delete_is_refused(pool: PgPool) -> TestResult {
    run(pool, async |f| {
        f.gateway.purge_hold(true);
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let path = format!("/api/v1/conversations/{}", task.id);
        // The open passes its checks, then waits; the delete lands before it subscribes.
        let hold = f.gateway.hold_stream_open(OpenPoint::Checked);
        let opening = {
            let request = f
                .client
                .get(f.url(&format!("{path}/events?stream=1&protocol=1")))
                .header("cookie", &cookie)
                .header("origin", f.origin());
            tokio::spawn(async move { request.send().await })
        };
        tokio::time::timeout(support::DEADLINE, hold.reached()).await?;
        assert_eq!(delete(f, &cookie, &path).await?, 204);
        hold.release();
        let response = tokio::time::timeout(support::DEADLINE, opening).await???;
        assert_eq!(
            response.status(),
            404,
            "a stream must never stay open on a deleted task"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-184
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_184_a_stream_that_subscribed_before_the_delete_ends(pool: PgPool) -> TestResult {
    run(pool, async |f| {
        f.gateway.purge_hold(true);
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let path = format!("/api/v1/conversations/{}", task.id);
        // The open subscribes and finds the task still there, then waits; the delete closes the node it holds.
        let hold = f.gateway.hold_stream_open(OpenPoint::Subscribed);
        let opening = {
            let request = f
                .client
                .get(f.url(&format!("{path}/events?stream=1&protocol=1")))
                .header("cookie", &cookie)
                .header("origin", f.origin());
            tokio::spawn(async move { request.send().await })
        };
        tokio::time::timeout(support::DEADLINE, hold.reached()).await?;
        assert_eq!(delete(f, &cookie, &path).await?, 204);
        hold.release();
        let response = tokio::time::timeout(support::DEADLINE, opening).await???;
        assert_eq!(
            response.status(),
            200,
            "it subscribed while the task was there"
        );
        // The stream ends by itself, its last frame a non-retryable error.
        let body = tokio::time::timeout(support::DEADLINE, response.text()).await??;
        let last: Value = body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .next_back()
            .ok_or("no frame")?
            .trim()
            .parse()?;
        assert_eq!(
            (&last["type"], &last["retryable"]),
            (&json!("error"), &json!(false)),
            "{body}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-185
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_185_a_rename_after_a_failed_start_keeps_the_failure_notice(
    pool: PgPool,
) -> TestResult {
    run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let id = &task.id;
        sqlx::query(
            "INSERT INTO idempotency (key,user_id,route,response_status,response_body,conversation_id,execution_status,execution_error) \
             VALUES ('failed-start',$2,'POST /api/v1/conversations/'||$1||'/messages',202,'{}'::bytea,$1,'failed','runtime_start_failed')",
        )
        .bind(uuid(id)?)
        .bind(owner.id)
        .execute(&f.pool)
        .await?;
        let (status, _) = call(
            f,
            Method::PATCH,
            &format!("/api/v1/conversations/{id}"),
            &cookie,
            (Some(json!({"title": "Renamed"})), Some("rename-after-failure")),
        )
        .await?;
        assert_eq!(status, 200);
        let mut later = f.stream(id, &cookie, 0).await?;
        assert_eq!(later.initial, SandboxState::Failed, "the rename's stored answer must not hide the failure");
        let (_, frame) = later.next().await?;
        assert!(
            matches!(frame, ServerFrame::Error { seq: None, code: ErrorCode::RuntimeStartFailed, .. }),
            "{frame:?}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-186
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_186_a_retried_create_after_its_task_was_purged_makes_a_new_task(
    pool: PgPool,
) -> TestResult {
    run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let body = json!({"title": "Once", "model_route": "local"});
        let (status, first) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (Some(body.clone()), Some("create-once")),
        )
        .await?;
        assert_eq!(status, 201);
        let (status, replay) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (Some(body.clone()), Some("create-once")),
        )
        .await?;
        assert_eq!(
            (status, &replay["id"]),
            (201, &first["id"]),
            "a replay before the delete answers as the first"
        );
        let path = format!(
            "/api/v1/conversations/{}",
            first["id"].as_str().ok_or("id")?
        );
        assert_eq!(delete(f, &cookie, &path).await?, 204);
        f.gateway.purge_now().await?;
        let (status, again) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (Some(body), Some("create-once")),
        )
        .await?;
        assert_eq!(status, 201, "{again}");
        assert_ne!(
            again["id"], first["id"],
            "the deleted task's answer went with it: this is a new task"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-187
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_187_a_purged_workspace_leaves_nothing_and_its_forks_stay(pool: PgPool) -> TestResult {
    run(pool, async |f| {
        f.gateway.purge_hold(true);
        let (owner, cookie) = f.admin().await?;
        let (status, created) =
            call(f, Method::POST, "/api/v1/workspaces", &cookie, (Some(json!({"name": "Old work"})), None)).await?;
        assert_eq!(status, 201, "{created}");
        let workspace = Uuid::parse_str(created["id"].as_str().ok_or("id")?)?;
        f.member("share@example.invalid").await?;
        let shared_with = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE email='share@example.invalid'")
            .fetch_one(&f.pool)
            .await?;
        // A fork naming it, a share, a snapshot by a since-deleted task, and recorded changes on disk.
        let fork = Uuid::now_v7();
        sqlx::query("INSERT INTO workspaces (id, owner_id, name, forked_from, fork_consistent) VALUES ($1,$2,'Fork',$3,true)")
            .bind(fork)
            .bind(owner.id)
            .bind(workspace)
            .execute(&f.pool)
            .await?;
        sqlx::query("INSERT INTO workspace_shares (workspace_id, user_id, access, created_by) VALUES ($1,$2,'read',$3)")
            .bind(workspace)
            .bind(shared_with)
            .bind(owner.id)
            .execute(&f.pool)
            .await?;
        let storage = f.config_storage_root();
        let changes = storage.join("changes").join(workspace.to_string());
        std::fs::create_dir_all(changes.join("blobs"))?;
        std::fs::write(changes.join("blobs").join("abc"), b"copy")?;
        let files = storage.join("workspaces").join(workspace.to_string());
        std::fs::create_dir_all(&files)?;
        std::fs::write(files.join("notes.md"), b"notes")?;

        // Never a live workspace: one in use stays whole, and a deleted one whose task is still there waits.
        let (status, live) =
            call(f, Method::POST, "/api/v1/workspaces", &cookie, (Some(json!({"name": "In use"})), None)).await?;
        assert_eq!(status, 201, "{live}");
        let live = Uuid::parse_str(live["id"].as_str().ok_or("id")?)?;
        let live_files = storage.join("workspaces").join(live.to_string());
        std::fs::create_dir_all(&live_files)?;
        std::fs::write(live_files.join("keep.md"), b"keep")?;
        let waiting = Uuid::now_v7();
        sqlx::query("INSERT INTO workspaces (id, owner_id, name, state) VALUES ($1,$2,'Waiting','deleted')")
            .bind(waiting)
            .bind(owner.id)
            .execute(&f.pool)
            .await?;
        let bound = f.conversation(owner.id).await?;
        sqlx::query("UPDATE conversations SET workspace_id=$1 WHERE id=$2")
            .bind(waiting)
            .bind(uuid(&bound.id)?)
            .execute(&f.pool)
            .await?;

        assert_eq!(delete(f, &cookie, &format!("/api/v1/workspaces/{workspace}")).await?, 204);
        assert_eq!(audits(f, "workspaces", workspace).await?, [("user".to_owned(), Some(owner.id), "delete".to_owned())]);
        let cycle = f.gateway.purge_now().await?;
        assert_eq!((cycle.workspaces, cycle.failed), (1, 0), "{cycle:?}");
        assert!(!files.exists() && !changes.exists(), "its files and recorded changes are gone");
        assert_eq!(
            count(
                f,
                "SELECT (SELECT count(*) FROM workspaces WHERE id=$1) + (SELECT count(*) FROM workspace_shares WHERE workspace_id=$1) \
                 + (SELECT count(*) FROM workspace_snapshots WHERE workspace_id=$1) + (SELECT count(*) FROM workspaces WHERE forked_from=$1)",
                workspace
            )
            .await?,
            0
        );
        assert_eq!(
            count(f, "SELECT count(*) FROM workspaces WHERE id=$1 AND forked_from IS NULL AND fork_consistent IS NULL", fork)
                .await?,
            1,
            "its fork stays, naming no origin"
        );
        assert!(live_files.join("keep.md").exists(), "a live workspace's files are untouched");
        assert_eq!(count(f, "SELECT count(*) FROM workspaces WHERE id=$1 AND state='ready'", live).await?, 1);
        assert_eq!(count(f, "SELECT count(*) FROM workspaces WHERE id=$1", waiting).await?, 1,
                   "a deleted workspace waits while a task is bound to it");
        Ok(())
    })
    .await
}

// TEST: GW-I-188
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_188_the_purge_is_bounded_and_resumes_after_a_failed_step(pool: PgPool) -> TestResult {
    run(pool, async |f| {
        f.gateway.purge_hold(true);
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let id = uuid(&task.id)?;
        sqlx::query(
            "INSERT INTO events (conversation_id, seq, ts, frame) \
             SELECT $1, n, CURRENT_TIMESTAMP, '{\"type\":\"event\"}'::jsonb FROM generate_series(1, 2000) AS n",
        )
        .bind(id)
        .execute(&f.pool)
        .await?;
        // Each statement's deleted rows and the connection it ran on, recorded by the database itself.
        sqlx::raw_sql(
            "CREATE TABLE purge_statements (n bigint, backend integer); \
             CREATE FUNCTION purge_counted() RETURNS trigger LANGUAGE plpgsql AS \
               $$BEGIN INSERT INTO purge_statements SELECT count(*), pg_backend_pid() FROM gone; RETURN NULL; END$$; \
             CREATE TRIGGER purge_counted AFTER DELETE ON events REFERENCING OLD TABLE AS gone \
               FOR EACH STATEMENT EXECUTE FUNCTION purge_counted();",
        )
        .execute(&f.pool)
        .await?;
        assert_eq!(delete(f, &cookie, &format!("/api/v1/conversations/{}", task.id)).await?, 204);

        // A step that fails: its large frames can't be removed. Files go first, so nothing else is touched.
        let blob = f.config_storage_root().join("blobs").join(id.to_string());
        std::fs::create_dir_all(blob.join("inner"))?;
        std::fs::write(blob.join("inner").join("1"), b"frame")?;
        set_mode(&blob, 0o500)?;
        let cycle = f.gateway.purge_now().await;
        set_mode(&blob, 0o700)?;
        let cycle = cycle?;
        assert_eq!((cycle.tasks, cycle.failed), (0, 1), "{cycle:?}");
        assert_eq!(count(f, "SELECT count(*) FROM events WHERE conversation_id=$1", id).await?, 2000);

        // The next cycle finishes it, in statements of at most 500 rows, on one connection.
        let cycle = f.gateway.purge_now().await?;
        assert_eq!((cycle.tasks, cycle.failed), (1, 0), "{cycle:?}");
        assert!(!blob.exists());
        let statements: Vec<i64> = sqlx::query_scalar("SELECT n FROM purge_statements WHERE n > 0").fetch_all(&f.pool).await?;
        assert_eq!(statements.iter().sum::<i64>(), 2000);
        assert!(statements.iter().all(|n| *n <= 500), "{statements:?}");
        assert!(statements.len() >= 4, "{statements:?}");
        let connections: i64 = sqlx::query_scalar("SELECT count(DISTINCT backend) FROM purge_statements WHERE n > 0")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(connections, 1, "every statement of the cycle ran on its one connection");
        Ok(())
    })
    .await
}

fn set_mode(path: &std::path::Path, mode: u32) -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

// TEST: GW-I-189
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_189_the_purge_counts_its_targets_and_reports_the_oldest_waiting(
    pool: PgPool,
) -> TestResult {
    run(pool, async |f| {
        f.gateway.purge_hold(true);
        let (owner, cookie) = f.admin().await?;
        let waiting = f.conversation(owner.id).await?;
        let plain = f.conversation(owner.id).await?;
        for task in [&waiting, &plain] {
            assert_eq!(delete(f, &cookie, &format!("/api/v1/conversations/{}", task.id)).await?, 204);
        }
        sandbox_record(f, uuid(&waiting.id)?).await?;
        sqlx::query("UPDATE conversations SET updated_at = CURRENT_TIMESTAMP - interval '10 minutes' WHERE id=$1")
            .bind(uuid(&waiting.id)?)
            .execute(&f.pool)
            .await?;
        f.gateway.purge_now().await?;
        let text = metrics(f).await?;
        assert!(text.contains("tqa_purge_targets_total{kind=\"task\",outcome=\"done\"} 1"), "{text}");
        assert!(text.contains("tqa_purge_targets_total{kind=\"task\",outcome=\"deferred\"} 1"), "{text}");
        let oldest: f64 = text
            .lines()
            .find_map(|line| line.strip_prefix("tqa_purge_oldest_seconds "))
            .ok_or("no oldest gauge")?
            .parse()?;
        assert!((590.0..1200.0).contains(&oldest), "the sandboxed task has waited ten minutes: {oldest}");
        sqlx::query("DELETE FROM sandbox_instances").execute(&f.pool).await?;
        f.gateway.purge_now().await?;
        let text = metrics(f).await?;
        assert!(text.contains("tqa_purge_oldest_seconds 0"), "nothing waits any more: {text}");
        Ok(())
    })
    .await
}
