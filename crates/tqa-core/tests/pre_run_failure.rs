//! RT-I-015: pre-run failures journal a turn without executing a model step.
mod support;
use std::sync::Arc;
use support::{MemorySink, TestResult, assert_closed, turn};
use tokio_util::sync::CancellationToken;
use tqa_core::{CoreError, PreRunFailure, RunOutcome, fail_before_run};
use tqa_types::{ErrorCode, Event, ServerFrame};

fn failure(history: &[ServerFrame]) -> PreRunFailure<'_> {
    PreRunFailure {
        history,
        message: None,
        code: ErrorCode::EgressDenied,
        reason: "Clone failed: network access was denied",
    }
}

#[tokio::test]
async fn rt_i_015_pre_run_failure_records_the_message_and_one_failed_run() -> TestResult {
    let sink = MemorySink::new(&[]);
    let mut input = failure(&[]);
    input.message = Some(turn());
    let result = fail_before_run(&sink, input, &CancellationToken::new()).await?;
    assert!(matches!(
        result,
        RunOutcome::Failed {
            code: ErrorCode::EgressDenied,
            ..
        }
    ));
    assert_eq!(sink.types(), ["message.user", "run.started", "run.failed"]);
    assert!(
        matches!(&sink.events()[0], Event::MessageUser { lead_brief: _, team_wake: _, text, attachments }
        if *text == turn().text && *attachments == turn().attachments)
    );
    assert!(matches!(&sink.frames()[2], ServerFrame::Event {
        run_id: Some(run_id), step: None, event: Event::RunFailed { code: ErrorCode::EgressDenied, .. }, ..
    } if run_id.as_str() == "r-1"));
    assert_closed(&sink.frames())?;
    Ok(())
}

#[tokio::test]
async fn rt_i_015_pre_run_failure_resumes_each_interrupted_append_without_duplicates() -> TestResult
{
    for cut in 1..=2 {
        let shutdown = CancellationToken::new();
        let mut sink = MemorySink::new(&[]);
        sink.cut = Some((cut, shutdown.clone()));
        let mut input = failure(&[]);
        input.message = Some(turn());
        assert_eq!(
            fail_before_run(&sink, input, &shutdown).await?,
            RunOutcome::Interrupted
        );
        let history = sink.frames();
        assert_eq!(history.len(), cut);
        let resumed = MemorySink::new(&history);
        fail_before_run(&resumed, failure(&history), &CancellationToken::new()).await?;
        assert_eq!(
            resumed.types(),
            ["message.user", "run.started", "run.failed"]
        );
        assert_closed(&resumed.frames())?;
        assert_eq!(&resumed.frames()[..history.len()], history);
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_015_pre_run_failure_stops_at_sink_failure_and_shutdown() -> TestResult {
    for at in 1..=3 {
        let mut sink = MemorySink::new(&[]);
        sink.fail_at = Some(at);
        let mut input = failure(&[]);
        input.message = Some(turn());
        let result = fail_before_run(&sink, input, &CancellationToken::new()).await;
        assert!(matches!(result, Err(CoreError::Sink(_))));
        assert_eq!(sink.frames().len(), at - 1);
    }
    let sink = MemorySink::new(&[]);
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    let mut input = failure(&[]);
    input.message = Some(turn());
    assert_eq!(
        fail_before_run(&sink, input, &shutdown).await?,
        RunOutcome::Interrupted
    );
    assert!(sink.frames().is_empty());
    Ok(())
}

#[tokio::test]
async fn rt_i_015_pre_run_failure_leaves_idle_history_unchanged_and_validates_input() -> TestResult
{
    let sink = MemorySink::new(&[]);
    assert_eq!(
        fail_before_run(&sink, failure(&[]), &CancellationToken::new()).await?,
        RunOutcome::Idle
    );
    let mut input = failure(&[]);
    input.message = Some(tqa_core::UserTurn {
        lead_brief: None,
        team_wake: None,
        text: String::new(),
        attachments: vec![],
    });
    assert!(matches!(
        fail_before_run(&sink, input, &CancellationToken::new()).await,
        Err(CoreError::InvalidTurn(_))
    ));
    let mut input = failure(&[]);
    input.reason = "bad\nstatus";
    assert!(matches!(
        fail_before_run(&sink, input, &CancellationToken::new()).await,
        Err(CoreError::InvalidConfiguration(_))
    ));
    assert!(sink.frames().is_empty());
    Ok(())
}

#[tokio::test]
async fn rt_i_015_pre_run_failure_refuses_to_replace_an_active_model_step() -> TestResult {
    let history = support::history(support::initial()?)?;
    let sink = Arc::new(MemorySink::new(&history));
    assert!(matches!(
        fail_before_run(sink.as_ref(), failure(&history), &CancellationToken::new()).await,
        Err(CoreError::Busy)
    ));
    assert_eq!(sink.frames(), history);
    Ok(())
}
