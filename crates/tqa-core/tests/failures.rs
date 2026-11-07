//! CORE-E-007/009/010/011/012/029 and CORE-U-017/018: model and tool failures remain actionable.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_tools::{ToolError, ToolRegistry};
use tqa_types::{BudgetKind, ErrorCode, Event, ToolStatus};

#[tokio::test]
async fn core_e_009_transient_status_retries_with_bounded_event_reason() -> TestResult {
    let h = Harness::new(vec![http(503), answer()], ToolRegistry::new())?;
    let runner = Runner::new(h.deps)?;
    assert_eq!(
        drive(
            runner.run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            ),
            &h.clock
        )
        .await??,
        RunOutcome::Completed
    );
    assert_eq!(h.provider.requests().len(), 2);
    assert!(h.sink.events().contains(&Event::ProviderRetry {
        attempt: 1,
        delay_ms: 1500,
        reason: "http 503".into(),
        provider_message: Some("private-response-marker".into()),
        fallback_problem: None,
    }));
    // The body reaches events only as the sanitised `provider_message` (CORE-U-052), never in a reason.
    assert!(h.sink.events().iter().all(|event| !matches!(event,
        Event::ProviderRetry { reason, .. } | Event::RunFailed { reason, .. } if reason.contains("private-response-marker"))));
    Ok(())
}
#[tokio::test]
async fn core_e_010_provider_exhaustion_and_nonretryable_statuses() -> TestResult {
    for (status, attempts) in [(503, 3), (400, 1), (401, 1), (404, 1)] {
        let h = Harness::new((0..3).map(|_| http(status)).collect(), ToolRegistry::new())?;
        let runner = Runner::new(h.deps)?;
        let outcome = drive(
            runner.run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            ),
            &h.clock,
        )
        .await??;
        assert!(
            matches!(outcome,RunOutcome::Failed { code:ErrorCode::ProviderError,ref reason } if reason.contains(&status.to_string()))
        );
        assert_eq!(h.provider.requests().len(), attempts);
        assert_eq!(
            h.sink
                .types()
                .iter()
                .filter(|t| *t == "provider.retry")
                .count(),
            attempts - 1
        );
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}
#[tokio::test]
async fn core_e_011_stream_cut_retries_without_projecting_partial_text() -> TestResult {
    let h = Harness::new(
        vec![
            reply(vec![StreamChunk::TextDelta("discard partial".into())]),
            answer(),
        ],
        ToolRegistry::new(),
    )?;
    let runner = Runner::new(h.deps)?;
    drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert!(h.sink.events().contains(&Event::StepStarted {
        step: 1,
        attempt: 2
    }));
    assert!(
        !tqa_context::project(&h.sink.frames())?
            .iter()
            .any(|message| message.text.contains("discard partial"))
    );
    assert!(h.sink.events().contains(&Event::TextDelta {
        text: "discard partial".into()
    }));
    Ok(())
}
#[tokio::test]
async fn core_e_029_empty_or_reasoning_only_response_retries_once() -> TestResult {
    for reasoning in [false, true] {
        let h = Harness::new(
            vec![
                empty_response(reasoning),
                empty_response(reasoning),
                answer(),
            ],
            ToolRegistry::new(),
        )?;
        let runner = Runner::new(h.deps)?;
        assert_eq!(
            drive(
                runner.run(
                    &[],
                    Some(turn()),
                    CancellationToken::new(),
                    CancellationToken::new()
                ),
                &h.clock
            )
            .await??,
            RunOutcome::Failed {
                code: ErrorCode::ProviderError,
                reason: "empty_response".into()
            }
        );
        assert_eq!(h.provider.requests().len(), 2);
        assert_eq!(tqa_context::project(&h.sink.frames())?.len(), 1);
    }
    Ok(())
}
#[tokio::test]
async fn core_u_017_core_u_018_unknown_schema_and_raw_json_errors_do_not_dispatch() -> TestResult {
    for (name, args, detail) in [
        ("absent", r#"{"path":"a"}"#, "read_file"),
        ("read_file", r#"{"path":42}"#, "expected string"),
        ("read_file", "{", "line 1"),
        ("read_file", "[]", "object"),
    ] {
        let tool = fake("read_file", true, vec![])?;
        let h = Harness::new(
            vec![calls(&[("a", name, args)]), answer()],
            registry(std::slice::from_ref(&tool))?,
        )?;
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await?;
        let results = tool_results(&h.sink.events());
        assert_eq!(results[0].1, ToolStatus::Error);
        assert!(results[0].2.contains(detail), "{}", results[0].2);
        assert!(tool.calls().is_empty());
        assert!(!h.sink.types().contains(&"tool.start".into()));
        assert!(!h.sink.types().contains(&"tool.end".into()));
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}
#[tokio::test]
async fn core_e_012_third_identical_error_stops_before_a_fourth_dispatch() -> TestResult {
    let failure = || tqa_testkit::FakeToolReply {
        delay: std::time::Duration::ZERO,
        result: Err(ToolError::NotFound),
    };
    let tool = fake(
        "read_file",
        true,
        vec![failure(), failure(), failure(), failure()],
    )?;
    let h = Harness::new(
        (0..4)
            .map(|_| calls(&[("a", "read_file", r#"{"path":"missing"}"#)]))
            .collect(),
        registry(std::slice::from_ref(&tool))?,
    )?;
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
            code: ErrorCode::RepeatedFailure,
            ..
        }
    ));
    assert_eq!(tool.calls().len(), 3);
    assert_eq!(tool_results(&h.sink.events()).len(), 3);
    assert_eq!(h.provider.requests().len(), 3);
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[tokio::test]
async fn core_e_007_step_warning_at_eighty_percent_and_hard_limit() -> TestResult {
    let tool = fake(
        "read_file",
        true,
        (0..10).map(|_| output("ok", 0)).collect(),
    )?;
    let mut h = Harness::new(
        (0..11)
            .map(|_| calls(&[("a", "read_file", r#"{"path":"file"}"#)]))
            .collect(),
        registry(&[tool])?,
    )?;
    h.deps.limits.max_steps = 10;
    assert!(matches!(
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Failed {
            code: ErrorCode::Budget,
            ..
        }
    ));
    assert_eq!(h.provider.requests().len(), 10);
    let warnings: Vec<_> = h
        .sink
        .events()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::BudgetWarning {
                    budget: BudgetKind::Steps,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        warnings,
        vec![Event::BudgetWarning {
            budget: BudgetKind::Steps,
            used: 8,
            limit: 10
        }]
    );
    assert_eq!(
        h.sink
            .types()
            .iter()
            .filter(|kind| *kind == "step.completed")
            .count(),
        10
    );
    Ok(())
}
#[tokio::test]
async fn core_e_007_context_budget_fails_before_provider_request() -> TestResult {
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.deps.route.context_window = Some(4);
    h.deps.route.max_output = Some(4);
    assert_eq!(
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Failed {
            code: ErrorCode::Budget,
            reason: "context window exceeded".into()
        }
    );
    assert!(h.provider.requests().is_empty());
    Ok(())
}

fn empty_response(reasoning: bool) -> tqa_testkit::ProviderReply {
    let mut chunks = Vec::new();
    if reasoning {
        chunks.push(StreamChunk::ReasoningDelta("private reasoning".into()));
    }
    chunks.push(StreamChunk::Finish(FinishReason::Stop));
    reply(chunks)
}

#[tokio::test]
async fn core_e_012_parallel_duplicate_failures_never_launch_a_fourth_execution() -> TestResult {
    let failure = || tqa_testkit::FakeToolReply {
        delay: std::time::Duration::ZERO,
        result: Err(ToolError::NotFound),
    };
    let tool = fake(
        "read_file",
        true,
        vec![failure(), failure(), failure(), failure()],
    )?;
    let h = Harness::new(
        vec![calls(&[
            ("a", "read_file", r#"{"path":"same"}"#),
            ("b", "read_file", r#"{"path":"same"}"#),
            ("c", "read_file", r#"{"path":"same"}"#),
            ("d", "read_file", r#"{"path":"same"}"#),
        ])],
        registry(std::slice::from_ref(&tool))?,
    )?;
    assert!(matches!(
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Failed {
            code: ErrorCode::RepeatedFailure,
            ..
        }
    ));
    assert_eq!(tool.calls().len(), 3);
    assert_eq!(
        tool_results(&h.sink.events())
            .iter()
            .filter(|(_, status, _)| *status == ToolStatus::Error)
            .count(),
        3
    );
    assert_closed(&h.sink.frames())?;
    Ok(())
}

#[tokio::test]
async fn core_e_029_shutdown_during_empty_retry_never_resumes_as_a_completed_answer() -> TestResult
{
    let shutdown = CancellationToken::new();
    let mut h = Harness::new(
        vec![reply(vec![
            StreamChunk::ReasoningDelta("reasoning only".into()),
            usage(),
            StreamChunk::Finish(FinishReason::Stop),
        ])],
        ToolRegistry::new(),
    )?;
    let mut sink = MemorySink::new(&[]);
    sink.cut = Some((6, shutdown.clone()));
    h.sink_with(sink);
    assert_eq!(
        Runner::new(h.deps)?
            .run(&[], Some(turn()), CancellationToken::new(), shutdown)
            .await?,
        RunOutcome::Interrupted
    );
    let frames = h.sink.frames();
    assert!(matches!(
        tqa_context::resume_point(&frames)?,
        tqa_context::ResumePoint::RetryModel {
            step: 1,
            attempt: 2,
            ..
        }
    ));
    assert_eq!(
        h.sink.types().last().map(String::as_str),
        Some("provider.retry")
    );
    assert_eq!(tqa_context::project(&frames)?.len(), 1);
    let mut resumed = Harness::new(vec![answer()], ToolRegistry::new())?;
    resumed.sink_with(MemorySink::new(&frames));
    assert_eq!(
        Runner::new(resumed.deps)?
            .run(
                &frames,
                None,
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Completed
    );
    assert_eq!(resumed.provider.requests().len(), 1);
    assert_closed(&resumed.sink.frames())?;
    Ok(())
}
