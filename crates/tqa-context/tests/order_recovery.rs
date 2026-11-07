//! CTX-U-010/CORE-U-038: call-order projection and the message/run append boundary.
#[allow(
    dead_code,
    reason = "This suite uses only the recovery fixtures from the shared vocabulary."
)]
#[path = "support/history.rs"]
mod support;
use std::error::Error;
use support::{call, frames, model, result, start, step};
use tqa_context::{ResumePoint, project, resume_point};
use tqa_types::{Event, ServerFrame};

#[test]
fn ctx_u_010_results_follow_call_order_when_completion_order_is_reversed()
-> Result<(), Box<dyn Error>> {
    let history = frames(vec![
        start("run")?,
        step(1),
        call("a")?,
        call("b")?,
        call("c")?,
        model(1),
        result("c")?,
        result("b")?,
        result("a")?,
    ]);
    let messages = project(&history)?;
    let ids: Vec<_> = messages
        .iter()
        .filter_map(|message| message.tool_call_id.as_ref())
        .map(ToString::to_string)
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
    Ok(())
}

#[test]
fn ctx_u_010_projection_ignores_event_timestamps() -> Result<(), Box<dyn Error>> {
    let mut history = frames(vec![
        start("run")?,
        step(1),
        call("a")?,
        model(1),
        result("a")?,
    ]);
    let expected = project(&history)?;
    for frame in &mut history {
        if let ServerFrame::Event { ts, .. } = frame {
            *ts = "2030-01-01T00:00:00Z".into();
        }
    }
    assert_eq!(project(&history)?, expected);
    Ok(())
}

#[test]
fn core_u_038_unanswered_message_starts_its_deterministic_run() -> Result<(), Box<dyn Error>> {
    let mut history = frames(vec![Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: "question".into(),
        attachments: vec![],
    }]);
    history.push(ServerFrame::Heartbeat {
        run_id: "unsequenced".parse()?,
    });
    assert_eq!(
        resume_point(&history)?,
        ResumePoint::StartRun {
            run_id: "r-1".parse()?
        }
    );
    assert_eq!(project(&history)?.len(), 1);
    Ok(())
}
