//! CTX-U-013 diagnostics: when the model summary is unusable, compaction falls back and the runtime log says why with
//! one bounded reason code, never with any of the summary's text.
mod support;
use std::{
    io,
    sync::{Arc, Mutex},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, LlmError, StreamChunk};
use tqa_testkit::{ProviderChunk, ProviderReply};
use tqa_tools::ToolRegistry;
use tqa_types::{CompactionSource, CompactionSummary, Event};
use tracing::instrument::WithSubscriber;

/// A text that must never reach a log.
const MARKER: &str = "SUMMARY-TEXT-MARKER";
const WINDOW: u32 = 16_384;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Capture {
    fn text(&self) -> String {
        String::from_utf8_lossy(
            &self
                .0
                .lock()
                .map(|buffer| buffer.clone())
                .unwrap_or_default(),
        )
        .into_owned()
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("capture lock"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Enough earlier turns that the run compacts before its first step, so the first request is the summary call.
fn long_history() -> Result<Vec<tqa_types::ServerFrame>, Box<dyn std::error::Error>> {
    let mut events = Vec::new();
    for turn in 1..=60 {
        let run_id: tqa_types::RunId = format!("r-{turn}").parse()?;
        events.extend([
            Event::MessageUser {
                lead_brief: None,
                team_wake: None,
                text: format!("request {turn}"),
                attachments: vec![],
            },
            Event::RunStarted {
                run_id: run_id.clone(),
            },
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
            Event::TextDelta {
                text: format!("{turn:04}{}", "a".repeat(1200)),
            },
            Event::ModelCompleted {
                step: 1,
                attempt: 1,
                finish_reason: tqa_types::FinishReason::Stop,
            },
            Event::StepCompleted {
                step: 1,
                duration_ms: 0,
            },
            Event::RunCompleted { run_id },
        ]);
    }
    history(events)
}
fn text(body: &str) -> ProviderReply {
    reply(vec![
        StreamChunk::TextDelta(body.into()),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ])
}
fn failing(error: LlmError) -> ProviderReply {
    ProviderReply {
        delay: std::time::Duration::ZERO,
        result: Err(error),
    }
}
fn server_error() -> ProviderReply {
    failing(LlmError::Http {
        status: 500,
        body_excerpt: MARKER.into(),
    })
}
fn cut() -> ProviderReply {
    ProviderReply {
        delay: std::time::Duration::ZERO,
        result: Ok(vec![
            ProviderChunk {
                delay: std::time::Duration::ZERO,
                result: Ok(StreamChunk::TextDelta(format!("{{\"goal\": \"{MARKER}"))),
            },
            ProviderChunk {
                delay: std::time::Duration::ZERO,
                result: Err(LlmError::StreamCut {
                    partial: Box::default(),
                }),
            },
        ]),
    }
}

/// Runs one turn whose compaction gets `summaries` in order (one per summary attempt), returning the compaction
/// source, the captured log and the number of summary calls. Fake time advances, so retry backoff passes.
async fn compact_with(
    summaries: Vec<ProviderReply>,
) -> Result<(CompactionSource, String, usize, Vec<Event>), Box<dyn std::error::Error>> {
    let history = long_history()?;
    let mut script = summaries;
    script.push(answer());
    let mut h = Harness::new(script, ToolRegistry::new())?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(1024);
    h.deps.limits.max_output_tokens = 1_000_000;
    h.sink_with(MemorySink::new(&history));
    let (sink, provider, clock) = (h.sink.clone(), h.provider.clone(), h.clock.clone());
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    let runner = Runner::new(h.deps)?;
    let run = tokio::spawn(
        async move {
            runner
                .run(
                    &history,
                    Some(turn()),
                    CancellationToken::new(),
                    CancellationToken::new(),
                )
                .await
        }
        .with_subscriber(subscriber),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !run.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "the run did not finish"
        );
        clock.advance(std::time::Duration::from_millis(500))?;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(run.await??, RunOutcome::Completed);
    let source = sink
        .events()
        .iter()
        .find_map(|event| match event {
            Event::Compaction { source, .. } => Some(*source),
            _ => None,
        })
        .ok_or("the run did not compact")?;
    // A summary call is the one whose system message is the fixed compaction prompt.
    let summary_calls = provider
        .requests()
        .iter()
        .filter(|request| {
            request.messages.first().is_some_and(|message| {
                matches!(message.content.first(), Some(tqa_llm::ContentPart::Text(text)) if text == tqa_prompt::COMPACTION_PROMPT)
            })
        })
        .count();
    Ok((source, capture.text(), summary_calls, sink.events()))
}

fn assert_reason(log: &str, code: &str) {
    let lines: Vec<_> = log
        .lines()
        .filter(|line| line.contains("compaction summary unusable"))
        .collect();
    assert_eq!(lines.len(), 1, "one warning: {log}");
    assert!(
        lines[0].contains("WARN") && lines[0].contains(&format!("reason={code}")),
        "{}",
        lines[0]
    );
    assert!(!log.contains(MARKER), "no summary text in the log: {log}");
}

#[tokio::test]
async fn ctx_u_013_an_unparseable_summary_falls_back_and_logs_parse_without_its_text() -> TestResult
{
    let (source, log, calls, _) =
        compact_with(vec![text(&format!("Here is my summary: {MARKER}"))]).await?;
    assert_eq!(calls, 1, "an unparseable summary is never retried");
    assert_eq!(source, CompactionSource::Fallback);
    assert_reason(&log, "parse");
    Ok(())
}

#[tokio::test]
async fn ctx_u_013_a_summary_over_its_bounds_logs_bounds_and_is_not_retried() -> TestResult {
    let summary = CompactionSummary {
        goal: MARKER.into(),
        done: (0..33).map(|index| format!("{MARKER} {index}")).collect(),
        ..CompactionSummary::default()
    };
    let (source, log, calls, _) =
        compact_with(vec![text(&serde_json::to_string(&summary)?)]).await?;
    assert_eq!((source, calls), (CompactionSource::Fallback, 1));
    assert_reason(&log, "bounds");
    Ok(())
}

#[tokio::test]
async fn ctx_u_013_a_non_retryable_summary_failure_falls_back_at_once_with_its_code() -> TestResult
{
    // One more call than the stream assembler accepts in a response.
    let flood = reply(
        (0..129)
            .map(|index| StreamChunk::ToolCallDelta {
                index,
                id: Some(format!("c{index}")),
                name: Some("read_file".into()),
                arguments_delta: "{}".into(),
            })
            .collect(),
    );
    let too_large = failing(LlmError::Http {
        status: 413,
        body_excerpt: MARKER.into(),
    });
    for (reply, code) in [(flood, "stream_limit"), (too_large, "http 413")] {
        let (source, log, calls, _) = compact_with(vec![reply]).await?;
        assert_eq!((source, calls), (CompactionSource::Fallback, 1), "{code}");
        assert_reason(&log, code);
    }
    Ok(())
}

#[tokio::test]
async fn ctx_u_013_retryable_failures_are_retried_twice_then_fall_back_with_their_code()
-> TestResult {
    let limited = || {
        failing(LlmError::RateLimited {
            retry_after: Some(std::time::Duration::from_secs(3)),
        })
    };
    for (replies, code) in [
        (vec![limited(), limited(), limited()], "rate_limited"),
        (vec![cut(), cut(), cut()], "stream_cut"),
        (
            vec![server_error(), server_error(), server_error()],
            "http 500",
        ),
    ] {
        let (source, log, calls, _) = compact_with(replies).await?;
        assert_eq!(
            calls,
            1 + tqa_core::SUMMARY_RETRY_ATTEMPTS as usize,
            "{code}"
        );
        assert_eq!(source, CompactionSource::Fallback, "{code}");
        assert_reason(&log, code);
    }
    Ok(())
}

#[tokio::test]
async fn ctx_u_013_a_rate_limited_summary_call_succeeds_on_retry() -> TestResult {
    let summary = CompactionSummary {
        goal: "answer the requests".into(),
        ..CompactionSummary::default()
    };
    let limited = failing(LlmError::RateLimited {
        retry_after: Some(std::time::Duration::from_secs(3)),
    });
    let (source, log, calls, _) =
        compact_with(vec![limited, text(&serde_json::to_string(&summary)?)]).await?;
    assert_eq!((source, calls), (CompactionSource::Model, 2));
    assert!(!log.contains("compaction summary unusable"), "{log}");
    Ok(())
}

#[tokio::test]
async fn ctx_u_013_a_valid_summary_logs_no_warning() -> TestResult {
    let summary = CompactionSummary {
        goal: "answer the requests".into(),
        ..CompactionSummary::default()
    };
    let (source, log, calls, _) =
        compact_with(vec![text(&serde_json::to_string(&summary)?)]).await?;
    assert_eq!((source, calls), (CompactionSource::Model, 1));
    assert!(!log.contains("compaction summary unusable"), "{log}");
    Ok(())
}

fn throttled(seconds: u64) -> ProviderReply {
    failing(LlmError::Throttled {
        retry_after: std::time::Duration::from_secs(seconds),
        scope: tqa_llm::ThrottleScope::User,
    })
}
/// The (delay in ms, reason) of every provider retry event, and whether each came before the compaction.
fn retries_before_compaction(events: &[Event]) -> Vec<(u64, String)> {
    let compaction = events
        .iter()
        .position(|event| matches!(event, Event::Compaction { .. }))
        .unwrap_or(events.len());
    events[..compaction]
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry {
                delay_ms, reason, ..
            } => Some((*delay_ms, reason.clone())),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn ctx_u_013_a_throttled_summary_call_waits_without_spending_attempts_and_reports_once()
-> TestResult {
    let summary = CompactionSummary {
        goal: "answer the requests".into(),
        ..CompactionSummary::default()
    };
    let mut replies: Vec<_> = (0..5).map(|_| throttled(2)).collect();
    replies.push(text(&serde_json::to_string(&summary)?));
    let (source, log, calls, events) = compact_with(replies).await?;
    assert_eq!(
        (source, calls),
        (CompactionSource::Model, 6),
        "five throttles spend none of the two retries"
    );
    assert!(!log.contains("compaction summary unusable"), "{log}");
    assert_eq!(
        retries_before_compaction(&events),
        [(2000, "rate_limited".to_owned())],
        "one coalesced event makes the wait visible"
    );
    Ok(())
}

#[tokio::test]
async fn ctx_u_013_a_changed_throttle_wait_is_reported_again_and_capped() -> TestResult {
    let summary = CompactionSummary {
        goal: "answer the requests".into(),
        ..CompactionSummary::default()
    };
    let replies = vec![
        throttled(2),
        throttled(2),
        throttled(5),
        throttled(45),
        text(&serde_json::to_string(&summary)?),
    ];
    let (source, _, calls, events) = compact_with(replies).await?;
    assert_eq!((source, calls), (CompactionSource::Model, 5));
    assert_eq!(
        retries_before_compaction(&events),
        [
            (2000, "rate_limited".to_owned()),
            (5000, "rate_limited".to_owned()),
            (30_000, "rate_limited".to_owned()),
        ]
    );
    Ok(())
}
