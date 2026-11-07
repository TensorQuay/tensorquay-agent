//! CORE-U-038: every recovery state uses only the journal and preserves acknowledged work.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event, ToolStatus};

#[tokio::test]
// TEST: CORE-U-020
async fn core_u_038_tool_phase_keeps_done_interrupts_started_dispatches_unstarted() -> TestResult {
    let mut events = initial()?;
    events.extend([
        call("a")?,
        call("b")?,
        call("c")?,
        model(),
        Event::ToolResult {
            call_id: "a".parse()?,
            status: ToolStatus::Ok,
            content: "already done".into(),
            target: None,
            evidence: None,
        },
        Event::ToolStart {
            call_id: "b".parse()?,
        },
    ]);
    let history = history(events)?;
    let tool = fake("read_file", true, vec![output("new result", 0)])?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    h.sink_with(MemorySink::new(&history));
    assert_eq!(
        Runner::new(h.deps)?
            .run(
                &history,
                None,
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Completed
    );
    assert_eq!(tool.calls(), vec![serde_json::json!({"path":"c"})]);
    assert_eq!(h.provider.requests().len(), 1);
    assert_resumed(&history, &h.sink.frames())?;
    assert_eq!(
        tool_results(&h.sink.events())[1],
        (
            "b".into(),
            ToolStatus::Interrupted,
            "the sandbox stopped while this tool ran; it may be retried".into()
        )
    );
    assert!(h.sink.types()[history.len()..].starts_with(&[
        "run.resumed".into(),
        "tool.end".into(),
        "tool.result".into(),
        "tool.start".into()
    ]));
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[tokio::test]
// TEST: CORE-U-020
async fn core_u_038_partial_highest_attempt_retries_without_resurrecting_previous() -> TestResult {
    let mut events = initial()?;
    events.extend([
        Event::TextDelta { text: "old".into() },
        model(),
        Event::StepStarted {
            step: 1,
            attempt: 2,
        },
        Event::TextDelta {
            text: "partial".into(),
        },
    ]);
    let history = history(events)?;
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.sink_with(MemorySink::new(&history));
    Runner::new(h.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(h.provider.requests().len(), 1);
    assert_resumed(&history, &h.sink.frames())?;
    assert!(h.sink.events().contains(&Event::StepStarted {
        step: 1,
        attempt: 3
    }));
    assert_eq!(
        tqa_context::project(&h.sink.frames())?
            .last()
            .ok_or("missing answer")?
            .text,
        "final answer"
    );
    Ok(())
}
#[tokio::test]
async fn core_u_038_unanswered_message_is_busy_then_starts_its_deterministic_run() -> TestResult {
    let history = history(vec![Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: "question".into(),
        attachments: vec![],
    }])?;
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.sink_with(MemorySink::new(&history));
    let runner = Runner::new(h.deps)?;
    assert_eq!(
        runner
            .run(
                &history,
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await
            .err()
            .ok_or("expected busy")?
            .code(),
        ErrorCode::Busy
    );
    assert_eq!(h.sink.frames(), history);
    assert_eq!(
        runner
            .run(
                &history,
                None,
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Completed
    );
    assert_eq!(
        h.sink.events()[1],
        Event::RunStarted {
            run_id: "r-1".parse()?
        }
    );
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[tokio::test]
async fn core_u_038_completed_no_call_step_finishes_without_another_model() -> TestResult {
    let mut events = initial()?;
    events.extend([
        Event::TextDelta {
            text: "answer".into(),
        },
        model(),
        Event::StepCompleted {
            step: 1,
            duration_ms: 10,
        },
    ]);
    let history = history(events)?;
    let mut h = Harness::new(vec![], ToolRegistry::new())?;
    h.sink_with(MemorySink::new(&history));
    Runner::new(h.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert!(h.provider.requests().is_empty());
    assert_eq!(
        &h.sink.types()[history.len()..],
        &["run.resumed", "run.completed"]
    );
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[tokio::test]
async fn core_u_038_next_step_preserves_completed_duration_and_warning() -> TestResult {
    let mut events = initial()?;
    events.extend([
        call("a")?,
        model(),
        Event::ToolResult {
            call_id: "a".parse()?,
            status: ToolStatus::Ok,
            content: "done".into(),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 800,
        },
        Event::BudgetWarning {
            budget: tqa_types::BudgetKind::WallClock,
            used: 0,
            limit: 1,
        },
    ]);
    let history = history(events)?;
    let tool = fake("read_file", true, vec![output("late", 500)])?;
    let mut h = Harness::new(
        vec![calls(&[("b", "read_file", r#"{"path":"b"}"#)])],
        registry(&[tool])?,
    )?;
    h.deps.limits.wall_clock_secs = 1;
    h.sink_with(MemorySink::new(&history));
    let runner = Runner::new(h.deps)?;
    assert!(matches!(
        drive(
            runner.run(
                &history,
                None,
                CancellationToken::new(),
                CancellationToken::new()
            ),
            &h.clock
        )
        .await??,
        RunOutcome::Failed {
            code: ErrorCode::Budget,
            ..
        }
    ));
    assert_eq!(
        h.sink
            .types()
            .iter()
            .filter(|kind| *kind == "budget.warning")
            .count(),
        1
    );
    assert!(
        tool_results(&h.sink.events())
            .iter()
            .any(|(id, status, text)| id == "b"
                && *status == ToolStatus::Cancelled
                && text == "stopped: budget.wall_clock")
    );
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[tokio::test]
async fn core_u_038_ended_run_dangling_calls_close_before_next_user_message() -> TestResult {
    let mut events = initial()?;
    events.extend([
        call("a")?,
        model(),
        Event::RunFailed {
            run_id: "r-1".parse()?,
            code: ErrorCode::SandboxFailed,
            reason: "stopped".into(),
            provider_message: None,
        },
    ]);
    let history = history(events)?;
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.sink_with(MemorySink::new(&history));
    Runner::new(h.deps)?
        .run(
            &history,
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert!(matches!(
        &h.sink.events()[history.len()],
        Event::ToolResult {
            status: ToolStatus::Interrupted,
            ..
        }
    ));
    assert!(matches!(
        &h.sink.events()[history.len() + 1],
        Event::MessageUser { .. }
    ));
    assert!(tqa_context::project(&h.sink.frames()).is_ok());
    Ok(())
}

#[tokio::test]
async fn core_e_007_resume_after_last_allowed_step_never_starts_another() -> TestResult {
    let mut events = initial()?;
    events.extend([
        call("a")?,
        model(),
        Event::ToolResult {
            call_id: "a".parse()?,
            status: ToolStatus::Ok,
            content: "done".into(),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
    ]);
    let history = history(events)?;
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.deps.limits.max_steps = 1;
    h.sink_with(MemorySink::new(&history));
    assert!(matches!(
        Runner::new(h.deps)?
            .run(
                &history,
                None,
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Failed {
            code: ErrorCode::Budget,
            ..
        }
    ));
    assert!(h.provider.requests().is_empty());
    assert!(!h.sink.events().contains(&Event::StepStarted {
        step: 2,
        attempt: 1
    }));
    Ok(())
}

#[tokio::test]
async fn core_e_007_wall_warning_records_active_step_time_once() -> TestResult {
    let tool = fake("read_file", true, vec![output("done", 1600)])?;
    let mut h = Harness::new(
        vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.deps.limits.wall_clock_secs = 2;
    let runner = Runner::new(h.deps)?;
    let run = runner.run(
        &[],
        Some(turn()),
        CancellationToken::new(),
        CancellationToken::new(),
    );
    tokio::pin!(run);
    assert!(futures::poll!(&mut run).is_pending());
    for _ in 0..100 {
        tokio::task::yield_now().await;
        if !tool.calls().is_empty() {
            break;
        }
    }
    assert_eq!(tool.calls().len(), 1);
    h.clock.advance(std::time::Duration::from_millis(1600))?;
    assert_eq!(run.await?, RunOutcome::Completed);
    assert!(h.sink.events().contains(&Event::StepCompleted {
        step: 1,
        duration_ms: 1600
    }));
    let warnings: Vec<_> = h
        .sink
        .events()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::BudgetWarning {
                    budget: tqa_types::BudgetKind::WallClock,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        warnings,
        vec![Event::BudgetWarning {
            budget: tqa_types::BudgetKind::WallClock,
            used: 1,
            limit: 2
        }]
    );
    Ok(())
}
