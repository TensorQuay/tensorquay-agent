//! Delivery failures replay durable sequences; append failures preserve the resumable prefix.
#[path = "support/fixture.rs"]
mod support;
use std::{sync::atomic::Ordering, time::Duration};
use support::*;
use tqa_runtime::{Runtime, RuntimeError};
use tqa_testkit::{Script, Scripted};
use tqa_types::{ErrorCode, Event, ServerFrame};

#[tokio::test]
async fn core_u_019_third_delivery_failure_replays_the_original_durable_frame() -> TestResult {
    let f = Fixture::new(vec![Script::new(vec![
        Scripted::Delay(Duration::from_secs(20)),
        Scripted::Text("answer".into()),
    ])])
    .await?;
    f.gateway.set_bound(true);
    let deps = f.deps()?;
    deps.testing.fail_delivery_at.store(3, Ordering::SeqCst);
    let runtime = Runtime::new(deps)?;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let task = tokio::spawn(runtime.run(shutdown.clone()));
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_millis(125))
    })
    .await?;
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_secs(20))
    })
    .await?;
    let records = read_log(&f.directory.path().join("state/events.jsonl"))?;
    // message.user, run.started, context.loaded (the failed third delivery) and step.started.
    assert_eq!(records.len(), 4);
    // The first two frames crossed a real socket, and the fake gateway stores them on its own task: wait for that
    // rather than assume it happened by the time the runtime's sleeps are pending (the flake under load: `stored`
    // was still empty).
    wait_for(|| f.gateway.stored().len() >= 2).await?;
    assert_eq!(f.gateway.stored(), records[..2]);
    assert_eq!(f.gateway.registrations(), 1);
    f.clock.advance(Duration::from_millis(125))?;
    f.gateway.wait_for_registrations(2).await?;
    f.wait_event("step.started").await?;
    // Received is not yet stored: the same wait before comparing what the gateway kept.
    wait_for(|| f.gateway.stored().len() >= records.len()).await?;
    assert_eq!(f.gateway.stored(), records);
    let delivered: Vec<_> = f
        .gateway
        .frames()
        .into_iter()
        .filter(|frame| matches!(frame, ServerFrame::Event { .. }))
        .collect();
    assert_eq!(
        delivered, records,
        "replay must send only the unacknowledged frames, from the third"
    );
    f.advance_when_idle(Duration::from_secs(20)).await?;
    f.wait_event("run.completed").await?;
    assert_eq!(f.gateway.model_requests().len(), 1);
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_e_010_full_journal_sends_only_io_error_and_restart_recovers_the_prefix() -> TestResult {
    let f = Fixture::new(vec![
        Script::new(vec![
            Scripted::Text("partial".into()),
            Scripted::Delay(Duration::from_secs(20)),
            Scripted::Text("must not escape".into()),
        ]),
        answer(),
    ])
    .await?;
    f.gateway.set_bound(true);
    let deps = f.deps()?;
    let hooks = deps.testing.clone();
    let runtime = Runtime::new(deps)?;
    let task = tokio::spawn(runtime.run(tokio_util::sync::CancellationToken::new()));
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_secs(20))
    })
    .await?;
    let prefix = f.gateway.stored();
    let path = f.directory.path().join("state/events.jsonl");
    assert_eq!(read_log(&path)?, prefix);
    hooks.fail_appends.store(true, Ordering::SeqCst);
    f.clock.advance(Duration::from_secs(20))?;
    let error = tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task)
        .await??
        .err()
        .ok_or("append failure must stop the runtime")?;
    assert_eq!(
        error,
        RuntimeError::io("wal append (test)"),
        "the error names the failed step"
    );
    assert_eq!(error.exit_code(), 1);
    assert_eq!(f.gateway.stored(), prefix);
    assert_eq!(read_log(&path)?, prefix);
    assert!(f.gateway.frames().iter().any(|frame| matches!(
        frame,
        ServerFrame::Error {
            seq: None,
            code: ErrorCode::Io,
            retryable: false,
            ..
        }
    )));
    assert!(!prefix.iter().any(|frame| matches!(
        frame,
        ServerFrame::Event {
            event: Event::RunCompleted { .. }
                | Event::RunFailed { .. }
                | Event::RunCancelled { .. },
            ..
        }
    )));
    assert!(matches!(
        tqa_context::resume_point(&prefix)?,
        tqa_context::ResumePoint::RetryModel { attempt: 2, .. }
    ));
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(2).await?;
    f.wait_event("run.completed").await?;
    let after = f.gateway.stored();
    assert_eq!(&after[..prefix.len()], prefix);
    assert!(matches!(
        &after[prefix.len()],
        ServerFrame::Event {
            event: Event::RunResumed { .. },
            ..
        }
    ));
    for (index, frame) in after.iter().enumerate() {
        assert_eq!(serde_json::to_value(frame)?["seq"], index + 1);
    }
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

fn read_log(path: &std::path::Path) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let bytes = std::fs::read_to_string(path)?;
    assert!(bytes.is_empty() || bytes.ends_with('\n'));
    bytes
        .lines()
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line)?;
            Ok(serde_json::from_value(value["frame"].clone())?)
        })
        .collect()
}
