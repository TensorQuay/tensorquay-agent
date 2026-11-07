//! Deterministic environment lifetime fault paired with an actual runtime and WAL.
#[path = "../../tqa-runtime/tests/support/fixture.rs"]
mod runtime;
#[path = "support/spec.rs"]
mod specifications;
use runtime::{Fixture, TestResult, answer, stop, user, wait_for};
use std::time::{Duration, SystemTime};
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{FakeSandboxBackend, SandboxBackend, SandboxError, WorkspaceSource};
use tqa_testkit::{Script, Scripted};
use tqa_types::{Event, Seq, ServerFrame};

#[tokio::test]
async fn sbx_d_003_backend_loss_restarts_the_runtime_on_its_persistent_wal() -> TestResult {
    let mut interrupted = Script::new(vec![
        Scripted::Text("before daemon loss".into()),
        Scripted::Delay(Duration::from_secs(61)),
        Scripted::Text("unreachable".into()),
    ]);
    interrupted.header_delay = Duration::from_secs(1);
    let f = Fixture::new(vec![interrupted, answer()]).await?;
    f.gateway.set_bound(true);
    let backend = FakeSandboxBackend::new(SystemTime::UNIX_EPOCH);
    let mut spec = specifications::spec()?;
    spec.conversation_id = "conversation".parse()?;
    spec.workspace.source = WorkspaceSource::HostDirectory(f.directory.path().join("workspace"));
    let state = f.directory.path().join("state");
    spec.state = tqa_sandbox::StateMount::directory(&state);
    let original = backend
        .create(spec.clone(), CancellationToken::new())
        .await?;
    let (_, process) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_secs(1))
    })
    .await?;
    let prefix = f.gateway.stored();
    assert!(!prefix.is_empty());
    f.gateway.set_persist(false);
    f.clock.advance(Duration::from_secs(1))?;
    f.wait_event("text.delta").await?;

    // A daemon loss ends the runtime owner; stopped handles remain available for cleanup.
    backend.exit(&original, 137)?;
    process.abort();
    assert!(process.await.is_err());
    assert_eq!(
        backend.address(&original).await,
        Err(SandboxError::NotFound)
    );
    assert_eq!(backend.list().await?, vec![original.clone()]);
    wait_for(|| tqa_context::Wal::open(&state).is_ok()).await?;
    let retained = retained_history(&state, prefix)?;
    backend.destroy(&original).await?;

    let replacement = backend.create(spec, CancellationToken::new()).await?;
    assert_ne!(replacement.id, original.id);
    f.gateway.set_persist(true);
    let (shutdown, process) = f.launch()?;
    f.gateway.wait_for_registrations(2).await?;
    f.wait_event("run.completed").await?;
    let history = f.gateway.stored();
    assert_eq!(&history[..retained.len()], retained);
    check_history(&history)?;
    assert_eq!(f.gateway.model_requests().len(), 2);
    assert_eq!(backend.list().await?, vec![replacement.clone()]);
    stop(shutdown, process).await?;
    backend.destroy(&replacement).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

fn retained_history(
    directory: &std::path::Path,
    prefix: Vec<ServerFrame>,
) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let (wal, recovery) = tqa_context::Wal::open(directory)?;
    let watermark = Seq::from(prefix.len() as u64);
    let tail = recovery
        .records
        .into_iter()
        .filter(|(seq, _)| *seq > watermark)
        .map(|(_, frame)| frame)
        .collect::<Vec<_>>();
    assert!(!tail.is_empty());
    assert!(!recovery.truncated_tail);
    drop(wal);
    Ok([prefix, tail].concat())
}

fn check_history(history: &[ServerFrame]) -> TestResult {
    for (index, frame) in history.iter().enumerate() {
        assert_eq!(serde_json::to_value(frame)?["seq"], index + 1);
    }
    assert_eq!(
        history
            .iter()
            .filter(|frame| matches!(
                frame,
                ServerFrame::Event {
                    event: Event::RunResumed { .. },
                    ..
                }
            ))
            .count(),
        1
    );
    Ok(())
}
