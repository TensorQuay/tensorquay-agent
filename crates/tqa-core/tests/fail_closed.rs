//! CORE-U-038 edges: a run parked for an approval before ADR-0035 is failed rather than resumed, and every core error
//! keeps a stable code for the supervisor.
mod support;
use async_trait::async_trait;
use std::sync::Arc;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{CoreError, EventContext, EventSink, Runner, SinkError};
use tqa_types::{ErrorCode, Event, Seq};

/// ADR-0035: a history whose last run event is the retired `run.parked` marker (a run parked for an approval before the
/// removal) is failed with `approvals_removed`, never resumed: its waiting call is closed, not run.
#[tokio::test]
async fn core_u_038_a_history_ending_parked_fails_and_never_runs_the_waiting_call() -> TestResult {
    let tool = fake("execute", false, vec![output("must not run", 0)])?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    let history: Vec<tqa_types::ServerFrame> = [
        r#"{"type":"event","seq":1,"run_id":null,"step":null,"ts":"2026-09-28T10:00:00Z","event":{"type":"message.user","text":"go","attachments":[]}}"#,
        r#"{"type":"event","seq":2,"run_id":"r-1","step":null,"ts":"2026-09-28T10:00:00Z","event":{"type":"run.started","run_id":"r-1"}}"#,
        r#"{"type":"event","seq":3,"run_id":"r-1","step":1,"ts":"2026-09-28T10:00:00Z","event":{"type":"step.started","step":1,"attempt":1}}"#,
        r#"{"type":"event","seq":4,"run_id":"r-1","step":1,"ts":"2026-09-28T10:00:00Z","event":{"type":"tool.call","call_id":"a","name":"execute","arguments":{"path":"a"}}}"#,
        r#"{"type":"event","seq":5,"run_id":"r-1","step":1,"ts":"2026-09-28T10:00:00Z","event":{"type":"model.completed","step":1,"attempt":1,"finish_reason":"tool_calls"}}"#,
        r#"{"type":"permission.request","seq":6,"request_id":"q","tool":"execute","summary":"run","risk":"low","expires_at":"2026-09-28T10:05:00Z"}"#,
        r#"{"type":"event","seq":7,"run_id":"r-1","step":1,"ts":"2026-09-28T10:00:00Z","event":{"type":"run.parked","run_id":"r-1","request_id":"q"}}"#,
        r#"{"type":"event","seq":8,"run_id":null,"step":null,"ts":"2026-09-28T10:00:00Z","event":{"type":"sandbox.state","state":"parked"}}"#,
    ]
    .iter()
    .map(|line| tqa_types::ServerFrame::parse(line.as_bytes()))
    .collect::<Result<_, _>>()?;
    h.sink_with(MemorySink::new(&history));
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert!(
        matches!(
            outcome,
            tqa_core::RunOutcome::Failed {
                code: ErrorCode::ApprovalsRemoved,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert!(tool.calls().is_empty(), "the waiting call ran");
    let events = sink.events();
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ToolResult { call_id, status: tqa_types::ToolStatus::Cancelled, .. } if call_id.as_str() == "a"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::RunFailed {
            code: ErrorCode::ApprovalsRemoved,
            ..
        }
    )));
    Ok(())
}

/// ADR-0035: a run whose last frame is a retired, unanswered `permission.request` (an approval that was pending when the
/// removal came) is failed the same way: the call it asked about never runs.
#[tokio::test]
async fn core_u_038_a_history_ending_in_an_unanswered_request_fails_too() -> TestResult {
    let tool = fake("execute", false, vec![output("must not run", 0)])?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    let history: Vec<tqa_types::ServerFrame> = [
        r#"{"type":"event","seq":1,"run_id":null,"step":null,"ts":"2026-09-28T10:00:00Z","event":{"type":"message.user","text":"go","attachments":[]}}"#,
        r#"{"type":"event","seq":2,"run_id":"r-1","step":null,"ts":"2026-09-28T10:00:00Z","event":{"type":"run.started","run_id":"r-1"}}"#,
        r#"{"type":"event","seq":3,"run_id":"r-1","step":1,"ts":"2026-09-28T10:00:00Z","event":{"type":"step.started","step":1,"attempt":1}}"#,
        r#"{"type":"event","seq":4,"run_id":"r-1","step":1,"ts":"2026-09-28T10:00:00Z","event":{"type":"tool.call","call_id":"a","name":"execute","arguments":{"path":"a"}}}"#,
        r#"{"type":"event","seq":5,"run_id":"r-1","step":1,"ts":"2026-09-28T10:00:00Z","event":{"type":"model.completed","step":1,"attempt":1,"finish_reason":"tool_calls"}}"#,
        r#"{"type":"permission.request","seq":6,"request_id":"q","tool":"execute","summary":"run","risk":"low","expires_at":"2026-09-28T10:05:00Z"}"#,
    ]
    .iter()
    .map(|line| tqa_types::ServerFrame::parse(line.as_bytes()))
    .collect::<Result<_, _>>()?;
    h.sink_with(MemorySink::new(&history));
    let outcome = Runner::new(h.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert!(
        matches!(
            outcome,
            tqa_core::RunOutcome::Failed {
                code: ErrorCode::ApprovalsRemoved,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert!(tool.calls().is_empty(), "the call it asked about ran");
    Ok(())
}

#[test]
fn core_u_038_every_core_error_keeps_a_stable_supervisor_code() {
    let frame = tqa_types::FrameError {
        code: ErrorCode::InvalidFrame,
        path: "$".into(),
        detail: String::new(),
    };
    let cases = [
        (
            CoreError::Sink(SinkError {
                code: ErrorCode::Io,
            }),
            ErrorCode::Io,
        ),
        (CoreError::Busy, ErrorCode::Busy),
        (
            CoreError::InvalidConfiguration("route"),
            ErrorCode::InvalidFrame,
        ),
        (CoreError::InvalidTurn(frame), ErrorCode::InvalidFrame),
        (CoreError::Internal("bug"), ErrorCode::Internal),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error}");
    }
}

/// A sink that journals every event but misreports its sequence: from the `after`th append on, zero or one too many.
struct Misnumbered {
    inner: Arc<MemorySink>,
    after: usize,
    zero: bool,
    appends: std::sync::atomic::AtomicUsize,
}
#[async_trait]
impl EventSink for Misnumbered {
    async fn emit(&self, event: Event, context: EventContext) -> Result<Seq, SinkError> {
        let seq = self.inner.emit(event, context).await?;
        let count = self
            .appends
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        if count < self.after {
            return Ok(seq);
        }
        Ok(if self.zero {
            Seq::ZERO
        } else {
            seq.next().unwrap_or(seq)
        })
    }
}

#[tokio::test]
async fn core_u_038_a_sink_that_skips_or_zeroes_a_sequence_stops_the_run_with_seq_gap() -> TestResult
{
    for zero in [false, true] {
        let mut h = Harness::new(vec![answer()], tqa_tools::ToolRegistry::new())?;
        let journal = h.sink.clone();
        h.deps.sink = Arc::new(Misnumbered {
            inner: h.sink.clone(),
            // A zero matters on the very first append, where there is no earlier sequence to compare.
            after: if zero { 1 } else { 2 },
            zero,
            appends: std::sync::atomic::AtomicUsize::new(0),
        });
        let result = Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await;
        assert!(
            matches!(
                &result,
                Err(CoreError::Sink(SinkError {
                    code: ErrorCode::SeqGap
                }))
            ),
            "zero={zero}: {result:?}"
        );
        // The first misnumbered append is the last one: no later write follows a bad sequence.
        assert_eq!(
            journal.frames().len(),
            if zero { 1 } else { 2 },
            "zero={zero}"
        );
    }
    Ok(())
}
