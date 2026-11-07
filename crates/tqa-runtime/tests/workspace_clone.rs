//! RT-I-015: repository initialization precedes the first run and survives restart.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_runtime::Runtime;
use tqa_types::{ClientFrame, ErrorCode, Event, ServerFrame, WorkspaceCloneStatus};

fn configure(f: &Fixture) -> TestResult {
    let mut configured = serde_json::to_value(session()?)?;
    configured["workspace"]["clone"] = json!({"url":"https://git.example/repository.git"});
    f.gateway
        .set_session(ClientFrame::parse(&serde_json::to_vec(&configured)?)?)?;
    Ok(())
}

async fn run_turn(f: &Fixture, registration: usize) -> TestResult {
    let runtime = Runtime::new(f.deps()?)?;
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();
    let prior = f.gateway.stored().len();
    let task = tokio::spawn(async move { runtime.run(token).await });
    f.gateway.set_bound(true);
    f.gateway.wait_for_registrations(registration).await?;
    f.gateway.send(user()).await?;
    let completed = wait_for(|| {
        f.gateway.stored().iter().skip(prior).any(|frame| {
            matches!(
                frame,
                ServerFrame::Event {
                    event: Event::RunCompleted { .. } | Event::RunFailed { .. },
                    ..
                }
            )
        })
    })
    .await;
    let stopped = stop(shutdown, task).await;
    completed?;
    stopped
}

#[tokio::test]
async fn rt_i_015_clone_preserves_existing_files_and_does_not_repeat_after_restart() -> TestResult {
    let f = Fixture::new(vec![answer(), answer()]).await?;
    configure(&f)?;
    let file = f.directory.path().join("workspace/existing.txt");
    std::fs::write(&file, b"keep these exact bytes")?;
    run_turn(&f, 1).await?;
    assert_eq!(std::fs::read(&file)?, b"keep these exact bytes");
    let before = f.gateway.stored();
    let skipped = before
        .iter()
        .position(|frame| {
            matches!(
                frame,
                ServerFrame::Event {
                    event: Event::WorkspaceClone {
                        status: WorkspaceCloneStatus::Skipped,
                        code: None,
                        ..
                    },
                    ..
                }
            )
        })
        .ok_or("missing durable skipped status")?;
    let started = before
        .iter()
        .position(|frame| {
            matches!(
                frame,
                ServerFrame::Event {
                    event: Event::RunStarted { .. },
                    ..
                }
            )
        })
        .ok_or("missing run start")?;
    assert!(skipped < started);
    std::fs::remove_file(file)?;
    run_turn(&f, 2).await?;
    assert_eq!(f.gateway.model_requests().len(), 2);
    assert_eq!(
        f.gateway
            .stored()
            .iter()
            .filter(|frame| matches!(
                frame,
                ServerFrame::Event {
                    event: Event::WorkspaceClone { .. },
                    ..
                }
            ))
            .count(),
        1
    );
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_015_clone_without_configured_proxy_fails_the_run_without_model_execution()
-> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    configure(&f)?;
    run_turn(&f, 1).await?;
    let frames = f.gateway.stored();
    assert!(frames.iter().any(|frame| matches!(
        frame,
        ServerFrame::Event {
            event: Event::WorkspaceClone {
                status: WorkspaceCloneStatus::Failed,
                code: Some(ErrorCode::EgressDenied),
                ..
            },
            ..
        }
    )));
    assert!(frames.iter().any(|frame| matches!(
        frame,
        ServerFrame::Event {
            event: Event::RunFailed {
                code: ErrorCode::EgressDenied,
                ..
            },
            ..
        }
    )));
    assert!(f.gateway.model_requests().is_empty());
    assert!(
        std::fs::read_dir(f.directory.path().join("workspace"))?
            .next()
            .is_none()
    );
    f.gateway.shutdown().await?;
    Ok(())
}
