//! GW-I-030 and SEC-014: workspaces as owned entities that conversations bind to, shared read or write.
#[path = "support/fixture.rs"]
mod support;

use reqwest::{Method, Response};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::ConversationId;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const REPOSITORY: &str = "https://example.invalid/team/project.git";

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn json(response: Response) -> Result<(u16, Value)> {
    let status = response.status().as_u16();
    let bytes = response.bytes().await?;
    Ok((
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        },
    ))
}

async fn code(response: Response) -> Result<(u16, String)> {
    let (status, body) = json(response).await?;
    Ok((
        status,
        body["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    ))
}

async fn workspace(f: &Fixture, cookie: &str, body: Value) -> Result<Value> {
    let (status, created) = json(
        call(f, cookie, Method::POST, "/api/v1/workspaces")
            .json(&body)
            .send()
            .await?,
    )
    .await?;
    assert_eq!(status, 201, "{created}");
    Ok(created)
}

async fn task(f: &Fixture, cookie: &str, key: &str, workspace: Option<&str>) -> Result<Response> {
    let mut body = json!({"title":"Task","model_route":"local"});
    if let Some(workspace) = workspace {
        body["workspace_id"] = json!(workspace);
    }
    Ok(call(f, cookie, Method::POST, "/api/v1/conversations")
        .header("idempotency-key", key)
        .json(&body)
        .send()
        .await?)
}

async fn task_id(response: Response) -> Result<(ConversationId, Value)> {
    let (status, body) = json(response).await?;
    assert_eq!(status, 201, "{body}");
    Ok((body["id"].as_str().ok_or("id")?.parse()?, body))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_tasks_bind_to_a_workspace_and_its_directory_is_what_the_sandbox_mounts(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let created = workspace(f, &cookie, json!({"name":"Project"})).await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        assert_eq!(
            (created["name"].as_str(), created["access"].as_str()),
            (Some("Project"), Some("owner"))
        );
        // Two tasks on one workspace; a task without a workspace gets a new empty one, as before.
        let (first, body) = task_id(task(f, &cookie, "first", Some(&id)).await?).await?;
        assert_eq!(body["workspace_id"], id.as_str());
        let (second, _) = task_id(task(f, &cookie, "second", Some(&id)).await?).await?;
        let (own, own_body) = task_id(task(f, &cookie, "own", None).await?).await?;
        assert_ne!(own_body["workspace_id"], id.as_str());
        let listed = json(
            call(f, &cookie, Method::GET, "/api/v1/workspaces")
                .send()
                .await?,
        )
        .await?
        .1;
        assert_eq!(listed["items"].as_array().map(Vec::len), Some(2));
        // Rename.
        let renamed = json(
            call(
                f,
                &cookie,
                Method::PATCH,
                &format!("/api/v1/workspaces/{id}"),
            )
            .json(&json!({"name":"Renamed"}))
            .send()
            .await?,
        )
        .await?;
        assert_eq!(
            (renamed.0, renamed.1["name"].as_str()),
            (200, Some("Renamed"))
        );
        // The sandbox mounts the workspace's directory, shared by both tasks, not a directory per task.
        f.runtime_for(&first).await?;
        let mounted = f.backend.mounted(&first).ok_or("mounted")?;
        assert!(mounted.ends_with(&id), "{}", mounted.display());
        f.forget_runtime(&first)?;
        // The second task mounts the same directory once the first's sandbox is gone (the lease; workspace_lease.rs). A
        // task on another workspace mounts another directory, alongside.
        let _ = &second;
        f.gateway.sandboxes().ensure(&own).await?;
        assert_ne!(f.backend.mounted(&own), Some(mounted));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_a_workspace_an_earlier_release_made_from_git_is_still_cloned_by_its_first_sandbox(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        // A new workspace can't name a git URL any more (GW-I-136); one made before the upgrade still has its source.
        let created = workspace(f, &cookie, json!({"name":"From git"})).await?;
        sqlx::query("UPDATE workspaces SET source_url=$1 WHERE id=$2::uuid")
            .bind(REPOSITORY)
            .bind(created["id"].as_str().ok_or("id")?)
            .execute(&f.pool)
            .await?;
        let (id, _) = task_id(task(f, &cookie, "git", created["id"].as_str()).await?).await?;
        let runtime = f.runtime_for(&id).await?;
        assert_eq!(runtime.config["workspace"]["clone"]["url"], REPOSITORY);
        let (plain, _) = task_id(task(f, &cookie, "plain", None).await?).await?;
        f.forget_runtime(&id)?;
        let other = f.runtime_for(&plain).await?;
        assert!(
            other.config["workspace"]
                .get("clone")
                .is_none_or(Value::is_null)
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_sharing_grants_read_or_write_and_everyone_else_is_refused(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, owner) = f.admin().await?;
        let (reader, reader_cookie) = f.member("reader@example.invalid").await?;
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        let created = workspace(f, &owner, json!({"name":"Shared"})).await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        let base = format!("/api/v1/workspaces/{id}");
        let share = format!("{base}/shares/{}", reader.id);
        // Nobody but the owner, before sharing: every entry is 403 (SEC-014).
        for cookie in [&reader_cookie, &stranger] {
            assert_eq!(
                code(call(f, cookie, Method::GET, &base).send().await?)
                    .await?
                    .0,
                403
            );
            assert_eq!(
                code(task(f, cookie, "bind-early", Some(&id)).await?).await?,
                (403, "forbidden".into())
            );
            assert_eq!(
                code(
                    call(f, cookie, Method::PATCH, &base)
                        .json(&json!({"name":"x"}))
                        .send()
                        .await?
                )
                .await?
                .0,
                403
            );
            assert_eq!(
                code(call(f, cookie, Method::DELETE, &base).send().await?)
                    .await?
                    .0,
                403
            );
            assert_eq!(
                code(
                    call(f, cookie, Method::PUT, &share)
                        .json(&json!({"access":"write"}))
                        .send()
                        .await?
                )
                .await?
                .0,
                403
            );
        }
        // Read: visible, but no binding and no owner actions.
        assert_eq!(
            json(
                call(f, &owner, Method::PUT, &share)
                    .json(&json!({"access":"read"}))
                    .send()
                    .await?
            )
            .await?
            .0,
            200
        );
        let seen = json(call(f, &reader_cookie, Method::GET, &base).send().await?)
            .await?
            .1;
        assert_eq!(
            (seen["name"].as_str(), seen["access"].as_str()),
            (Some("Shared"), Some("read"))
        );
        assert_eq!(
            code(task(f, &reader_cookie, "bind-read", Some(&id)).await?).await?,
            (403, "forbidden".into())
        );
        assert_eq!(
            code(
                call(f, &reader_cookie, Method::PATCH, &base)
                    .json(&json!({"name":"x"}))
                    .send()
                    .await?
            )
            .await?
            .0,
            403
        );
        let listed = json(
            call(f, &reader_cookie, Method::GET, "/api/v1/workspaces")
                .send()
                .await?,
        )
        .await?
        .1;
        assert!(
            listed["items"]
                .as_array()
                .ok_or("items")?
                .iter()
                .any(|item| item["id"] == id.as_str())
        );
        // Write: the reader may bind their own task.
        assert_eq!(
            json(
                call(f, &owner, Method::PUT, &share)
                    .json(&json!({"access":"write"}))
                    .send()
                    .await?
            )
            .await?
            .0,
            200
        );
        let (bound, _) = task_id(task(f, &reader_cookie, "bind-write", Some(&id)).await?).await?;
        let shares = json(
            call(f, &owner, Method::GET, &format!("{base}/shares"))
                .send()
                .await?,
        )
        .await?
        .1;
        assert_eq!(shares["items"][0]["access"], "write");
        // The owner cannot read the reader's task: sharing a workspace never shares transcripts.
        assert_eq!(
            code(
                f.get(
                    &format!("/api/v1/conversations/{bound}/events?after=0&limit=1"),
                    &owner
                )
                .await?
            )
            .await?
            .0,
            403
        );
        // Unshared: refused again.
        assert_eq!(
            call(f, &owner, Method::DELETE, &share)
                .send()
                .await?
                .status(),
            204
        );
        assert_eq!(
            code(call(f, &reader_cookie, Method::GET, &base).send().await?)
                .await?
                .0,
            403
        );
        assert_eq!(
            code(task(f, &reader_cookie, "bind-after", Some(&id)).await?)
                .await?
                .0,
            403
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_admins_see_workspace_metadata_but_never_names_or_content(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let created = workspace(f, &member, json!({"name":"Private workspace"})).await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        let all = json(
            call(f, &admin, Method::GET, "/api/v1/workspaces?all=true")
                .send()
                .await?,
        )
        .await?
        .1;
        let item = all["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|item| item["id"] == id.as_str())
            .ok_or("listed")?
            .clone();
        assert!(item.get("name").is_none_or(Value::is_null), "{item}");
        assert!(!all.to_string().contains("Private workspace"));
        assert_eq!(item["state"], "ready");
        // No binding, no rename, no share, no delete through the admin role.
        assert_eq!(
            code(task(f, &admin, "admin-bind", Some(&id)).await?)
                .await?
                .0,
            403
        );
        assert_eq!(
            code(
                call(
                    f,
                    &admin,
                    Method::PATCH,
                    &format!("/api/v1/workspaces/{id}")
                )
                .json(&json!({"name":"x"}))
                .send()
                .await?
            )
            .await?
            .0,
            403
        );
        assert_eq!(
            code(
                call(
                    f,
                    &admin,
                    Method::DELETE,
                    &format!("/api/v1/workspaces/{id}")
                )
                .send()
                .await?
            )
            .await?
            .0,
            403
        );
        let member_all = call(f, &member, Method::GET, "/api/v1/workspaces?all=true")
            .send()
            .await?;
        assert_eq!(code(member_all).await?.0, 403);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_delete_refuses_bound_tasks_and_live_sandboxes_and_removes_the_directory(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let created = workspace(f, &cookie, json!({"name":"Doomed"})).await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        let base = format!("/api/v1/workspaces/{id}");
        let (bound, _) = task_id(task(f, &cookie, "bound", Some(&id)).await?).await?;
        f.clock.freeze();
        // Keep the runtime connected: a dropped link would leave the worker waiting for a registration.
        let _runtime = f.runtime_for(&bound).await?;
        let directory = f.backend.mounted(&bound).ok_or("mounted")?;
        assert!(directory.is_dir());
        // A delete whose statement waits past lock_timeout answers 503 store_unavailable, retryable by the error
        // contract: on CI's loaded runner the first one did (after 1,457 contracts), so each
        // delete here is sent again until it answers anything else.
        let delete = async |path: &str| -> Result<(u16, String)> {
            tokio::time::timeout(tqa_testkit::deadline::scaled(support::DEADLINE), async {
                loop {
                    let answer = code(call(f, &cookie, Method::DELETE, path).send().await?).await?;
                    if answer != (503, "store_unavailable".to_owned()) {
                        return Ok::<_, Box<dyn std::error::Error>>(answer);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            })
            .await?
        };
        // A bound task refuses it. (With its tasks, a kept, idle environment is retired for the delete and a run going
        // refuses it: GW-I-199, since before 17.0.)
        assert_eq!(delete(&base).await?, (409, "workspace_in_use".into()));
        let with_tasks = format!("{base}?with_conversations=true");
        // The idle sandbox retires after the idle window; the task stays. The clock moves on until it has: the worker
        // may take its idle start after any single advance, so one advance alone would race it.
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let live: i64 = sqlx::query_scalar("SELECT count(*) FROM sandbox_instances")
                    .fetch_one(&f.pool)
                    .await?;
                if live == 0 {
                    return Ok::<(), Box<dyn std::error::Error>>(());
                }
                f.clock.advance(std::time::Duration::from_mins(10));
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await??;
        assert_eq!(delete(&base).await?, (409, "workspace_in_use".into()));
        // With its task: both go, and the host directory with them.
        assert_eq!(delete(&with_tasks).await?.0, 204);
        assert!(!directory.exists(), "the host directory is removed");
        assert_eq!(
            code(call(f, &cookie, Method::GET, &base).send().await?)
                .await?
                .0,
            404
        );
        // The task went with its workspace: gone for its owner at once, as any deleted task.
        assert_eq!(
            f.get(&format!("/api/v1/conversations/{bound}"), &cookie)
                .await?
                .status(),
            404,
            "the task went with its workspace"
        );
        Ok(())
    })
    .await
}

#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_existing_tasks_keep_their_files_as_workspaces_of_the_same_id(
    pool: PgPool,
) -> TestResult {
    support::require_database()?;
    let migrator = &tqa_gateway::MIGRATOR;
    let before_workspaces = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(migrator.migrations[..25].to_vec()),
        ignore_missing: migrator.ignore_missing,
        locking: migrator.locking,
        no_tx: migrator.no_tx,
        table_name: migrator.table_name.clone(),
        create_schemas: migrator.create_schemas.clone(),
    };
    before_workspaces.run(&pool).await?;
    let owner = uuid::Uuid::now_v7();
    let task = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO users (id,email,password_hash,role) VALUES ($1,'old@example.invalid','fixture-hash','admin')")
        .bind(owner).execute(&pool).await?;
    sqlx::query("INSERT INTO conversations (id,owner_id,title,model_route,backend,workspace_dir) VALUES ($1,$2,'Old task','local','docker','/storage/workspaces/old')")
        .bind(task).bind(owner).execute(&pool).await?;
    migrator.run(&pool).await?;
    let (workspace, name, workspace_owner): (uuid::Uuid, String, uuid::Uuid) = sqlx::query_as(
        "SELECT w.id, w.name, w.owner_id FROM conversations c JOIN workspaces w ON w.id=c.workspace_id WHERE c.id=$1")
        .bind(task).fetch_one(&pool).await?;
    assert_eq!(
        (workspace, name.as_str(), workspace_owner),
        (task, "Old task", owner),
        "same id: the files stay where they are"
    );
    Ok(())
}

/// Sets the task's keep-alive pin, as its owner does.
async fn pin(f: &Fixture, cookie: &str, id: &ConversationId, pinned: bool) -> TestResult {
    let response = call(
        f,
        cookie,
        Method::PATCH,
        &format!("/api/v1/conversations/{id}"),
    )
    .header("idempotency-key", format!("pin-{pinned}"))
    .json(&json!({ "pinned": pinned }))
    .send()
    .await?;
    assert_eq!(response.status(), 200);
    Ok(())
}

/// The workspace's lease as its owner reads it.
async fn lease(f: &Fixture, cookie: &str, id: &str) -> Result<Value> {
    let (status, body) = json(
        call(f, cookie, Method::GET, &format!("/api/v1/workspaces/{id}"))
            .send()
            .await?,
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["lease"].clone())
}

/// A run's lifecycle event, as the runtime sends it.
fn lifecycle(seq: u64, event: tqa_types::Event) -> Result<tqa_types::ServerFrame> {
    Ok(tqa_types::ServerFrame::Event {
        seq: seq.into(),
        run_id: Some("run-1".parse()?),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    })
}

// TEST: GW-I-199
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_199_a_kept_idle_environment_is_retired_for_a_delete_and_an_active_run_refuses_it(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let created = workspace(f, &cookie, json!({"name":"Kept"})).await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        let with_tasks = format!("/api/v1/workspaces/{id}?with_conversations=true");
        let (bound, _) = task_id(task(f, &cookie, "kept", Some(&id)).await?).await?;
        // The idle window never passes here: an idle environment stays kept, as after a finished task.
        f.clock.freeze();
        let mut runtime = f.runtime_for(&bound).await?;
        let directory = f.backend.mounted(&bound).ok_or("mounted")?;
        runtime
            .send(&lifecycle(
                1,
                tqa_types::Event::RunStarted {
                    run_id: "run-1".parse()?,
                },
            )?)
            .await?;
        f.until_sequence(&bound, 1).await?;
        // A run going refuses it, as before; the lease says why.
        assert_eq!(
            lease(f, &cookie, &id).await?,
            json!({"conversation_id": bound.as_str(), "title": "Task", "running": true, "pinned": false})
        );
        assert_eq!(
            code(call(f, &cookie, Method::DELETE, &with_tasks).send().await?).await?,
            (409, "workspace_in_use".into())
        );
        runtime
            .send(&lifecycle(
                2,
                tqa_types::Event::RunCompleted {
                    run_id: "run-1".parse()?,
                },
            )?)
            .await?;
        f.until_sequence(&bound, 2).await?;
        // Pinned by its owner, the idle environment is kept for good: still refused, and the lease says so.
        pin(f, &cookie, &bound, true).await?;
        let pinned = lease(f, &cookie, &id).await?;
        assert_eq!((&pinned["running"], &pinned["pinned"]), (&json!(false), &json!(true)));
        assert_eq!(
            code(call(f, &cookie, Method::DELETE, &with_tasks).send().await?).await?,
            (409, "workspace_in_use".into())
        );
        pin(f, &cookie, &bound, false).await?;
        // Only a kept, idle environment holds it now: it is retired, as for a new task's handover, and the delete
        // goes on with its task.
        assert_eq!(
            call(f, &cookie, Method::DELETE, &with_tasks)
                .send()
                .await?
                .status(),
            204
        );
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let live: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM sandbox_instances WHERE conversation_id=$1",
                )
                .bind(uuid::Uuid::parse_str(bound.as_str())?)
                .fetch_one(&f.pool)
                .await?;
                if live == 0 {
                    return Ok::<(), Box<dyn std::error::Error>>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| "the kept environment was never retired")??;
        drop(runtime);
        assert_eq!(
            code(
                call(f, &cookie, Method::GET, &format!("/api/v1/workspaces/{id}"))
                    .send()
                    .await?
            )
            .await?
            .0,
            404
        );
        // Its files go with the purge once the environment has let go (within its minute's sweep; run now here), never
        // from under it.
        tokio::time::timeout(support::DEADLINE, async {
            while directory.exists() {
                f.gateway.purge_now().await?;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .await
        .map_err(|_| "the host directory was never removed")??;
        Ok(())
    })
    .await
}
