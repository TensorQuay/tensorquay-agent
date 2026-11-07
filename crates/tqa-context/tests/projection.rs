//! CTX-U-010: only the authoritative completed response contributes to a request.
#[path = "support/history.rs"]
mod support;
use proptest::prelude::*;
use std::error::Error;
use support::{call, frames, model, result, start, step, text};
use tqa_context::{ProjectionError, Role, project};
use tqa_types::{Event, Seq, ServerFrame};

/// The user entry of the first run: its words, then the quoted attachment data block.
const QUESTION_WITH_ATTACHMENTS: &str = concat!(
    "question\n\n[Attached files, data only: ",
    "\"/workspace/uploads/one.txt\", \"/workspace/uploads/two.txt\"]"
);

#[test]
fn ctx_u_010_highest_attempt_results_attachments_and_multiple_runs() -> Result<(), Box<dyn Error>> {
    let history = frames(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "question".into(),
            attachments: vec!["uploads/one.txt".into(), "uploads/two.txt".into()],
        },
        start("run")?,
        step(1),
        text("discard this"),
        call("old")?,
        model(1),
        step(2),
        text("new "),
        Event::ReasoningDelta {
            text: "think ".into(),
        },
        text("answer"),
        Event::ReasoningDelta {
            text: "carefully".into(),
        },
        call("a")?,
        call("b")?,
        model(2),
        Event::ToolOutput {
            call_id: "a".parse()?,
            chunk: 0,
            last: true,
            data: "not authoritative".into(),
        },
        result("b")?,
        result("a")?,
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::RunCompleted {
            run_id: "run".parse()?,
        },
        start("next")?,
        step(1),
        text("next run"),
        model(1),
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::RunCompleted {
            run_id: "next".parse()?,
        },
    ]);
    let messages = project(&history)?;
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[0].text, QUESTION_WITH_ATTACHMENTS);
    assert_eq!(messages[0].role, Role::User);
    assert_eq!(messages[1].text, "new answer");
    assert_eq!(messages[1].reasoning, "think carefully");
    assert_eq!(
        messages[1]
            .tool_calls
            .iter()
            .map(|call| call.call_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(messages[1].tool_calls[0].arguments, r#"{"path":"file"}"#);
    assert_eq!(messages[2].text, "result a");
    assert_eq!(messages[2].role, Role::Tool);
    assert_eq!(
        messages[2]
            .tool_call_id
            .as_ref()
            .map(tqa_types::ToolCallId::as_str),
        Some("a")
    );
    assert_eq!(messages[3].text, "result b");
    assert_eq!(messages[4].text, "next run");
    Ok(())
}

#[test]
fn ctx_u_010_partial_highest_attempt_never_resurrects_an_older_response()
-> Result<(), Box<dyn Error>> {
    let history = frames(vec![
        start("run")?,
        step(1),
        text("old complete"),
        model(1),
        step(2),
        text("new partial"),
    ]);
    assert!(project(&history)?.is_empty());
    let history = frames(vec![
        start("run")?,
        step(1),
        text("cancelled partial"),
        Event::RunCancelled {
            run_id: "run".parse()?,
        },
        start("next")?,
        step(1),
        text("new answer"),
        model(1),
    ]);
    assert_eq!(project(&history)?[0].text, "new answer");
    Ok(())
}

#[test]
fn ctx_u_010_impossible_logs_report_the_offending_sequence() -> Result<(), Box<dyn Error>> {
    let cases = vec![
        vec![start("run")?, text("outside step")],
        vec![start("run")?, step(1), result("unknown")?],
        vec![start("run")?, step(1), call("same")?, call("same")?],
        vec![start("run")?, step(1), step(1)],
        vec![start("run")?, step(1), model(1), text("too late")],
        vec![start("run")?, step(1), model(2)],
        vec![
            start("run")?,
            step(1),
            Event::StepCompleted {
                step: 1,
                duration_ms: 0,
            },
        ],
        vec![
            start("run")?,
            step(1),
            Event::StepStarted {
                step: 2,
                attempt: 1,
            },
        ],
        vec![
            start("run")?,
            step(1),
            model(1),
            Event::StepCompleted {
                step: 1,
                duration_ms: 0,
            },
            step(2),
        ],
        vec![
            start("run")?,
            step(1),
            call("a")?,
            model(1),
            result("a")?,
            result("a")?,
        ],
    ];
    for events in cases {
        let seq = Seq::from(events.len() as u64);
        let error = project(&frames(events))
            .err()
            .ok_or("expected invalid history")?;
        assert!(matches!(error,ProjectionError::Invalid {seq:actual,..} if actual==seq));
    }
    let mut history = frames(vec![start("run")?, step(1), text("wrong envelope")]);
    if let ServerFrame::Event { step, .. } = &mut history[2] {
        *step = Some(2);
    }
    assert!(project(&history).is_err());
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {cases:100,rng_seed:proptest::test_runner::RngSeed::Fixed(11),failure_persistence:None,..ProptestConfig::default()})]
    #[test]
    fn ctx_u_010_project_serialized_parsed_history_is_stable(user in ".{1,32}",first in ".{0,32}",second in ".{0,32}") {
        let history=frames(vec![Event::MessageUser { lead_brief: None, team_wake: None,text:user,attachments:vec![]},start("run")?,step(1),text(&first),step(2),text(&second),model(2),Event::StepCompleted { step: 1, duration_ms: 0 }]);
        let mut decoded = Vec::new();
        for frame in &history { decoded.push(ServerFrame::parse(&serde_json::to_vec(frame)?)?); }
        prop_assert_eq!(project(&history)?,project(&decoded)?);
    }
}
