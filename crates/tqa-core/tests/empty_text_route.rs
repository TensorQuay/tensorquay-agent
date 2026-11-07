//! CORE-U-055: an empty answer on a text-protocol route fails with a reason that says what was seen; a native-tool
//! route still fails `empty_response`. A text-protocol route gives an empty reply up to three tries in all, the step's
//! whole attempt budget, since a server there drops a reply's text now and then and a retry often gets it.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event};

/// Runs a turn whose model replies are `count` empty completions (reasoning only), then, with `answer`, a plain one.
async fn empties(
    count: usize,
    native: bool,
    answer: bool,
) -> Result<(RunOutcome, Vec<Event>), Box<dyn std::error::Error>> {
    let empty = || {
        reply(vec![
            StreamChunk::ReasoningDelta("thinking".into()),
            StreamChunk::Finish(FinishReason::Stop),
        ])
    };
    let mut script: Vec<_> = (0..count).map(|_| empty()).collect();
    if answer {
        script.push(reply(vec![
            StreamChunk::TextDelta("Done.".into()),
            StreamChunk::Finish(FinishReason::Stop),
        ]));
    }
    let mut h = Harness::new(script, ToolRegistry::new())?;
    h.deps.route.native_tools = native;
    let sink = h.sink.clone();
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
    Ok((outcome, sink.events()))
}

fn retry_reasons(events: &[Event]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry { reason, .. } => Some(reason.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_u_055_an_empty_answer_on_a_text_protocol_route_names_itself() -> TestResult {
    let (outcome, events) = empties(3, false, false).await?;
    let RunOutcome::Failed { code, reason } = outcome else {
        return Err(format!("expected a failure, got {outcome:?}").into());
    };
    assert_eq!(code, ErrorCode::ProviderError);
    assert!(
        reason.starts_with("empty_response"),
        "still an empty response: {reason}"
    );
    assert!(reason.contains("text-protocol route"), "{reason}");
    assert!(
        reason.contains("after its reasoning"),
        "names what was seen: {reason}"
    );
    assert!(
        events.iter().any(
            |event| matches!(event, Event::RunFailed { reason: failed, .. } if *failed == reason)
        ),
        "run.failed carries the same reason"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_055_a_native_tool_route_is_unchanged() -> TestResult {
    let (outcome, events) = empties(2, true, false).await?;
    assert_eq!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            reason: "empty_response".into()
        }
    );
    assert_eq!(retry_reasons(&events), ["empty_response"], "one retry");
    Ok(())
}

/// The retry after an empty answer is labelled `empty_response`, not the transport class `protocol`, so the retry and
/// the failure name the same thing (28 Sep: six `protocol` retries in one fallback run).
#[tokio::test]
async fn core_u_055_the_retry_names_the_empty_response() -> TestResult {
    for (count, native) in [(3, false), (2, true)] {
        let (_, events) = empties(count, native, false).await?;
        assert!(
            retry_reasons(&events)
                .iter()
                .all(|reason| *reason == "empty_response"),
            "native tools {native}: {:?}",
            retry_reasons(&events)
        );
    }
    Ok(())
}

/// On a text-protocol route, a reply that comes back after two empty ones completes the run: three tries in all, each
/// retry visible. A native-tool route still stops at the second empty reply.
#[tokio::test]
async fn core_u_055_a_text_protocol_route_gives_an_empty_reply_three_tries() -> TestResult {
    let (outcome, events) = empties(2, false, true).await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(retry_reasons(&events), ["empty_response", "empty_response"]);
    let (outcome, _) = empties(2, true, true).await?;
    assert!(
        matches!(
            outcome,
            RunOutcome::Failed {
                code: ErrorCode::ProviderError,
                ..
            }
        ),
        "a native-tool route keeps one retry: {outcome:?}"
    );
    Ok(())
}

/// The three tries are the step's existing attempt budget, shared with other retryable failures: an empty reply, a
/// server error and another empty reply use it up, and the step fails without a fourth request.
#[tokio::test]
async fn core_u_055_empty_tries_share_the_steps_attempt_budget() -> TestResult {
    let empty = || {
        reply(vec![
            StreamChunk::ReasoningDelta("thinking".into()),
            StreamChunk::Finish(FinishReason::Stop),
        ])
    };
    let mut h = Harness::new(
        vec![empty(), http(500), empty(), answer()],
        ToolRegistry::new(),
    )?;
    h.deps.route.native_tools = false;
    let (sink, provider) = (h.sink.clone(), h.provider.clone());
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
        matches!(
            outcome,
            RunOutcome::Failed {
                code: ErrorCode::ProviderError,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(provider.requests().len(), 3, "no fourth request");
    assert_eq!(retry_reasons(&sink.events()).len(), 2);
    Ok(())
}
