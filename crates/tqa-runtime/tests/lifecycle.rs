//! RT-I-005/009/048: restart and shutdown preserve the durable run boundary; an idle stop clears kept screenshots.
#[path = "support/fixture.rs"]
mod support;
use std::{sync::atomic::Ordering, time::Duration};
use support::*;
use tqa_runtime::{Runtime, RuntimeError};
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, ErrorCode, Event, ServerFrame};

fn delayed() -> Script {
    Script::new(vec![
        Scripted::Text("partial".into()),
        Scripted::Delay(Duration::from_secs(20)),
        Scripted::Text("end".into()),
    ])
}

#[tokio::test]
async fn rt_i_009_shutdown_leaves_an_active_run_resumable_without_a_terminal_event() -> TestResult {
    let f = Fixture::new(vec![delayed()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    stop(shutdown, task).await?;
    let (wal, recovery) = tqa_context::Wal::open(&f.directory.path().join("state"))?;
    assert!(!recovery.records.is_empty());
    assert!(!f.gateway.stored().iter().any(terminal));
    assert!(!matches!(
        tqa_context::resume_point(&f.gateway.stored())?,
        tqa_context::ResumePoint::Idle { .. }
    ));
    drop(wal);
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_005_crash_then_restart_replays_contiguously_and_resumes_once() -> TestResult {
    let f = Fixture::new(vec![delayed(), answer()]).await?;
    f.gateway.set_bound(true);
    let (_, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    let before = f.gateway.stored();
    task.abort();
    assert!(task.await.is_err());
    // Reopening proves the aborted owner released the worker's exclusive file lock.
    wait_for(|| tqa_context::Wal::open(&f.directory.path().join("state")).is_ok()).await?;
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(2).await?;
    f.wait_event("run.completed").await?;
    let stored = f.gateway.stored();
    assert_eq!(&stored[..before.len()], before);
    for (index, frame) in stored.iter().enumerate() {
        assert_eq!(serde_json::to_value(frame)?["seq"], index + 1);
    }
    assert_eq!(
        stored
            .iter()
            .filter(|f| matches!(
                f,
                ServerFrame::Event {
                    event: Event::RunResumed { .. },
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(
        stored
            .iter()
            .filter(|f| matches!(
                f,
                ServerFrame::Event {
                    event: Event::MessageUser { .. },
                    ..
                }
            ))
            .count(),
        1
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
// TEST: CORE-U-019
async fn rt_i_005_no_frame_is_observable_before_append_even_when_the_append_fails() -> TestResult {
    let f = Fixture::new(vec![delayed()]).await?;
    f.gateway.set_bound(true);
    let deps = f.deps()?;
    let hooks = deps.testing.clone();
    let runtime = Runtime::new(deps)?;
    let task = tokio::spawn(runtime.run(tokio_util::sync::CancellationToken::new()));
    f.gateway.wait_for_registrations(1).await?;
    wait_for(|| {
        f.gateway
            .requests()
            .iter()
            .any(|r| r.path == "/runtime/history")
    })
    .await?;
    hooks.before_append.pause();
    f.gateway.send(user()).await?;
    hooks.before_append.wait_until_entered().await?;
    // A socket control round trip proves the link has had a chance to send queued frames.
    f.gateway
        .send(ClientFrame::SessionInit {
            conversation_id: "conversation".parse()?,
            last_seq: tqa_types::Seq::ZERO,
            protocol: 1,
        })
        .await?;
    wait_for(|| {
        f.gateway.frames().iter().any(|f| {
            matches!(
                f,
                ServerFrame::Error {
                    code: ErrorCode::InvalidFrame,
                    ..
                }
            )
        })
    })
    .await?;
    assert!(
        f.gateway.stored().is_empty(),
        "a frame escaped before the WAL append"
    );
    hooks.fail_appends.store(true, Ordering::SeqCst);
    hooks.before_append.resume();
    assert_eq!(
        tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await??,
        Err(RuntimeError::io("wal append (test)"))
    );
    // The runtime exits only after its link wrote the error frame; the fake gateway records what it receives on its own
    // task, so wait (bounded) for that record instead of expecting it at the instant the runtime task ends.
    wait_for(|| {
        f.gateway.frames().iter().any(|f| {
            matches!(
                f,
                ServerFrame::Error {
                    seq: None,
                    code: ErrorCode::Io,
                    retryable: false,
                    ..
                }
            )
        })
    })
    .await?;
    assert!(f.gateway.stored().is_empty());
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_001_changed_snapshot_exits_four_without_terminating_the_run() -> TestResult {
    let f = Fixture::new(vec![delayed()]).await?;
    f.gateway.set_bound(true);
    let (_, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    let mut changed = session()?;
    if let ClientFrame::SessionConfig { workspace, .. } = &mut changed {
        workspace.display = "changed".into();
    }
    f.gateway.set_session(changed)?;
    f.gateway.disconnect();
    wait_for(|| f.sleeper.durations().contains(&Duration::from_millis(125))).await?;
    f.clock.advance(Duration::from_millis(125))?;
    let error = tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task)
        .await??
        .err()
        .ok_or("runtime accepted changed snapshot")?;
    assert_eq!(error, RuntimeError::SnapshotChanged("workspace"));
    assert_eq!(error.exit_code(), 4);
    assert!(!f.gateway.stored().iter().any(terminal));
    f.gateway.shutdown().await?;
    Ok(())
}

fn terminal(frame: &ServerFrame) -> bool {
    matches!(
        frame,
        ServerFrame::Event {
            event: Event::RunCompleted { .. }
                | Event::RunFailed { .. }
                | Event::RunCancelled { .. },
            ..
        }
    )
}

/// Plants a kept screenshot as the store lays them out, `<state>/screenshots/<run>/<order>-<call>.png`.
fn plant_screenshot(
    f: &Fixture,
    run: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let dir = f.directory.path().join("state/screenshots").join(run);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("000000-call_1.png");
    std::fs::write(&path, b"PNG")?;
    Ok(path)
}

// TEST: RT-I-048
#[tokio::test]
async fn rt_i_048_a_sandbox_stopping_idle_clears_its_kept_screenshots() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    let kept = plant_screenshot(&f, "run_earlier")?;
    stop(shutdown, task).await?;
    assert!(!kept.exists(), "an idle stop leaves no screenshot behind");
    assert!(
        !f.directory
            .path()
            .join("state/screenshots/run_earlier")
            .exists()
    );
    f.gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-048
#[tokio::test]
async fn rt_i_048_a_stop_during_a_run_keeps_its_screenshots_for_the_resume() -> TestResult {
    let f = Fixture::new(vec![delayed()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    let kept = plant_screenshot(&f, "run_active")?;
    stop(shutdown, task).await?;
    assert!(
        kept.exists(),
        "the interrupted run resumes and still needs its screenshots"
    );
    f.gateway.shutdown().await?;
    Ok(())
}
