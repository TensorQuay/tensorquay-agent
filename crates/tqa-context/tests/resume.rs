//! CORE-U-038: next step, tool recovery, retried model and idle dangling calls.
#[path = "support/history.rs"]
mod support;
use std::error::Error;
use support::{call, frames, model, result, start, step, text};
use tqa_context::{ResumePoint, project, resume_point};
use tqa_types::{Event, ToolStatus};

#[test]
fn core_u_038_resume_point_table() -> Result<(), Box<dyn Error>> {
    let run: tqa_types::RunId = "run".parse()?;
    let cases = vec![
        (vec![], ResumePoint::Idle { dangling: vec![] }),
        (
            vec![start("run")?],
            ResumePoint::NextStep {
                run_id: run.clone(),
                step: 1,
            },
        ),
        (
            vec![
                start("run")?,
                step(1),
                model(1),
                Event::StepCompleted {
                    step: 1,
                    duration_ms: 0,
                },
            ],
            ResumePoint::NextStep {
                run_id: run.clone(),
                step: 2,
            },
        ),
        (
            vec![start("run")?, step(2), text("partial")],
            ResumePoint::RetryModel {
                run_id: run.clone(),
                step: 1,
                attempt: 3,
            },
        ),
        (
            vec![start("run")?, step(1), model(1), step(2), text("partial")],
            ResumePoint::RetryModel {
                run_id: run.clone(),
                step: 1,
                attempt: 3,
            },
        ),
        (
            vec![
                start("run")?,
                step(1),
                call("done")?,
                call("running")?,
                call("waiting")?,
                model(1),
                result("done")?,
                Event::ToolStart {
                    call_id: "running".parse()?,
                },
            ],
            ResumePoint::ToolPhase {
                run_id: run,
                step: 1,
                done: vec!["done".parse()?],
                interrupted: vec!["running".parse()?],
                not_started: vec!["waiting".parse()?],
            },
        ),
    ];
    for (events, expected) in cases {
        assert_eq!(resume_point(&frames(events))?, expected);
    }
    Ok(())
}

#[test]
fn core_u_038_ended_runs_close_dangling_calls_before_projection() -> Result<(), Box<dyn Error>> {
    for terminal in [
        Event::RunCompleted {
            run_id: "run".parse()?,
        },
        Event::RunCancelled {
            run_id: "run".parse()?,
        },
        Event::RunFailed {
            run_id: "run".parse()?,
            reason: "failed".into(),
            code: tqa_types::ErrorCode::ProviderError,
            provider_message: None,
        },
    ] {
        let mut events = vec![
            start("run")?,
            step(1),
            call("a")?,
            call("b")?,
            model(1),
            result("a")?,
            terminal,
        ];
        assert_eq!(
            resume_point(&frames(events.clone()))?,
            ResumePoint::Idle {
                dangling: vec!["b".parse()?]
            }
        );
        assert!(project(&frames(events.clone())).is_err());
        events.push(Event::ToolResult {
            call_id: "b".parse()?,
            status: ToolStatus::Interrupted,
            content: "the sandbox stopped while this tool ran; it may be retried".into(),
            target: None,
            evidence: None,
        });
        assert_eq!(
            resume_point(&frames(events.clone()))?,
            ResumePoint::Idle { dangling: vec![] }
        );
        assert_eq!(project(&frames(events))?.len(), 3);
    }
    Ok(())
}

#[test]
fn core_u_038_counters_never_wrap() -> Result<(), Box<dyn Error>> {
    let histories = [
        vec![start("run")?, step(u32::MAX)],
        vec![
            start("run")?,
            Event::StepStarted {
                step: u32::MAX,
                attempt: 1,
            },
            Event::ModelCompleted {
                step: u32::MAX,
                attempt: 1,
                finish_reason: tqa_types::FinishReason::Stop,
            },
            Event::StepCompleted {
                step: u32::MAX,
                duration_ms: 0,
            },
        ],
    ];
    for events in histories {
        assert!(matches!(
            resume_point(&frames(events)),
            Err(tqa_context::ProjectionError::Exhausted { .. })
        ));
    }
    Ok(())
}
