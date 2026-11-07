//! CORE-U-005/006/038 and CORE-E-030: every event boundary is cancellable or resumable.
mod support;
use std::{sync::Arc, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_tools::ToolRegistry;
use tqa_types::{Event, ToolStatus};

#[tokio::test]
async fn core_u_005_cancel_after_every_event_closes_calls_and_emits_one_terminal() -> TestResult {
    let total = include_str!("fixtures/parallel.txt").lines().count();
    for cut in 1..=total {
        let cancel = CancellationToken::new();
        let tool = fake("read_file", true, vec![output("a", 0), output("b", 0)])?;
        let mut h = Harness::new(
            vec![
                calls(&[
                    ("a", "read_file", r#"{"path":"a"}"#),
                    ("b", "read_file", r#"{"path":"b"}"#),
                ]),
                answer(),
            ],
            registry(&[tool])?,
        )?;
        let mut sink = MemorySink::new(&[]);
        sink.cut = Some((cut, cancel.clone()));
        h.sink_with(sink);
        let outcome = Runner::new(h.deps)?
            .run(&[], Some(turn()), cancel, CancellationToken::new())
            .await?;
        assert_eq!(
            outcome,
            if cut < total {
                RunOutcome::Cancelled
            } else {
                RunOutcome::Completed
            }
        );
        assert_closed(&h.sink.frames())?;
        let types = h.sink.types();
        assert!(matches!(
            types.last().map(String::as_str),
            Some("run.cancelled" | "run.completed")
        ));
        assert_eq!(h.provider.active_streams(), 0);
    }
    Ok(())
}
#[tokio::test]
async fn core_u_005_pre_cancel_still_acknowledges_the_message_and_run() -> TestResult {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let h = Harness::new(vec![], ToolRegistry::new())?;
    assert_eq!(
        Runner::new(h.deps)?
            .run(&[], Some(turn()), cancel, CancellationToken::new())
            .await?,
        RunOutcome::Cancelled
    );
    assert_eq!(
        h.sink.types(),
        ["message.user", "run.started", "run.cancelled"]
    );
    assert!(h.provider.requests().is_empty());
    Ok(())
}
#[tokio::test]
async fn core_u_038_shutdown_after_every_event_stops_writes_and_can_resume() -> TestResult {
    let total = include_str!("fixtures/parallel.txt").lines().count();
    for cut in 1..total {
        let shutdown = CancellationToken::new();
        let tool = fake("read_file", true, vec![output("a", 0), output("b", 0)])?;
        let mut h = Harness::new(
            vec![
                calls(&[
                    ("a", "read_file", r#"{"path":"a"}"#),
                    ("b", "read_file", r#"{"path":"b"}"#),
                ]),
                answer(),
            ],
            registry(&[tool])?,
        )?;
        let mut sink = MemorySink::new(&[]);
        sink.cut = Some((cut, shutdown.clone()));
        h.sink_with(sink);
        assert_eq!(
            Runner::new(h.deps)?
                .run(&[], Some(turn()), CancellationToken::new(), shutdown)
                .await?,
            RunOutcome::Interrupted
        );
        assert_eq!(h.sink.events().len(), cut);
        assert_eq!(h.provider.active_streams(), 0);
        let history = h.sink.frames();
        let point = tqa_context::resume_point(&history)?;
        let needs_calls = matches!(
            point,
            tqa_context::ResumePoint::StartRun { .. }
                | tqa_context::ResumePoint::RetryModel { step: 1, .. }
                | tqa_context::ResumePoint::NextStep { step: 1, .. }
        );
        let mut script = vec![];
        if needs_calls {
            script.push(calls(&[
                ("a", "read_file", r#"{"path":"a"}"#),
                ("b", "read_file", r#"{"path":"b"}"#),
            ]));
        }
        script.push(answer());
        let tool = fake("read_file", true, vec![output("a", 0), output("b", 0)])?;
        let mut resumed = Harness::new(script, registry(&[tool])?)?;
        resumed.sink_with(MemorySink::new(&history));
        assert_eq!(
            Runner::new(resumed.deps)?
                .run(
                    &history,
                    None,
                    CancellationToken::new(),
                    CancellationToken::new()
                )
                .await?,
            RunOutcome::Completed
        );
        assert!(tqa_context::project(&resumed.sink.frames()).is_ok());
        assert!(
            matches!(tqa_context::resume_point(&resumed.sink.frames())?,tqa_context::ResumePoint::Idle { dangling } if dangling.is_empty())
        );
        if cut > 1 {
            assert_eq!(resumed.sink.types()[cut], "run.resumed");
        }
    }
    Ok(())
}
#[tokio::test]
async fn core_u_005_cancel_during_provider_delay_drops_the_stream_promptly() -> TestResult {
    let mut scripted = answer();
    if let Ok(chunks) = &mut scripted.result {
        chunks[1].delay = Duration::from_secs(10);
    }
    let h = Harness::new(vec![scripted], ToolRegistry::new())?;
    let cancel = CancellationToken::new();
    let runner = Runner::new(h.deps)?;
    let run = runner.run(&[], Some(turn()), cancel.clone(), CancellationToken::new());
    tokio::pin!(run);
    assert!(futures::poll!(&mut run).is_pending());
    assert_eq!(h.provider.active_streams(), 1);
    cancel.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(200), run).await??,
        RunOutcome::Cancelled
    );
    assert_eq!(h.provider.active_streams(), 0);
    assert_eq!(
        h.sink.types().last().map(String::as_str),
        Some("run.cancelled")
    );
    Ok(())
}
#[tokio::test]
async fn core_u_006_cancel_during_tool_emits_end_and_joins_worker() -> TestResult {
    let tool = fake("shell", false, vec![output("late", 10_000)])?;
    let h = Harness::new(
        vec![calls(&[("s", "shell", r#"{"path":"s"}"#)])],
        registry(std::slice::from_ref(&tool))?,
    )?;
    let cancel = CancellationToken::new();
    let runner = Runner::new(h.deps)?;
    let run = runner.run(&[], Some(turn()), cancel.clone(), CancellationToken::new());
    tokio::pin!(run);
    assert!(futures::poll!(&mut run).is_pending());
    for _ in 0..100 {
        tokio::task::yield_now().await;
        if !tool.calls().is_empty() {
            break;
        }
    }
    assert_eq!(tool.calls().len(), 1);
    cancel.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(200), run).await??,
        RunOutcome::Cancelled
    );
    assert!(h.sink.events().contains(&Event::ToolEnd {
        call_id: "s".parse()?,
        status: ToolStatus::Cancelled,
        exit_code: None,
        failure: None,
    }));
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[cfg(unix)]
#[tokio::test]
async fn core_u_006_shutdown_stops_shell_and_resume_marks_it_interrupted() -> TestResult {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(tqa_tools::builtin::Shell::new()))?;
    let mut h = Harness::new(
        vec![calls(&[(
            "s",
            "shell",
            r#"{"command":"sleep 10","timeout_ms":20000}"#,
        )])],
        tools,
    )?;
    h.deps.clock = Arc::new(tqa_llm::SystemClock);
    h.deps.sleeper = Arc::new(tqa_llm::TokioSleeper);
    let shutdown = CancellationToken::new();
    let runner = Runner::new(h.deps)?;
    let run = runner.run(
        &[],
        Some(turn()),
        CancellationToken::new(),
        shutdown.clone(),
    );
    tokio::pin!(run);
    assert!(futures::poll!(&mut run).is_pending());
    tokio::time::sleep(Duration::from_millis(30)).await;
    shutdown.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), run).await??,
        RunOutcome::Interrupted
    );
    let history = h.sink.frames();
    assert!(h.sink.types().contains(&"tool.start".into()));
    assert!(!h.sink.types().contains(&"tool.end".into()));
    let mut resumed = Harness::new(vec![answer()], ToolRegistry::new())?;
    resumed.sink_with(MemorySink::new(&history));
    Runner::new(resumed.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(
        tool_results(&resumed.sink.events())[0].1,
        ToolStatus::Interrupted
    );
    assert_closed(&resumed.sink.frames())?;
    Ok(())
}
