//! GW-I-259: a sandbox's create and teardown each log a line at INFO when they begin, naming the conversation and
//! why, so a gap between them can be told apart (try 8 of 7 Oct 2026 logged only the finished destroy). Neither line
//! carries a token or a secret.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_sandbox::SandboxBackend;
use tqa_types::{ConversationId, Event, ServerFrame};

/// The process's subscriber at the gateway's own default filter, so it also sees spawned tasks.
fn capture_logs() -> Result<log_capture::Capture, Box<dyn std::error::Error>> {
    static CAPTURE: std::sync::OnceLock<Option<log_capture::Capture>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = log_capture::Capture::default();
            let filter = tqa_gateway::logging::filter(None).ok()?;
            let subscriber = tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_env_filter(filter)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .ok()
                .map(|()| capture)
        })
        .clone()
        .ok_or_else(|| "another global tracing subscriber was already set".into())
}

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn task(
    f: &Fixture,
    cookie: &str,
    workspace: &str,
    key: &str,
) -> Result<ConversationId, Box<dyn std::error::Error>> {
    let task: Value = call(f, cookie, Method::POST, "/api/v1/conversations")
        .header("idempotency-key", key)
        .json(&json!({"title":"Task","model_route":"local","workspace_id":workspace}))
        .send()
        .await?
        .json()
        .await?;
    Ok(task["id"].as_str().ok_or("task id")?.parse()?)
}

async fn message(f: &Fixture, cookie: &str, id: &ConversationId, key: &str) -> TestResult {
    let response = call(
        f,
        cookie,
        Method::POST,
        &format!("/api/v1/conversations/{id}/messages"),
    )
    .header("idempotency-key", key)
    .json(&json!({"text":"work"}))
    .send()
    .await?;
    assert_eq!(response.status(), 202);
    Ok(())
}

fn frame(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

/// Until `id`'s sandbox is created.
async fn until(f: &Fixture, id: &ConversationId) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while f.backend.created(id).is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

/// The captured lines that say `message` about `id`.
fn lines(text: &str, message: &str, id: &ConversationId) -> Vec<String> {
    let id = id.to_string();
    text.lines()
        .filter(|line| line.contains(message) && line.contains(&id))
        .map(str::to_owned)
        .collect()
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_259_a_sandbox_create_and_teardown_log_when_they_begin_with_the_reason(
    pool: PgPool,
) -> TestResult {
    let logs = capture_logs()?;
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let created: Value = call(f, &cookie, Method::POST, "/api/v1/workspaces")
            .json(&json!({"name":"Logged"}))
            .send()
            .await?
            .json()
            .await?;
        let workspace = created["id"].as_str().ok_or("id")?.to_owned();
        let holder = task(f, &cookie, &workspace, "holder").await?;
        let waiter = task(f, &cookie, &workspace, "waiter").await?;
        message(f, &cookie, &holder, "h").await?;
        until(f, &holder).await?;
        let mut runtime = f.runtime_for(&holder).await?;
        runtime.action().await?;
        for (seq, event) in [
            (
                1,
                Event::RunStarted {
                    run_id: "h".parse()?,
                },
            ),
            (
                2,
                Event::RunCompleted {
                    run_id: "h".parse()?,
                },
            ),
        ] {
            runtime.send(&frame(seq, event)).await?;
            f.until_sequence(&holder, seq).await?;
        }
        // The idle holder hands its workspace over to the waiter: its sandbox is torn down and the waiter's created.
        message(f, &cookie, &waiter, "w").await?;
        until(f, &waiter).await?;
        let text = logs.text()?;
        for (message, id, reason) in [
            ("sandbox create begun", &holder, "reason=\"work\""),
            ("sandbox teardown begun", &holder, "reason=\"hand_over\""),
            ("sandbox create begun", &waiter, "reason=\"work\""),
        ] {
            let found = lines(&text, message, id);
            assert_eq!(found.len(), 1, "{message} for {id}: {found:?}");
            assert!(found[0].contains(" INFO "), "{}", found[0]);
            assert!(found[0].contains(reason), "{}", found[0]);
            assert!(!found[0].to_lowercase().contains("token"), "{}", found[0]);
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-259
// A sandbox lost with its run going: what the engine still holds of it is torn down (`lost`), then a replacement is
// created (`replacement`), each logged as it begins (the review of GW-I-259: destroys outside `retire` say it too).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_259_a_lost_sandbox_logs_its_teardown_and_its_replacement(pool: PgPool) -> TestResult {
    let logs = capture_logs()?;
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&frame(
                1,
                Event::RunStarted {
                    run_id: "lost".parse()?,
                },
            ))
            .await?;
        f.until_sequence(id, 1).await?;
        f.clock.observed().await?;
        let original = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        f.backend.lose(&original).await?;
        drop(runtime);
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(std::sync::atomic::Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let text = logs.text()?;
        for (message, reason) in [
            ("sandbox teardown begun", "reason=\"lost\""),
            ("sandbox create begun", "reason=\"replacement\""),
        ] {
            let found: Vec<String> = lines(&text, message, id)
                .into_iter()
                .filter(|line| line.contains(reason))
                .collect();
            assert_eq!(found.len(), 1, "{message} {reason}: {found:?}");
            assert!(found[0].contains(" INFO "), "{}", found[0]);
            assert!(!found[0].to_lowercase().contains("token"), "{}", found[0]);
        }
        Ok(())
    })
    .await
}
