//! GW-I-006: a failed action is never silent; it is logged and reaches open and later client streams.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;

use serde_json::json;
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, ErrorCode, SandboxState, ServerFrame};

/// One global subscriber for the whole process, so it also sees the manager's spawned tasks, installed once: `cargo
/// test` runs these tests in one process (nextest runs each in its own). Each test reads only its own conversation's
/// lines (`warned`).
fn capture_logs() -> Result<log_capture::Capture, Box<dyn std::error::Error>> {
    static CAPTURE: std::sync::OnceLock<Option<log_capture::Capture>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = log_capture::Capture::default();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::WARN)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .ok()
                .map(|()| capture)
        })
        .clone()
        .ok_or_else(|| "another global tracing subscriber was already set".into())
}

async fn failure_notice(stream: &mut support::stream::Stream, expected: ErrorCode) -> TestResult {
    let (sequence, frame) = stream.next().await?;
    assert_eq!(sequence, None);
    assert!(
        matches!(
            frame,
            ServerFrame::Error {
                seq: None,
                code,
                retryable: true,
                ..
            } if code == expected
        ),
        "expected a retryable {expected:?} notice, received {frame:?}"
    );
    Ok(())
}

async fn later_stream_sees_failure(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
    expected: ErrorCode,
) -> TestResult {
    let mut later = f.stream(id, cookie, 0).await?;
    assert_eq!(later.initial, SandboxState::Failed);
    failure_notice(&mut later, expected).await
}

fn warned(capture: &log_capture::Capture, id: &ConversationId, cause: &str) -> TestResult {
    let output = capture.text()?;
    let line = output
        .lines()
        .find(|line| line.contains("WARN") && line.contains(id.as_str()))
        .ok_or_else(|| format!("no WARN line for the failed action in: {output}"))?;
    assert!(
        line.contains("/messages"),
        "the WARN names the action route: {line}"
    );
    assert!(
        line.contains(cause),
        "the WARN names the cause {cause:?}: {line}"
    );
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_failed_action_warns_and_reaches_open_and_later_streams(
    pool: PgPool,
) -> TestResult {
    let capture = capture_logs()?;
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        let path = format!("/api/v1/conversations/{id}/messages");
        f.backend.fail_create.store(true, Ordering::SeqCst);
        let mut stream = f.stream(id, &cookie, 0).await?;
        let accepted = f
            .action(&path, &cookie, Some("fails"), &json!({"text":"start"}))
            .await?;
        assert_eq!(accepted.status(), 202);
        failure_notice(&mut stream, ErrorCode::SandboxFailed).await?;
        assert_eq!(stream.state().await?, SandboxState::Failed);
        warned(&capture, id, "sandbox create failed")?;
        later_stream_sees_failure(f, id, &cookie, ErrorCode::SandboxFailed).await?;
        // A later accepted action that succeeds clears the failure for new streams.
        f.backend.fail_create.store(false, Ordering::SeqCst);
        let retried = f
            .action(&path, &cookie, Some("retry"), &json!({"text":"start"}))
            .await?;
        assert_eq!(retried.status(), 202);
        let _runtime = f.runtime(&conversation).await?;
        assert_eq!(f.stream(id, &cookie, 0).await?.initial, SandboxState::Ready);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_restart_that_fails_queued_work_warns_and_later_streams_see_it(
    pool: PgPool,
) -> TestResult {
    let capture = capture_logs()?;
    support::run_restarting(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        let path = format!("/api/v1/conversations/{id}/messages");
        f.action(&path, &cookie, Some("retiring"), &json!({"text":"pending"}))
            .await?
            .error_for_status()?;
        f.gateway.sandboxes().ensure(id).await?;
        f.gateway.cancel();
        sqlx::query("UPDATE sandbox_instances SET phase='destroying'")
            .execute(&f.pool)
            .await?;
        f.restart().await?;
        warned(&capture, id, "interrupted by a gateway restart")?;
        later_stream_sees_failure(f, id, &cookie, ErrorCode::SandboxFailed).await
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_runtime_exit_before_registration_fails_at_once_with_its_code(
    pool: PgPool,
) -> TestResult {
    let capture = capture_logs()?;
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        *f.backend.runtime_exit.lock().map_err(|_| "exit lock")? = Some(2);
        let mut stream = f.stream(id, &cookie, 0).await?;
        let path = format!("/api/v1/conversations/{id}/messages");
        let accepted = f
            .action(&path, &cookie, Some("exits"), &json!({"text":"start"}))
            .await?;
        assert_eq!(accepted.status(), 202);
        // Reported as soon as the runtime exits, not after the 30 s readiness deadline.
        failure_notice(&mut stream, ErrorCode::RuntimeStartFailed).await?;
        assert_eq!(stream.state().await?, SandboxState::Failed);
        warned(&capture, id, "runtime exited before registration: code 2")?;
        let status: (String, Option<String>) = sqlx::query_as(
            "SELECT execution_status,execution_error FROM idempotency WHERE key='exits'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            status,
            ("failed".into(), Some("runtime_start_failed".into()))
        );
        later_stream_sees_failure(f, id, &cookie, ErrorCode::RuntimeStartFailed).await
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_a_stream_opened_during_failure_cleanup_reports_the_failure(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        *f.backend.runtime_exit.lock().map_err(|_| "exit lock")? = Some(2);
        // Hold the cleanup: the failure is reported while the sandbox record is still being destroyed.
        f.backend
            .hold_destroy
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut stream = f.stream(id, &cookie, 0).await?;
        let path = format!("/api/v1/conversations/{id}/messages");
        let accepted = f
            .action(&path, &cookie, Some("exits"), &json!({"text":"start"}))
            .await?;
        assert_eq!(accepted.status(), 202);
        failure_notice(&mut stream, ErrorCode::RuntimeStartFailed).await?;
        later_stream_sees_failure(f, id, &cookie, ErrorCode::RuntimeStartFailed).await?;
        f.backend
            .hold_destroy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        f.backend.release_destroy.notify_waiters();
        Ok(())
    })
    .await
}

/// Whether a WARN line for the conversation holds `text`.
fn warned_with(capture: &log_capture::Capture, id: &ConversationId, text: &str) -> TestResult {
    let output = capture.text()?;
    let found = output
        .lines()
        .any(|line| line.contains("WARN") && line.contains(id.as_str()) && line.contains(text));
    assert!(found, "no WARN line for {id} holding {text:?} in: {output}");
    Ok(())
}

/// Waits until the backend has created `count` sandboxes.
async fn creates_reach(f: &Fixture, count: usize) -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while f.backend.creates.load(Ordering::SeqCst) < count {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

// TEST: GW-I-227
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_227_a_vm_host_that_dies_before_registering_is_started_once_more(
    pool: PgPool,
) -> TestResult {
    let capture = capture_logs()?;
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        f.backend.host_deaths.store(1, Ordering::SeqCst);
        let path = format!("/api/v1/conversations/{id}/messages");
        let accepted = f
            .action(&path, &cookie, Some("blip"), &json!({"text":"start"}))
            .await?;
        assert_eq!(accepted.status(), 202);
        creates_reach(f, 2).await?;
        // The second VM host lives: its runtime registers and the start completes, with nothing failed.
        let _runtime = f.runtime(&conversation).await?;
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            2,
            "one start more, no further"
        );
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            1,
            "the dead one removed"
        );
        warned_with(
            &capture,
            id,
            "the VM host died before its runtime registered; starting it once more",
        )?;
        let status: (String,) =
            sqlx::query_as("SELECT execution_status FROM idempotency WHERE key='blip'")
                .fetch_one(&f.pool)
                .await?;
        assert_ne!(
            status.0, "failed",
            "a person sees no failed start for one blip"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-227
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_227_a_second_death_in_a_row_fails_the_start(pool: PgPool) -> TestResult {
    let capture = capture_logs()?;
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        f.backend.host_deaths.store(2, Ordering::SeqCst);
        let mut stream = f.stream(id, &cookie, 0).await?;
        let path = format!("/api/v1/conversations/{id}/messages");
        let accepted = f
            .action(&path, &cookie, Some("twice"), &json!({"text":"start"}))
            .await?;
        assert_eq!(accepted.status(), 202);
        failure_notice(&mut stream, ErrorCode::RuntimeStartFailed).await?;
        assert_eq!(
            f.backend.creates.load(Ordering::SeqCst),
            2,
            "started once more, then given up"
        );
        warned_with(&capture, id, "starting it once more")?;
        warned_with(&capture, id, "runtime exited before registration: code -1")?;
        let status: (String, Option<String>) = sqlx::query_as(
            "SELECT execution_status,execution_error FROM idempotency WHERE key='twice'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            status,
            ("failed".into(), Some("runtime_start_failed".into()))
        );
        Ok(())
    })
    .await
}
