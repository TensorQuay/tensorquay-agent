//! CTX-E-004: one tool result larger than the whole context window is cut to fit with a marker, and the run goes on.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_context::{MIN_RESULT_BYTES, estimate};
use tqa_core::{RunOutcome, Runner};
use tqa_llm::ContentPart;

/// A small window, so one capped result overflows it on its own.
const WINDOW: u32 = 4096;
const RESERVE: u32 = 512;
/// The result cap stays above the window, so only the context fitting can keep the request inside it.
const RESULT_CAP: u32 = 64 * 1024;

fn text_of(request: &tqa_llm::CompletionRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .map(|message| {
            message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn ctx_e_004_a_result_larger_than_the_window_is_cut_with_a_marker_and_the_run_continues()
-> TestResult {
    let big = oversized("S");
    assert!(
        u64::from(estimate(&big).tokens) > u64::from(WINDOW),
        "the result alone must exceed the window"
    );
    let tool = fake("read_file", true, vec![output(&big, 0)])?;
    let mut h = Harness::new(
        vec![
            calls(&[("big", "read_file", r#"{"path":"big"}"#)]),
            answer(),
            answer(),
        ],
        registry(&[tool])?,
    )?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.tool_result_bytes = RESULT_CAP;
    let provider = h.provider.clone();
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
        RunOutcome::Completed,
        "the run continues past the oversized result"
    );

    let requests = provider.requests();
    let after = requests
        .last()
        .ok_or("no model request after the tool result")?;
    let texts = text_of(after);
    let tokens: u64 = texts
        .iter()
        .map(|text| u64::from(estimate(text).tokens) + 4)
        .sum();
    assert!(
        tokens <= u64::from(WINDOW - RESERVE),
        "the request after the result fits the window: {tokens} tokens"
    );
    let result = texts
        .iter()
        .find(|text| text.contains("result line of the oversized output"))
        .ok_or("the cut result is still in the request")?;
    assert!(
        result.contains("truncated"),
        "the cut is marked: {}",
        &result[result.len().saturating_sub(300)..]
    );
    assert!(result.len() < big.len());
    Ok(())
}

/// Distinct numbered lines, so neither dedupe nor compression can shrink the output by accident.
fn oversized(tag: &str) -> String {
    use std::fmt::Write as _;
    (0..3000).fold(String::new(), |mut text, line| {
        let _ = writeln!(text, "{tag} {line:05} result line of the oversized output");
        text
    })
}

fn request_tokens(request: &tqa_llm::CompletionRequest) -> u64 {
    text_of(request)
        .iter()
        .map(|text| u64::from(estimate(text).tokens) + 4)
        .sum()
}

#[tokio::test]
async fn ctx_e_004_two_parallel_oversized_results_share_what_is_free_and_the_next_request_fits()
-> TestResult {
    let (first, second) = (oversized("A"), oversized("B"));
    let a = fake(
        "read_file",
        true,
        vec![output(&first, 0), output(&second, 0)],
    )?;
    let mut h = Harness::new(
        vec![
            calls(&[
                ("a", "read_file", r#"{"path":"a"}"#),
                ("b", "read_file", r#"{"path":"b"}"#),
            ]),
            answer(),
            answer(),
        ],
        registry(&[a])?,
    )?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.tool_result_bytes = RESULT_CAP;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    let results = tool_results(&sink.events());
    assert_eq!(results.len(), 2);
    let (earlier, later) = (&results[0].2, &results[1].2);
    assert!(
        earlier.contains("truncated") && later.contains("truncated"),
        "both are cut with a marker"
    );
    assert!(
        later.len() < earlier.len(),
        "the later result gets a share of what the earlier one left"
    );
    let after = provider.requests();
    let tokens = request_tokens(after.last().ok_or("no request after the results")?);
    assert!(
        tokens <= u64::from(WINDOW - RESERVE),
        "the request after both results fits: {tokens} tokens"
    );
    Ok(())
}

#[tokio::test]
async fn ctx_e_004_a_nearly_full_window_still_keeps_the_floor_with_head_tail_and_spill_path()
-> TestResult {
    let big = oversized("F");
    let tool = fake("read_file", true, vec![output(&big, 0)])?;
    let mut h = Harness::new(
        vec![
            calls(&[("floor", "read_file", r#"{"path":"floor"}"#)]),
            answer(),
            answer(),
        ],
        registry(&[tool])?,
    )?;
    // About 690 input tokens, most of them taken, so the free share is below the floor.
    h.deps.route.context_window = Some(1200);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.tool_result_bytes = RESULT_CAP;
    let sink = h.sink.clone();
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    let results = tool_results(&sink.events());
    let content = &results.first().ok_or("no tool result")?.2;
    assert_eq!(content.len(), MIN_RESULT_BYTES, "the floor, exactly");
    assert!(
        content.starts_with("F 00000 result line")
            && content.ends_with("F 02999 result line of the oversized output\n")
    );
    assert!(
        content.contains(".tqa/conversation/spill/floor.txt"),
        "the spill path is stated"
    );
    Ok(())
}

/// A response that streams one complete call and is then cut, so the step is retried.
fn cut_after_a_call() -> tqa_testkit::ProviderReply {
    use tqa_testkit::ProviderChunk;
    let chunk = |result| ProviderChunk {
        delay: std::time::Duration::ZERO,
        result,
    };
    tqa_testkit::ProviderReply {
        delay: std::time::Duration::ZERO,
        result: Ok(vec![
            chunk(Ok(tqa_llm::StreamChunk::ToolCallDelta {
                index: 0,
                id: Some("stale".into()),
                name: Some("read_file".into()),
                arguments_delta: r#"{"path":"stale"}"#.into(),
            })),
            chunk(Err(tqa_llm::LlmError::StreamCut {
                partial: Box::default(),
            })),
        ]),
    }
}

#[tokio::test]
async fn ctx_e_004_after_a_retried_attempt_the_cap_follows_the_selected_attempt() -> TestResult {
    let big = oversized("R");
    let tool = fake("read_file", true, vec![output(&big, 0)])?;
    let mut h = Harness::new(
        vec![
            cut_after_a_call(),
            calls(&[("fresh", "read_file", r#"{"path":"fresh"}"#)]),
            answer(),
            answer(),
        ],
        registry(&[tool])?,
    )?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.tool_result_bytes = RESULT_CAP;
    let (sink, clock) = (h.sink.clone(), h.clock.clone());
    let runner = Runner::new(h.deps)?;
    let run = tokio::spawn(async move {
        runner
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await
    });
    // The retry waits out its backoff on the fake clock, while the spill write takes real time.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !run.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "the retried run did not finish"
        );
        clock.advance(std::time::Duration::from_millis(50))?;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    let outcome = run.await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let results = tool_results(&sink.events());
    assert_eq!(results.len(), 1, "only the selected attempt's call ran");
    assert!(results[0].2.contains("truncated") && results[0].2.len() < RESULT_CAP as usize);
    Ok(())
}
