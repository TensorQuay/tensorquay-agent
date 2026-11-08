//! CORE-E-041 (decided on 7 Oct 2026): the repeated-failure guard lets a model that is still
//! changing files try again. A call's count starts again after a successful call of a tool that changes the workspace
//! (`shell`, `write_file`, `edit_file`); three failures with no change between still stop the run, and six failures of
//! the same call stop it whatever happens between. CORE-E-012's own cases (`failures.rs`) are unchanged.
mod support;
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_testkit::{FakeTool, FakeToolReply, ProviderReply};
use tqa_tools::{ToolError, ToolOutput};
use tqa_types::{ErrorCode, Event, ToolStatus};

const TEST: &str = r#"{"path":"test/slug.test.ts"}"#;
const THREE: &str = "the same tool call failed three times with no change between";
const SIX: &str = "the same tool call failed six times in this run";

fn failed() -> FakeToolReply {
    FakeToolReply {
        delay: std::time::Duration::ZERO,
        result: Err(ToolError::NotFound),
    }
}
fn ok() -> FakeToolReply {
    FakeToolReply {
        delay: std::time::Duration::ZERO,
        result: Ok(ToolOutput::text("done")),
    }
}

/// A run whose model makes `steps` calls, one a step, then answers; each fake answers in its own order.
async fn run(
    steps: &[(&str, &str)],
    tools: &[Arc<FakeTool>],
) -> Result<RunOutcome, Box<dyn Error>> {
    let script: Vec<ProviderReply> = steps
        .iter()
        .enumerate()
        .map(|(index, (name, arguments))| calls(&[(&format!("c{index}"), name, arguments)]))
        .chain(std::iter::once(answer()))
        .collect();
    let h = Harness::new(script, registry(tools)?)?;
    Ok(Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}

fn stopped(outcome: &RunOutcome, reason: &str) -> bool {
    matches!(outcome, RunOutcome::Failed { code: ErrorCode::RepeatedFailure, reason: said } if said == reason)
}

fn write() -> (&'static str, &'static str) {
    ("write_file", r#"{"path":"test/slug.test.ts"}"#)
}

// TEST: CORE-E-041
/// A failing call, a successful write, then the same call failing: the count starts again at the write, so the run
/// stops at the third failure after it, not the second.
#[tokio::test]
async fn core_e_041_a_successful_change_starts_the_count_again() -> TestResult {
    let shell = fake("shell", false, vec![failed(), failed(), failed(), failed()])?;
    let writer = fake("write_file", false, vec![ok()])?;
    let outcome = run(
        &[
            ("shell", TEST),
            write(),
            ("shell", TEST),
            ("shell", TEST),
            ("shell", TEST),
        ],
        &[shell.clone(), writer],
    )
    .await?;
    assert!(stopped(&outcome, THREE), "{outcome:?}");
    assert_eq!(
        shell.calls().len(),
        4,
        "one before the write, three after it"
    );
    Ok(())
}

// TEST: CORE-E-041
/// A read or a fetch between failures changes nothing: the third failure stops the run, as before.
#[tokio::test]
async fn core_e_041_a_read_or_a_fetch_between_failures_starts_nothing_again() -> TestResult {
    let shell = fake("shell", false, vec![failed(), failed(), failed()])?;
    let reader = fake("read_file", true, vec![ok()])?;
    let fetch = fake("fetch_url", false, vec![ok()])?;
    let outcome = run(
        &[
            ("shell", TEST),
            ("read_file", TEST),
            ("shell", TEST),
            ("fetch_url", r#"{"path":"https://docs.example/"}"#),
            ("shell", TEST),
        ],
        &[shell.clone(), reader, fetch],
    )
    .await?;
    assert!(stopped(&outcome, THREE), "{outcome:?}");
    assert_eq!(shell.calls().len(), 3);
    Ok(())
}

// TEST: CORE-E-041
/// The ceiling: a successful write before every failure still stops the run at the call's sixth failure.
#[tokio::test]
async fn core_e_041_six_failures_of_one_call_stop_the_run_whatever_changed() -> TestResult {
    let shell = fake("shell", false, (0..7).map(|_| failed()).collect())?;
    let writer = fake("write_file", false, (0..7).map(|_| ok()).collect())?;
    let steps: Vec<(&str, &str)> = (0..7).flat_map(|_| [write(), ("shell", TEST)]).collect();
    let outcome = run(&steps, &[shell.clone(), writer]).await?;
    assert!(stopped(&outcome, SIX), "{outcome:?}");
    assert_eq!(shell.calls().len(), 6);
    Ok(())
}

// TEST: CORE-E-041
/// A write that failed changed nothing: it starts no count again.
#[tokio::test]
async fn core_e_041_a_failed_change_starts_nothing_again() -> TestResult {
    let shell = fake("shell", false, vec![failed(), failed(), failed()])?;
    let writer = fake("write_file", false, vec![failed()])?;
    let outcome = run(
        &[("shell", TEST), write(), ("shell", TEST), ("shell", TEST)],
        &[shell.clone(), writer],
    )
    .await?;
    assert!(stopped(&outcome, THREE), "{outcome:?}");
    assert_eq!(shell.calls().len(), 3);
    Ok(())
}

// TEST: CORE-E-041
/// A run resumed mid-sequence counts from its journal: two failures since the last write, then one more stops it.
/// Counting the run's three failures regardless of the write, it would have stopped before calling again.
#[tokio::test]
async fn core_e_041_a_resume_rebuilds_the_counts_from_the_journal() -> TestResult {
    let run_id: tqa_types::RunId = "r-1".parse()?;
    let mut events = vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "make the tests pass".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: run_id.clone(),
        },
    ];
    let steps = [
        ("shell", TEST, ToolStatus::Error),
        ("write_file", TEST, ToolStatus::Ok),
        ("shell", TEST, ToolStatus::Error),
        ("shell", TEST, ToolStatus::Error),
    ];
    for (index, (name, arguments, status)) in steps.into_iter().enumerate() {
        let step = u32::try_from(index + 1)?;
        let call_id: tqa_types::ToolCallId = format!("h{step}").parse()?;
        events.extend([
            Event::StepStarted { step, attempt: 1 },
            Event::ToolCall {
                call_id: call_id.clone(),
                name: name.into(),
                arguments: serde_json::from_str(arguments)?,
            },
            Event::ModelCompleted {
                step,
                attempt: 1,
                finish_reason: tqa_types::FinishReason::ToolCalls,
            },
            Event::ToolResult {
                call_id,
                status,
                content: format!("result {step}"),
                target: None,
                evidence: None,
            },
            Event::StepCompleted {
                step,
                duration_ms: 0,
            },
        ]);
    }
    let history = history(events)?;
    let shell = fake("shell", false, vec![failed(), failed()])?;
    let writer = fake("write_file", false, vec![])?;
    let mut h = Harness::new(
        vec![calls(&[("c9", "shell", TEST)]), answer()],
        registry(&[shell.clone(), writer])?,
    )?;
    h.sink_with(MemorySink::new(&history));
    let outcome = Runner::new(h.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert!(stopped(&outcome, THREE), "{outcome:?}");
    assert_eq!(
        shell.calls().len(),
        1,
        "the one failure the journal left room for"
    );
    Ok(())
}

// TEST: CORE-E-041
/// After a reset, a batch of identical parallel calls still never launches past three failures since the change.
#[tokio::test]
async fn core_e_041_a_parallel_batch_after_a_change_never_launches_past_the_limit() -> TestResult {
    let reader = fake("read_file", true, (0..5).map(|_| failed()).collect())?;
    let writer = fake("write_file", false, vec![ok()])?;
    let same = r#"{"path":"missing"}"#;
    let script = vec![
        calls(&[("a", "read_file", same)]),
        calls(&[("b", "write_file", TEST)]),
        calls(&[
            ("c", "read_file", same),
            ("d", "read_file", same),
            ("e", "read_file", same),
            ("f", "read_file", same),
        ]),
        answer(),
    ];
    let h = Harness::new(script, registry(&[reader.clone(), writer])?)?;
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert!(stopped(&outcome, THREE), "{outcome:?}");
    assert_eq!(
        reader.calls().len(),
        4,
        "one before the write, three of the batch after it"
    );
    Ok(())
}

// TEST: CORE-E-041
/// The shape of a miss in an agent wave before 17.0: the test run fails, the model edits, three rounds, then the tests
/// pass. It used to stop at the third failure; the edits between them let it finish.
#[tokio::test]
async fn core_e_041_fix_and_rerun_goes_on_while_the_model_edits() -> TestResult {
    let mut runs = vec![failed(), failed(), failed(), failed()];
    runs.push(ok());
    let shell = fake("shell", false, runs)?;
    let editor = fake("edit_file", false, (0..4).map(|_| ok()).collect())?;
    let edit = ("edit_file", TEST);
    let outcome = run(
        &[
            ("shell", TEST),
            edit,
            ("shell", TEST),
            edit,
            ("shell", TEST),
            edit,
            ("shell", TEST),
            edit,
            ("shell", TEST),
        ],
        &[shell.clone(), editor],
    )
    .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(shell.calls().len(), 5);
    Ok(())
}

// TEST: CORE-E-041
/// A shape a first-hour check before 17.0 found: the same edit fails three times with nothing changed between:
/// it stops at three.
#[tokio::test]
async fn core_e_041_the_same_failing_edit_three_times_still_stops() -> TestResult {
    let editor = fake("edit_file", false, vec![failed(), failed(), failed()])?;
    let wrong = r#"{"path":"src/slug.ts"}"#;
    let outcome = run(
        &[
            ("edit_file", wrong),
            ("edit_file", wrong),
            ("edit_file", wrong),
        ],
        std::slice::from_ref(&editor),
    )
    .await?;
    assert!(stopped(&outcome, THREE), "{outcome:?}");
    assert_eq!(editor.calls().len(), 3);
    Ok(())
}
