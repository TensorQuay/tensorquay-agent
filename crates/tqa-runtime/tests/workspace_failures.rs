//! RT-I-015: failed or interrupted initialization never becomes an accepted partial checkout.
#[path = "support/clone.rs"]
mod clone;
#[path = "support/fixture.rs"]
mod support;
use clone::FakeClone;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_runtime::{Runtime, RuntimeError};
use tqa_tools::CloneError;
use tqa_types::{ClientFrame, ErrorCode, Event, ServerFrame, WorkspaceCloneStatus};

type Running = (
    CancellationToken,
    tokio::task::JoinHandle<Result<(), RuntimeError>>,
);
async fn launch(
    f: &Fixture,
    fake: Arc<FakeClone>,
    registration: usize,
) -> Result<Running, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    value["workspace"]["clone"] = json!({"url":"https://git.example/repo"});
    f.gateway
        .set_session(ClientFrame::parse(&serde_json::to_vec(&value)?)?)?;
    let mut deps = f.deps_for_egress(Some(f.gateway.origin().into()))?;
    deps.repository_cloner = fake;
    let runtime = Runtime::new(deps)?;
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();
    let task = tokio::spawn(async move { runtime.run(token).await });
    f.gateway.set_bound(true);
    f.gateway.wait_for_registrations(registration).await?;
    Ok((shutdown, task))
}
fn failed_runs(f: &Fixture, expected: ErrorCode) -> usize {
    f.gateway
        .stored()
        .iter()
        .filter(|frame| {
            matches!(frame, ServerFrame::Event {
        event: Event::RunFailed { code, .. }, ..
    } if *code == expected)
        })
        .count()
}

#[tokio::test]
async fn rt_i_015_clone_denial_remains_failed_after_restart_without_retrying() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    let fake = FakeClone::new(Some(CloneError::EgressDenied));
    for count in 1..=2 {
        let (shutdown, task) = launch(&f, fake.clone(), count).await?;
        f.gateway.send(user()).await?;
        let result = wait_for(|| failed_runs(&f, ErrorCode::EgressDenied) == count).await;
        stop(shutdown, task).await?;
        result?;
    }
    assert_eq!(fake.seen.lock().map_err(|_| "fixture lock")?.len(), 1);
    assert!(f.gateway.model_requests().is_empty());
    assert!(!f.directory.path().join("workspace/README.md").exists());
    assert!(
        f.directory
            .path()
            .join("workspace/.tqa-clone-stage/checkout/README.md")
            .exists()
    );
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_015_clone_interruption_is_failed_on_restart_and_never_skipped() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    let fake = FakeClone::new(None);
    let (_release, paused) = tokio::sync::oneshot::channel();
    *fake.release.lock().map_err(|_| "fixture lock")? = Some(paused);
    let (shutdown, task) = launch(&f, fake.clone(), 1).await?;
    tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        fake.entered.notified(),
    )
    .await?;
    f.wait_event("workspace.clone").await?;
    stop(shutdown, task).await?;
    assert_eq!(failed_runs(&f, ErrorCode::Io), 0);
    let (shutdown, task) = launch(&f, fake.clone(), 2).await?;
    f.gateway.send(user()).await?;
    let result = wait_for(|| failed_runs(&f, ErrorCode::Io) == 1).await;
    stop(shutdown, task).await?;
    result?;
    assert_eq!(fake.seen.lock().map_err(|_| "fixture lock")?.len(), 1);
    assert!(f.gateway.model_requests().is_empty());
    assert!(!f.gateway.stored().iter().any(|frame| matches!(
        frame,
        ServerFrame::Event {
            event: Event::WorkspaceClone {
                status: WorkspaceCloneStatus::Skipped,
                ..
            },
            ..
        }
    )));
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_015_clone_deadline_cancels_transport_and_records_a_plain_failure() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    let fake = FakeClone::new(None);
    let (_release, paused) = tokio::sync::oneshot::channel();
    *fake.release.lock().map_err(|_| "fixture lock")? = Some(paused);
    let (shutdown, task) = launch(&f, fake.clone(), 1).await?;
    tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        fake.entered.notified(),
    )
    .await
    .map_err(|_| "the clone never reached its transport")?;
    f.wait_event("workspace.clone")
        .await
        .map_err(|error| format!("no workspace.clone event: {error}"))?;
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_mins(2))
    })
    .await
    .map_err(|error| format!("the clone's deadline never started: {error}"))?;
    // Past the deadline only once no journal or link write is in flight: a jump past their io_timeout would end
    // such a write as a timeout instead (the fixture's advance_when_idle).
    f.advance_when_idle(Duration::from_mins(2))
        .await
        .map_err(|error| format!("a write stayed in flight: {error}"))?;
    f.gateway.send(user()).await?;
    let result = wait_for(|| failed_runs(&f, ErrorCode::Io) == 1).await;
    stop(shutdown, task).await?;
    result.map_err(|error| format!("no run failed after the deadline: {error}"))?;
    assert!(
        f.gateway
            .stored()
            .iter()
            .any(|frame| matches!(frame, ServerFrame::Event {
        event: Event::WorkspaceClone { status: WorkspaceCloneStatus::Failed, message, .. }, ..
    } if message.contains("timed out")))
    );
    assert!(f.gateway.model_requests().is_empty());
    f.gateway.shutdown().await?;
    Ok(())
}

struct FailedSink(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl tqa_core::EventSink for FailedSink {
    async fn emit(
        &self,
        _event: Event,
        _context: tqa_core::EventContext,
    ) -> Result<tqa_types::Seq, tqa_core::SinkError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(tqa_core::SinkError {
            code: ErrorCode::Io,
        })
    }
}

#[tokio::test]
async fn rt_i_015_clone_journal_failure_prevents_transport_and_filesystem_writes() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    let fake = FakeClone::new(None);
    let initializer = tqa_runtime::testing::WorkspaceInitializer {
        workspace: f.directory.path().join("workspace"),
        state_dir: f.directory.path().join("state"),
        proxy: Some(tqa_tools::ProxyEndpoint::new(f.gateway.origin())?),
        cloner: fake.clone(),
        sleeper: f.sleeper.clone(),
    };
    let repository = tqa_types::WorkspaceClone {
        url: "https://git.example/repo".into(),
        r#ref: None,
    };
    let sink = FailedSink(std::sync::atomic::AtomicUsize::new(0));
    let mut history = vec![];
    let result = initializer
        .initialize(
            Some(&repository),
            &mut history,
            &sink,
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(result, Err(RuntimeError::Io(_))));
    assert_eq!(
        sink.0.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "no compensating append after journal failure"
    );
    assert!(history.is_empty());
    assert!(fake.seen.lock().map_err(|_| "fixture lock")?.is_empty());
    for directory in ["workspace", "state"] {
        assert!(
            std::fs::read_dir(f.directory.path().join(directory))?
                .next()
                .is_none()
        );
    }
    f.gateway.shutdown().await?;
    Ok(())
}
