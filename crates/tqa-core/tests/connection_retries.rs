//! CORE-E-009/010/011: restart-sized backoff, exact attempts, and superseded output.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{ConnectionFailure, FinishReason, LlmError, StreamChunk};
use tqa_testkit::{FixedJitter, ProviderReply};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event};

#[tokio::test]
async fn core_e_010_transient_connection_attempts_succeed_or_exhaust_exactly_three() -> TestResult {
    for succeeds in [false, true] {
        let failure = || ProviderReply {
            delay: std::time::Duration::ZERO,
            result: Err(LlmError::Connection(ConnectionFailure::Refused)),
        };
        let h = Harness::new(
            vec![
                failure(),
                failure(),
                if succeeds { answer() } else { failure() },
            ],
            ToolRegistry::new(),
        )?;
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
        assert_eq!(
            outcome,
            if succeeds {
                RunOutcome::Completed
            } else {
                RunOutcome::Failed {
                    code: ErrorCode::ProviderError,
                    reason: "io".into(),
                }
            }
        );
        let sequence: Vec<_> = h
            .sink
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::StepStarted { attempt, .. } => Some(("attempt", *attempt)),
                Event::ProviderRetry {
                    attempt, reason, ..
                } => {
                    assert_eq!(reason, "io");
                    Some(("retry", *attempt))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            sequence,
            [
                ("attempt", 1),
                ("retry", 1),
                ("attempt", 2),
                ("retry", 2),
                ("attempt", 3)
            ]
        );
        assert_eq!(h.provider.requests().len(), 3);
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}

#[tokio::test]
async fn core_e_009_retry_window_covers_restart_even_with_minimum_jitter() -> TestResult {
    for fraction in [0.0, 0.5, 1.0 - f64::EPSILON] {
        let mut h = Harness::new(vec![http(503), http(503), answer()], ToolRegistry::new())?;
        h.deps.jitter = Box::new(FixedJitter::new(fraction)?);
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
        assert_eq!(outcome, RunOutcome::Completed);
        let attempts: Vec<_> = h
            .sink
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::StepStarted { attempt, .. } => Some(*attempt),
                _ => None,
            })
            .collect();
        let retries: Vec<_> = h
            .sink
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::ProviderRetry {
                    attempt, delay_ms, ..
                } => Some((*attempt, *delay_ms)),
                _ => None,
            })
            .collect();
        assert_eq!(attempts, [1, 2, 3]);
        assert_eq!(retries.len(), 2);
        assert_eq!((retries[0].0, retries[1].0), (1, 2));
        assert!((1000..=2000).contains(&retries[0].1));
        assert!((2000..=4000).contains(&retries[1].1));
        assert!((3000..=6000).contains(&(retries[0].1 + retries[1].1)));
        assert_eq!(h.provider.requests().len(), 3);
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}

#[tokio::test]
async fn core_e_011_retried_stream_projects_one_answer_without_a_repeated_prefix() -> TestResult {
    let h = Harness::new(
        vec![
            reply(vec![StreamChunk::TextDelta("shared prefix".into())]),
            reply(vec![
                StreamChunk::TextDelta("shared prefix complete".into()),
                StreamChunk::Finish(FinishReason::Stop),
            ]),
        ],
        ToolRegistry::new(),
    )?;
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
    assert_eq!(outcome, RunOutcome::Completed);
    let projected = tqa_context::project(&h.sink.frames())?;
    let answers: Vec<_> = projected
        .iter()
        .filter(|message| message.text.contains("shared prefix"))
        .collect();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].text, "shared prefix complete");
    assert_eq!(h.provider.requests().len(), 2);
    assert!(h.sink.events().contains(&Event::TextDelta {
        text: "shared prefix".into()
    }));
    assert!(h.sink.events().contains(&Event::ModelCompleted {
        step: 1,
        attempt: 2,
        finish_reason: tqa_types::FinishReason::Stop
    }));
    Ok(())
}
