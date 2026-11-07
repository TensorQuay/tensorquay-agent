//! CORE-U-033 (the projection half, ADR-0013 point 2): a step where the decision engine acted instead of the model
//! reaches the model as one user-role note, stating what the system ran and its result, never as a call the model
//! didn't make. The step's attempt is complete once its call is journaled, so a resume lands in the tool phase.
#[allow(
    dead_code,
    reason = "This suite uses only the frame builder from the shared vocabulary."
)]
#[path = "support/history.rs"]
mod support;
use serde_json::json;
use std::error::Error;
use support::frames;
use tqa_context::{ResumePoint, Role, project, resume_point};
use tqa_types::{DecisionChoice, DecisionPoint, Event, ServerFrame, ToolStatus};

/// A run whose first step is the engine's action: its decision (journaled with the step it decides), the step, the
/// call and, with `result`, its result and the step's end.
fn acted(result: bool) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let mut events = vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "task".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: "r".parse()?,
        },
        Event::Decision {
            point: DecisionPoint::BeforeStep,
            engine: "router".into(),
            choice: DecisionChoice::Act {
                tool: "read_file".into(),
                arguments: json!({"path": "a.rs"}),
            },
            latency_ms: 3,
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
        Event::ToolCall {
            call_id: "de-1-1".parse()?,
            name: "read_file".into(),
            arguments: json!({"path": "a.rs"}),
        },
    ];
    if result {
        events.extend([
            Event::ToolStart {
                call_id: "de-1-1".parse()?,
            },
            Event::ToolResult {
                call_id: "de-1-1".parse()?,
                status: ToolStatus::Ok,
                content: "fn main() {}".into(),
                target: None,
                evidence: None,
            },
            Event::StepCompleted {
                step: 1,
                duration_ms: 0,
            },
        ]);
    }
    let mut frames = frames(events);
    // The loop journals a decision with the step it decides.
    if let Some(ServerFrame::Event { step, .. }) = frames.get_mut(2) {
        *step = Some(1);
    }
    Ok(frames)
}

#[test]
fn core_u_033_an_engine_action_is_a_user_note_not_a_model_call() -> Result<(), Box<dyn Error>> {
    let messages = project(&acted(true)?)?;
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(messages[0].role, Role::User);
    let note = &messages[1];
    assert_eq!(note.role, Role::User);
    assert!(note.tool_calls.is_empty() && note.tool_call_id.is_none());
    for part in [
        "read_file",
        r#"{"path":"a.rs"}"#,
        "fn main() {}",
        "not the model",
    ] {
        assert!(
            note.text.contains(part),
            "missing {part:?} in {:?}",
            note.text
        );
    }
    assert!(
        !messages
            .iter()
            .any(|message| message.role == Role::Assistant || message.role == Role::Tool),
        "no assistant call and no tool message"
    );
    Ok(())
}

#[test]
fn core_u_033_an_action_with_its_call_journaled_resumes_in_the_tool_phase()
-> Result<(), Box<dyn Error>> {
    match resume_point(&acted(false)?)? {
        ResumePoint::ToolPhase {
            step, not_started, ..
        } => {
            assert_eq!(step, 1);
            assert_eq!(not_started, vec!["de-1-1".parse()?]);
        }
        other => return Err(format!("expected the tool phase, got {other:?}").into()),
    }
    Ok(())
}
