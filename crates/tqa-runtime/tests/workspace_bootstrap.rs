//! RT-I-015: clone bootstrap keeps the link alive and gates all model execution.
#[path = "support/clone.rs"]
mod clone;
#[path = "support/fixture.rs"]
mod support;
use clone::FakeClone;
use serde_json::json;
use support::*;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tqa_runtime::Runtime;
use tqa_types::{ClientFrame, Event, ServerFrame, WorkspaceCloneStatus};

fn configure(f: &Fixture) -> TestResult {
    let mut value = serde_json::to_value(session()?)?;
    value["workspace"]["clone"] =
        json!({"url":"https://git.example/repo", "ref":"feature/example"});
    f.gateway
        .set_session(ClientFrame::parse(&serde_json::to_vec(&value)?)?)?;
    Ok(())
}

#[tokio::test]
async fn rt_i_015_clone_waits_before_the_run_and_discovers_cloned_instructions() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    configure(&f)?;
    f.gateway.set_persist(false);
    let fake = FakeClone::new(None);
    let (release, paused) = oneshot::channel();
    *fake.release.lock().map_err(|_| "fixture lock")? = Some(paused);
    let mut deps = f.deps_for_egress(Some(f.gateway.origin().into()))?;
    deps.repository_cloner = fake.clone();
    let runtime = Runtime::new(deps)?;
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();
    let task = tokio::spawn(async move { runtime.run(token).await });
    f.gateway.set_bound(true);
    f.gateway.wait_for_registrations(1).await?;
    tokio::time::timeout(
        tqa_testkit::deadline::scaled(std::time::Duration::from_secs(5)),
        fake.entered.notified(),
    )
    .await?;
    f.wait_event("workspace.clone").await?;
    queue_and_redial(&f).await?;
    assert!(f.gateway.model_requests().is_empty());
    release
        .send(())
        .map_err(|()| "clone exited before release")?;
    let result = f.wait_event("run.completed").await;
    stop(shutdown, task).await?;
    result?;
    assert_checkout(&f, &fake)?;
    f.gateway.shutdown().await?;
    Ok(())
}

async fn queue_and_redial(f: &Fixture) -> TestResult {
    f.gateway.send(user()).await?;
    f.gateway.send(user()).await?;
    wait_for(|| {
        f.gateway.frames().iter().any(|frame| {
            matches!(
                frame,
                ServerFrame::Error {
                    code: tqa_types::ErrorCode::Busy,
                    ..
                }
            )
        })
    })
    .await?;
    f.gateway.disconnect();
    let backoff = std::time::Duration::from_millis(125);
    wait_for(|| f.sleeper.pending_durations().contains(&backoff)).await?;
    f.clock.advance(backoff)?;
    f.gateway.wait_for_registrations(2).await?;
    wait_for(|| {
        f.gateway
            .frames()
            .iter()
            .filter(|frame| clone_phase(frame, WorkspaceCloneStatus::Started))
            .count()
            == 2
    })
    .await?;
    Ok(())
}

fn assert_checkout(f: &Fixture, fake: &FakeClone) -> TestResult {
    assert_eq!(
        *fake.seen.lock().map_err(|_| "fixture lock")?,
        [Some("feature/example".into())]
    );
    let requests = f.gateway.model_requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .body
            .to_string()
            .contains("Repository fixture instruction")
    );
    assert_eq!(
        std::fs::read_to_string(f.directory.path().join("workspace/README.md"))?,
        "checkout fixture\n"
    );
    let frames = f.gateway.frames();
    let completed = frames
        .iter()
        .position(|frame| clone_phase(frame, WorkspaceCloneStatus::Completed))
        .ok_or("missing completed clone")?;
    let started = frames
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
    assert!(completed < started);
    Ok(())
}

fn clone_phase(frame: &ServerFrame, phase: WorkspaceCloneStatus) -> bool {
    matches!(frame, ServerFrame::Event { event: Event::WorkspaceClone { status, .. }, .. } if *status == phase)
}
