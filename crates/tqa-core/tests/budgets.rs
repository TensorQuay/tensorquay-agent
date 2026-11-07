//! Run-wide token accounting and active-time ceilings retain the partial response.
mod support;
use std::time::Duration;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk, Usage};
use tqa_tools::ToolRegistry;
use tqa_types::{BudgetKind, ErrorCode, Event};

fn tokens(count: u64) -> StreamChunk {
    StreamChunk::Usage(Usage {
        prompt_tokens: 10,
        completion_tokens: count,
        cached_tokens: None,
        reasoning_tokens: None,
    })
}
fn failure(reason: &str) -> RunOutcome {
    RunOutcome::Failed {
        code: ErrorCode::Budget,
        reason: reason.into(),
    }
}
fn warnings(events: &[Event]) -> Vec<(u64, u64)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::BudgetWarning {
                budget: BudgetKind::OutputTokens,
                used,
                limit,
            } => Some((*used, *limit)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_e_008_final_answer_at_the_exact_token_ceiling_succeeds() -> TestResult {
    for limit in [4, 5, 6] {
        let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
        h.deps.limits.max_output_tokens = limit;
        let outcome = Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await?;
        assert_eq!(
            outcome,
            if limit == 4 {
                failure("budget.output_tokens")
            } else {
                RunOutcome::Completed
            }
        );
        assert_eq!(
            h.provider.requests()[0].max_output,
            Some(u32::try_from(limit)?)
        );
        assert_eq!(h.provider.requests().len(), 1);
        assert_eq!(warnings(&h.sink.events()), vec![(5, limit)]);
        assert!(h.sink.events().contains(&Event::TextDelta {
            text: "answer".into()
        }));
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}

#[tokio::test]
async fn core_e_008_missing_usage_stops_a_long_stream_with_an_explicit_estimate() -> TestResult {
    for chunk in [
        StreamChunk::TextDelta("x".repeat(40)),
        StreamChunk::ReasoningDelta("x".repeat(40)),
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some("call".into()),
            name: Some("read_file".into()),
            arguments_delta: "x".repeat(40),
        },
    ] {
        let mut h = Harness::new(
            vec![reply(vec![
                StreamChunk::TextDelta("partial".into()),
                chunk,
                StreamChunk::TextDelta("must not escape".into()),
                StreamChunk::Finish(FinishReason::Stop),
            ])],
            ToolRegistry::new(),
        )?;
        h.deps.limits.max_output_tokens = 10;
        assert_eq!(
            Runner::new(h.deps)?
                .run(
                    &[],
                    Some(turn()),
                    CancellationToken::new(),
                    CancellationToken::new()
                )
                .await?,
            failure("budget.output_tokens")
        );
        let events = h.sink.events();
        assert!(events.contains(&Event::TextDelta {
            text: "partial".into()
        }));
        assert!(!events.contains(&Event::TextDelta {
            text: "must not escape".into()
        }));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Usage { estimated: true,
            completion_tokens, .. } if *completion_tokens > 10))
        );
        assert_eq!(h.provider.active_streams(), 0);
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}

#[tokio::test]
async fn core_e_008_retried_attempts_consume_the_run_budget_even_without_provider_usage()
-> TestResult {
    for reported in [false, true] {
        let mut chunks = vec![StreamChunk::TextDelta("partial".into())];
        if reported {
            chunks.push(tokens(2));
        }
        let mut h = Harness::new(
            vec![
                reply(chunks),
                reply(vec![
                    StreamChunk::TextDelta("ok".into()),
                    tokens(3),
                    StreamChunk::Finish(FinishReason::Stop),
                ]),
            ],
            ToolRegistry::new(),
        )?;
        h.deps.limits.max_output_tokens = 5;
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
        assert_eq!(
            h.provider
                .requests()
                .iter()
                .map(|r| r.max_output)
                .collect::<Vec<_>>(),
            vec![Some(5), Some(3)]
        );
        let usage: Vec<_> = h
            .sink
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::Usage {
                    completion_tokens,
                    estimated,
                    ..
                } => Some((completion_tokens, estimated)),
                _ => None,
            })
            .collect();
        assert_eq!(usage, vec![(2, !reported), (3, false)]);
        assert_eq!(warnings(&h.sink.events()), vec![(5, 5)]);
        assert_eq!(
            tqa_context::project(&h.sink.frames())?
                .last()
                .ok_or("missing answer")?
                .text,
            "ok"
        );
    }
    Ok(())
}

#[tokio::test]
async fn core_e_008_output_after_an_early_usage_report_is_still_bounded() -> TestResult {
    let mut h = Harness::new(
        vec![reply(vec![
            tokens(0),
            StreamChunk::TextDelta("x".repeat(40)),
            StreamChunk::TextDelta("must not escape".into()),
            StreamChunk::Finish(FinishReason::Stop),
        ])],
        ToolRegistry::new(),
    )?;
    h.deps.limits.max_output_tokens = 10;
    assert_eq!(
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        failure("budget.output_tokens")
    );
    let events = h.sink.events();
    assert!(!events.contains(&Event::TextDelta {
        text: "must not escape".into()
    }));
    assert!(events.iter().any(|event| matches!(event,
        Event::Usage { estimated: true, completion_tokens, .. } if *completion_tokens > 10)));
    assert_closed(&h.sink.frames())?;
    Ok(())
}

#[tokio::test]
async fn core_e_008_resume_preserves_usage_and_does_not_repeat_the_warning() -> TestResult {
    let mut events = initial()?;
    events.extend([
        call("a")?,
        Event::Usage {
            prompt_tokens: 10,
            completion_tokens: 8,
            cached_tokens: None,
            reasoning_tokens: None,
            estimated: false,
            estimated_input: None,
            estimated_images: None,
            estimated_route: None,
        },
        model(),
        Event::ToolResult {
            call_id: "a".parse()?,
            status: tqa_types::ToolStatus::Ok,
            content: "done".into(),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::BudgetWarning {
            budget: BudgetKind::OutputTokens,
            used: 8,
            limit: 10,
        },
    ]);
    let history = history(events)?;
    let mut h = Harness::new(
        vec![reply(vec![
            StreamChunk::TextDelta("ok".into()),
            tokens(2),
            StreamChunk::Finish(FinishReason::Stop),
        ])],
        ToolRegistry::new(),
    )?;
    h.deps.limits.max_output_tokens = 10;
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
    assert_eq!(h.provider.requests()[0].max_output, Some(2));
    assert_eq!(warnings(&h.sink.events()), vec![(8, 10)]);
    Ok(())
}

#[tokio::test]
async fn core_e_008_exhausted_tool_response_and_length_finish_never_start_another_step()
-> TestResult {
    let mut tool_response = calls(&[("a", "read_file", r#"{"path":"a"}"#)]);
    if let Ok(chunks) = &mut tool_response.result {
        chunks
            .iter_mut()
            .filter(|c| matches!(c.result, Ok(StreamChunk::Usage(_))))
            .for_each(|c| c.result = Ok(tokens(100)));
    }
    for script in [
        tool_response,
        reply(vec![
            StreamChunk::TextDelta("partial".into()),
            tokens(100),
            StreamChunk::Finish(FinishReason::Length),
        ]),
    ] {
        let tool = fake("read_file", true, vec![])?;
        let mut h = Harness::new(
            vec![script, answer()],
            registry(std::slice::from_ref(&tool))?,
        )?;
        h.deps.limits.max_output_tokens = 100;
        assert_eq!(
            Runner::new(h.deps)?
                .run(
                    &[],
                    Some(turn()),
                    CancellationToken::new(),
                    CancellationToken::new()
                )
                .await?,
            failure("budget.output_tokens")
        );
        assert_eq!(h.provider.requests().len(), 1);
        assert!(tool.calls().is_empty());
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}

#[tokio::test]
async fn core_e_008_wall_clock_ceiling_keeps_partial_text_and_cancels_the_stream() -> TestResult {
    let mut scripted = answer();
    if let Ok(chunks) = &mut scripted.result {
        chunks[1].delay = Duration::from_secs(2);
    }
    let mut h = Harness::new(vec![scripted], ToolRegistry::new())?;
    h.deps.limits.wall_clock_secs = 1;
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
        failure("budget.wall_clock")
    );
    assert!(h.sink.events().contains(&Event::TextDelta {
        text: "final ".into()
    }));
    assert!(!h.sink.events().contains(&Event::TextDelta {
        text: "answer".into()
    }));
    assert_eq!(h.provider.active_streams(), 0);
    assert_closed(&h.sink.frames())?;
    Ok(())
}
