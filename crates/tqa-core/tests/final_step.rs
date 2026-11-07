//! CORE-E-007: budgets stop additional work, not a final answer already produced.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event};

#[tokio::test]
async fn core_e_007_final_answer_on_the_only_allowed_step_completes() -> TestResult {
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.deps.limits.max_steps = 1;
    let result = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(result, RunOutcome::Completed);
    assert_eq!(h.provider.requests().len(), 1);
    assert!(!h.sink.types().contains(&"budget.warning".into()));
    assert!(!h.sink.types().contains(&"run.failed".into()));
    assert_closed(&h.sink.frames())?;
    Ok(())
}

#[tokio::test]
async fn core_e_007_resume_of_a_final_step_completes_with_spent_budgets() -> TestResult {
    for duration_ms in [0, 1000] {
        let mut events = initial()?;
        events.extend([
            Event::TextDelta {
                text: "final answer".into(),
            },
            model(),
            Event::StepCompleted {
                step: 1,
                duration_ms,
            },
        ]);
        let frames = history(events)?;
        let mut h = Harness::new(vec![], ToolRegistry::new())?;
        h.deps.limits.max_steps = 1;
        h.deps.limits.wall_clock_secs = 1;
        h.sink_with(MemorySink::new(&frames));
        let result = Runner::new(h.deps)?
            .run(
                &frames,
                None,
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await?;
        assert_eq!(result, RunOutcome::Completed);
        assert!(h.provider.requests().is_empty());
        assert_eq!(
            &h.sink.types()[frames.len()..],
            ["run.resumed", "run.completed"]
        );
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}

#[tokio::test]
async fn core_e_007_only_allowed_tool_step_still_refuses_the_next_model_call() -> TestResult {
    let tool = fake("read_file", true, vec![output("content", 0)])?;
    let mut h = Harness::new(
        vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
        registry(&[tool])?,
    )?;
    h.deps.limits.max_steps = 1;
    let result = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert!(matches!(
        result,
        RunOutcome::Failed {
            code: ErrorCode::Budget,
            ..
        }
    ));
    assert_eq!(h.provider.requests().len(), 1);
    assert!(h.sink.types().contains(&"budget.warning".into()));
    assert_closed(&h.sink.frames())?;
    Ok(())
}
