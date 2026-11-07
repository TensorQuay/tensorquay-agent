//! SEC-017 (gateway half): what one member of a shared workspace can do to another conversation's history there.
//!
//! Two people share a workspace; each has a task (conversation) in it. The second's sandbox and link are the attacker's
//! reach. The history is protected by four gateway controls (ARCHITECTURE §12, ADR-0020):
//! - each sandbox mounts only its own conversation's state directory (the runtime's write-ahead log), and none of them
//!   lies inside the shared workspace;
//! - a link is bound to its conversation by its token alone: every frame it carries lands in that conversation's history,
//!   whatever run or request ids it names;
//! - the runtime history route serves only the token's conversation;
//! - sharing a workspace shares its files, not its conversations: the other person's API calls on the first's
//!   conversation are refused.
//!
//! The Docker stack half (from inside the second sandbox, the first's state directory is absent) is a stack test.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use support::{Fixture, TestResult};
use tqa_sandbox::StateSource;
use tqa_types::{ClientFrame, ConversationId, Event, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn event(seq: u64, run: &str, event: Event) -> Result<ServerFrame> {
    Ok(ServerFrame::Event {
        seq: seq.into(),
        run_id: Some(run.parse()?),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    })
}

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn task(f: &Fixture, cookie: &str, workspace: &str, key: &str) -> Result<ConversationId> {
    let task: Value = call(f, cookie, Method::POST, "/api/v1/conversations")
        .header("idempotency-key", key)
        .json(&json!({"title":"Task","model_route":"local","workspace_id":workspace}))
        .send()
        .await?
        .json()
        .await?;
    Ok(task["id"].as_str().ok_or("task id")?.parse()?)
}

async fn message(f: &Fixture, cookie: &str, id: &ConversationId, key: &str) -> Result<u16> {
    Ok(call(
        f,
        cookie,
        Method::POST,
        &format!("/api/v1/conversations/{id}/messages"),
    )
    .header("idempotency-key", key)
    .json(&json!({"text":"work"}))
    .send()
    .await?
    .status()
    .as_u16())
}

/// A conversation's stored history as (seq, run id, event type, text).
async fn history(f: &Fixture, id: &ConversationId) -> Result<Vec<(i64, Option<String>, Value)>> {
    let rows: Vec<(i64, Option<String>, Value)> = sqlx::query_as(
        "SELECT seq, run_id, frame FROM events WHERE conversation_id=$1 ORDER BY seq",
    )
    .bind(uuid::Uuid::parse_str(id.as_str())?)
    .fetch_all(&f.pool)
    .await?;
    Ok(rows)
}

fn state_dir(f: &Fixture, id: &ConversationId) -> Result<std::path::PathBuf> {
    match f.backend.mounts(id).ok_or("no sandbox")?.1.source {
        StateSource::HostDirectory(path) => Ok(path),
        StateSource::BlockImage(path) => {
            Err(format!("a block image on Docker: {}", path.display()).into())
        }
    }
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_017_a_shared_workspace_member_cannot_reach_another_conversations_history(
    pool: sqlx::PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, owner_cookie) = f.admin().await?;
        let (sharer, sharer_cookie) = f.member("sharer@example.invalid").await?;
        let created: Value = call(f, &owner_cookie, Method::POST, "/api/v1/workspaces")
            .json(&json!({"name":"Shared"}))
            .send()
            .await?
            .json()
            .await?;
        let workspace = created["id"].as_str().ok_or("workspace id")?.to_owned();
        let share = format!("/api/v1/workspaces/{workspace}/shares/{}", sharer.id);
        let shared = call(f, &owner_cookie, Method::PUT, &share)
            .json(&json!({"access":"write"}))
            .send()
            .await?;
        assert!(shared.status().is_success(), "share: {}", shared.status());
        let first = task(f, &owner_cookie, &workspace, "first").await?;
        let second = task(f, &sharer_cookie, &workspace, "second").await?;

        // The first conversation runs and records its history.
        assert_eq!(message(f, &owner_cookie, &first, "a").await?, 202);
        let mut runtime_a = f.runtime_for(&first).await?;
        assert!(matches!(
            runtime_a.action().await?,
            ClientFrame::UserMessage { .. }
        ));
        runtime_a
            .send(&event(
                1,
                "run-a",
                Event::RunStarted {
                    run_id: "run-a".parse()?,
                },
            )?)
            .await?;
        runtime_a
            .send(&event(
                2,
                "run-a",
                Event::TextDelta {
                    text: "the first's words".into(),
                },
            )?)
            .await?;
        runtime_a
            .send(&event(
                3,
                "run-a",
                Event::RunCompleted {
                    run_id: "run-a".parse()?,
                },
            )?)
            .await?;
        f.until_sequence(&first, 3).await?;
        let before = history(f, &first).await?;
        assert_eq!(before.len(), 3);

        // The second takes the idle workspace (GW-I-031) and its sandbox comes up.
        assert_eq!(message(f, &sharer_cookie, &second, "b").await?, 202);
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.created(&second).is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let mut runtime_b = f.runtime_for(&second).await?;
        assert!(matches!(
            runtime_b.action().await?,
            ClientFrame::UserMessage { .. }
        ));

        // Mounts: one workspace, two state directories, neither inside the workspace nor inside each other.
        let workspace_a = f.backend.mounted(&first).ok_or("first workspace")?;
        let workspace_b = f.backend.mounted(&second).ok_or("second workspace")?;
        assert_eq!(
            workspace_a, workspace_b,
            "both tasks mount the one shared workspace"
        );
        let (state_a, state_b) = (state_dir(f, &first)?, state_dir(f, &second)?);
        assert_ne!(
            state_a, state_b,
            "each conversation has its own state directory"
        );
        for state in [&state_a, &state_b] {
            assert!(
                !state.starts_with(&workspace_a) && !workspace_a.starts_with(state),
                "{}",
                state.display()
            );
        }
        assert!(!state_b.starts_with(&state_a) && !state_a.starts_with(&state_b));
        assert!(state_a.ends_with(first.as_str()) && state_b.ends_with(second.as_str()));

        // The link: frames naming the first's run, and a text meant to read as the first's, land in the second's
        // history only. The first's stays exactly as it was.
        runtime_b
            .send(&event(
                1,
                "run-a",
                Event::RunStarted {
                    run_id: "run-a".parse()?,
                },
            )?)
            .await?;
        runtime_b
            .send(&event(
                2,
                "run-a",
                Event::TextDelta {
                    text: "forged into the first".into(),
                },
            )?)
            .await?;
        runtime_b
            .send(&event(
                3,
                "run-a",
                Event::RunFailed {
                    run_id: "run-a".parse()?,
                    reason: "forged".into(),
                    code: tqa_types::ErrorCode::Internal,
                    provider_message: None,
                },
            )?)
            .await?;
        f.until_sequence(&second, 3).await?;
        assert_eq!(
            history(f, &first).await?,
            before,
            "the second's link changed the first's history"
        );
        let theirs = history(f, &second).await?;
        assert_eq!(theirs.len(), 3);
        assert!(theirs[1].2.to_string().contains("forged into the first"));

        // The runtime history route: the second's token reads the second's history only, and names no other.
        let token = f.backend.token(&second).ok_or("second token")?;
        let page: Value = f
            .client
            .get(f.url("/runtime/history?after=0&limit=100"))
            .bearer_auth(token.expose_secret())
            .send()
            .await?
            .json()
            .await?;
        let text = page["items"].to_string();
        assert!(
            !text.contains("the first's words"),
            "the second's token read the first's history: {text}"
        );
        let named = f
            .client
            .get(f.url(&format!(
                "/runtime/history?after=0&limit=100&conversation_id={first}"
            )))
            .bearer_auth(token.expose_secret())
            .send()
            .await?;
        assert_eq!(named.status(), 400);

        // The API: the workspace share gives the second person nothing on the first's conversation.
        for (method, path) in [
            (Method::GET, format!("/api/v1/conversations/{first}")),
            (Method::GET, format!("/api/v1/conversations/{first}/events")),
        ] {
            let status = call(f, &sharer_cookie, method, &path)
                .send()
                .await?
                .status();
            assert!(status == 403 || status == 404, "{path}: {status}");
        }
        let posted = message(f, &sharer_cookie, &first, "into-first").await?;
        assert!(
            posted == 403 || posted == 404,
            "posting into the first: {posted}"
        );
        assert_eq!(history(f, &first).await?, before);
        Ok(())
    })
    .await
}
