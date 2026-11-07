//! CTX-U-010 and CORE-U-038 edges: the projection refuses inconsistent histories with a stable reason at the offending
//! sequence, so a corrupted or reordered journal can never reach the model.
#[path = "support/history.rs"]
mod support;
use support::*;
use tqa_context::{ProjectionError, project, resume_point};
use tqa_types::{Event, FinishReason, RunId, Seq};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn reason<T>(result: Result<T, ProjectionError>) -> Option<(Seq, &'static str)> {
    result.err().and_then(|error| match error {
        ProjectionError::Invalid { seq, reason } => Some((seq, reason)),
        ProjectionError::Exhausted { .. } => None,
    })
}
fn user(text: &str) -> Event {
    Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: text.into(),
        attachments: vec![],
    }
}

/// Each history, the sequence it breaks at, and the reason.
type Case = (Vec<Event>, u64, &'static str);
fn cases() -> Result<Vec<Case>, tqa_types::InvalidId> {
    let completed = |step| Event::ModelCompleted {
        step,
        attempt: 1,
        finish_reason: FinishReason::ToolCalls,
    };
    let start_call = |id: &str| -> Result<Event, tqa_types::InvalidId> {
        Ok(Event::ToolStart {
            call_id: id.parse()?,
        })
    };
    Ok(vec![
        (
            vec![
                start("r")?,
                Event::StepStarted {
                    step: 1,
                    attempt: 0,
                },
            ],
            2,
            "invalid step context",
        ),
        (
            vec![start("r")?, step(1), completed(2)],
            3,
            "model completion step mismatch",
        ),
        (
            vec![
                start("r")?,
                step(1),
                Event::StepCompleted {
                    step: 2,
                    duration_ms: 0,
                },
            ],
            3,
            "step completion mismatch",
        ),
        (
            vec![start("r")?, step(1), call("c1")?, start_call("c1")?],
            4,
            "tool started before model completion",
        ),
        (
            vec![
                start("r")?,
                step(1),
                call("c1")?,
                model(1),
                start_call("c1")?,
                start_call("c1")?,
            ],
            6,
            "tool already started or finished",
        ),
        (
            vec![
                start("r")?,
                step(1),
                call("c1")?,
                model(1),
                result("c1")?,
                start_call("c1")?,
            ],
            6,
            "tool already started or finished",
        ),
        (
            vec![start("r")?, text("outside")],
            2,
            "event outside a step",
        ),
    ])
}

#[test]
fn ctx_u_010_each_inconsistent_step_or_call_is_refused_with_its_reason() -> TestResult {
    for (events, seq, expected) in cases()? {
        assert_eq!(
            reason(project(&frames(events))),
            Some((Seq::from(seq), expected)),
            "{expected}"
        );
    }
    Ok(())
}

#[test]
fn core_u_038_a_new_message_cannot_start_a_run_before_an_ended_runs_calls_are_closed() -> TestResult
{
    let run: RunId = "r".parse()?;
    let history = frames(vec![
        user("first"),
        start("r")?,
        step(1),
        call("c1")?,
        model(1),
        Event::RunFailed {
            run_id: run,
            reason: "stopped".into(),
            code: tqa_types::ErrorCode::Internal,
            provider_message: None,
        },
        user("second"),
    ]);
    assert_eq!(
        reason(resume_point(&history)),
        Some((
            Seq::from(7),
            "user message precedes unfinished call recovery"
        ))
    );
    // Without the second message the same history resumes idle, listing the call to close.
    let idle = resume_point(&history[..6])?;
    assert!(matches!(idle, tqa_context::ResumePoint::Idle { dangling } if dangling.len() == 1));
    Ok(())
}
