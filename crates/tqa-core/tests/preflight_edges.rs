//! RT-I-015 edges: a pre-run failure never lands a new turn on top of an active run, and a sink that misnumbers its
//! first append stops it with `seq_gap` at that append.
mod support;
use async_trait::async_trait;
use support::{MemorySink, TestResult, history, turn};
use tokio_util::sync::CancellationToken;
use tqa_core::{CoreError, EventContext, EventSink, PreRunFailure, fail_before_run};
use tqa_types::{ErrorCode, Event, Seq, ServerFrame};

fn failure(history: &[ServerFrame]) -> PreRunFailure<'_> {
    PreRunFailure {
        history,
        message: Some(turn()),
        code: ErrorCode::EgressDenied,
        reason: "Clone failed",
    }
}

#[tokio::test]
async fn rt_i_015_a_new_turn_is_refused_while_a_run_is_active() -> TestResult {
    let active = history(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "earlier".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: "r-1".parse()?,
        },
    ])?;
    let sink = MemorySink::new(&active);
    let result = fail_before_run(&sink, failure(&active), &CancellationToken::new()).await;
    assert!(matches!(result, Err(CoreError::Busy)), "{result:?}");
    assert_eq!(sink.frames(), active, "nothing is written");
    Ok(())
}

/// A sink that journals nothing, reports sequence zero and counts its appends.
#[derive(Default)]
struct Zero(std::sync::atomic::AtomicUsize);
#[async_trait]
impl EventSink for Zero {
    async fn emit(
        &self,
        _event: Event,
        _context: EventContext,
    ) -> Result<Seq, tqa_core::SinkError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Seq::ZERO)
    }
}

#[tokio::test]
async fn rt_i_015_a_sink_that_reports_sequence_zero_stops_the_failure_with_seq_gap() -> TestResult {
    let sink = Zero::default();
    let result = fail_before_run(&sink, failure(&[]), &CancellationToken::new()).await;
    assert!(
        matches!(
            result,
            Err(CoreError::Sink(tqa_core::SinkError {
                code: ErrorCode::SeqGap
            }))
        ),
        "{result:?}"
    );
    assert_eq!(
        sink.0.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "no write follows the bad sequence"
    );
    Ok(())
}
