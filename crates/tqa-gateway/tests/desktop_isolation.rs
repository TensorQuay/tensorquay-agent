//! SEC-059's share-holder and fork cases at the gateway (ADR-0039 T6: a signed-in session stays with its task). The
//! browser profile lives in the sandbox's own tmpfs, never in the workspace or the state, so another task's desktop
//! starts signed out when it is a sandbox of its own: its own runtime token, its own state, and the Desktop's own
//! browser tmpfs. Checked for the task of a person the workspace is shared with (the same workspace) and for a task on
//! a fork of it, beside the owner's task whose desktop someone could have signed in on.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{DEADLINE, Fixture, TestResult, call};
use tqa_types::ConversationId;
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The Desktop template, which the tasks below are bound to (as the screen tests bind theirs).
async fn desktop_template(f: &Fixture) -> TestResult {
    sqlx::query("INSERT INTO templates (name,display_name,image,capabilities,is_default,version,last_set_by) VALUES ('desktop','Desktop',$1,'{shell,browser,desktop}',false,1,'admin') ON CONFLICT (name) DO NOTHING")
        .bind(format!("sha256:{}", "4".repeat(64)))
        .execute(&f.pool)
        .await?;
    Ok(())
}

async fn post(f: &Fixture, cookie: &str, path: &str, body: Value) -> Result<Value> {
    let (status, answer) = call(
        f,
        Method::POST,
        path,
        cookie,
        (Some(body), Some(&Uuid::now_v7().to_string())),
    )
    .await?;
    assert!((200..300).contains(&status), "{path}: {status} {answer}");
    Ok(answer)
}

/// A Desktop task on `workspace`, made by the person signed in with `cookie`.
async fn desktop_task(f: &Fixture, cookie: &str, workspace: &str) -> Result<ConversationId> {
    let task = post(
        f,
        cookie,
        "/api/v1/conversations",
        json!({"title": "Sign in", "model_route": "local", "workspace_id": workspace}),
    )
    .await?;
    let id = task["id"].as_str().ok_or("id")?;
    sqlx::query("UPDATE conversations SET template='desktop' WHERE id=$1")
        .bind(Uuid::parse_str(id)?)
        .execute(&f.pool)
        .await?;
    Ok(id.parse::<ConversationId>()?)
}

/// Forks `source` and waits (bounded) until the copy is ready.
async fn fork(f: &Fixture, cookie: &str, source: &str) -> Result<String> {
    let started = post(
        f,
        cookie,
        &format!("/api/v1/workspaces/{source}/fork"),
        json!({"name": "Copy"}),
    )
    .await?;
    let id = started["id"].as_str().ok_or("id")?.to_owned();
    tokio::time::timeout(DEADLINE * 2, async {
        loop {
            let (_, body) = call(
                f,
                Method::GET,
                &format!("/api/v1/workspaces/{id}"),
                cookie,
                (None, None),
            )
            .await?;
            if body["state"] != "forking" {
                return Ok::<_, Box<dyn std::error::Error>>(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await??;
    Ok(id)
}

/// What a task's sandbox was created with: whether it is a Desktop's, its runtime token, its workspace and its state.
fn sandbox(f: &Fixture, id: &ConversationId) -> Result<(bool, String, String, String)> {
    let desktop = f.backend.desktop(id).ok_or("no sandbox spec")?;
    let token = f
        .backend
        .token(id)
        .ok_or("no token")?
        .expose_secret()
        .to_owned();
    let (workspace, state) = f.backend.mounts(id).ok_or("no mounts")?;
    Ok((
        desktop,
        token,
        format!("{workspace:?}"),
        format!("{state:?}"),
    ))
}

// TEST: SEC-059
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_059_a_share_holder_s_and_a_fork_s_desktop_are_sandboxes_of_their_own(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        desktop_template(f).await?;
        // The installation's administrator comes first: members are made through it.
        f.admin().await?;
        let (_, owner) = f.member("owner@example.invalid").await?;
        let (_, holder) = f.member("holder@example.invalid").await?;
        let workspace = post(f, &owner, "/api/v1/workspaces", json!({"name": "Accounts"})).await?;
        let workspace = workspace["id"].as_str().ok_or("id")?.to_owned();
        let (status, body) = call(
            f,
            Method::PUT,
            &format!("/api/v1/workspaces/{workspace}/shares"),
            &owner,
            (
                Some(json!({"email": "holder@example.invalid", "access": "write"})),
                Some(&Uuid::now_v7().to_string()),
            ),
        )
        .await?;
        assert!((200..300).contains(&status), "share: {status} {body}");
        // The owner's desktop, where a person may have signed in during Take over; the share holder's task on the
        // same workspace; the owner's task on a fork of it.
        let signed_in = desktop_task(f, &owner, &workspace).await?;
        let shared = desktop_task(f, &holder, &workspace).await?;
        let copy = fork(f, &owner, &workspace).await?;
        let forked = desktop_task(f, &owner, &copy).await?;
        f.gateway.sandboxes().ensure(&signed_in).await?;
        f.gateway.sandboxes().ensure(&forked).await?;
        let (owner_desktop, owner_token, owner_workspace, owner_state) = sandbox(f, &signed_in)?;
        // The workspace's lease: the share holder's desktop never runs beside the owner's, where the sign-in is.
        let beside = f.gateway.sandboxes().ensure(&shared).await;
        assert!(
            beside
                .as_ref()
                .is_err_and(|error| format!("{error:?}") == "Busy"),
            "the share holder's desktop started beside the owner's: {:?}",
            beside.map(|_| ())
        );
        // The owner's desktop idle, the share holder's message takes the workspace over: the owner's sandbox (and the
        // browser tmpfs in it) ends, and the share holder's starts as a sandbox of its own.
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/conversations/{}/messages", shared.as_str()),
            &holder,
            (
                Some(json!({"text": "Sign in again"})),
                Some(&Uuid::now_v7().to_string()),
            ),
        )
        .await?;
        assert_eq!(status, 202, "the share holder's message");
        tokio::time::timeout(DEADLINE * 2, async {
            while f.backend.desktop(&shared).is_none() {
                f.gateway.sandboxes().admission_pass_now().await?;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        let live = f.backend.list_conversations().await?;
        assert!(
            !live.contains(&signed_in),
            "the owner's desktop ended before the share holder's started"
        );
        assert!(owner_desktop, "the owner's task is a Desktop's");
        for (who, id, same_workspace) in [
            ("the share holder's task", &shared, true),
            ("the fork's task", &forked, false),
        ] {
            let (desktop, token, workspace, state) = sandbox(f, id)?;
            assert!(
                desktop,
                "{who}: a Desktop of its own, with its own empty browser tmpfs"
            );
            assert_ne!(
                token, owner_token,
                "{who}: its own sandbox, never the owner's"
            );
            assert_ne!(
                state, owner_state,
                "{who}: its own state, never the owner's"
            );
            assert_eq!(
                workspace == owner_workspace,
                same_workspace,
                "{who}: the workspace it was made on"
            );
        }
        Ok(())
    })
    .await
}
