//! CTX-U-014 / CTX-U-013: the latest run notes are pinned after compaction, from the journal alone.
#[allow(
    dead_code,
    reason = "This suite uses only the frame builder from the shared vocabulary."
)]
#[path = "support/history.rs"]
mod support;
use serde_json::{Value, json};
use std::error::Error;
use support::frames;
use tqa_context::{COMPACTION_SUMMARY_PREFIX, Message, RUN_NOTES_PREFIX, plan_compaction, project};
use tqa_types::{CompactionSource, Event, FinishReason, ToolStatus};

type TestResult = Result<(), Box<dyn Error>>;

fn step(
    number: u32,
    name: &str,
    arguments: Value,
    status: ToolStatus,
) -> Result<Vec<Event>, Box<dyn Error>> {
    let call_id: tqa_types::ToolCallId = format!("c{number}").parse()?;
    Ok(vec![
        Event::StepStarted {
            step: number,
            attempt: 1,
        },
        Event::ToolCall {
            call_id: call_id.clone(),
            name: name.into(),
            arguments,
        },
        Event::ModelCompleted {
            step: number,
            attempt: 1,
            finish_reason: FinishReason::ToolCalls,
        },
        Event::ToolResult {
            call_id,
            status,
            content: "ok".into(),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: number,
            duration_ms: 0,
        },
    ])
}
/// Three note writes (the last one failed) followed by `reads` ordinary steps.
fn history(reads: u32) -> Result<Vec<Event>, Box<dyn Error>> {
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
    ];
    events.extend(step(
        1,
        "write_notes",
        json!({"text": "first"}),
        ToolStatus::Ok,
    )?);
    events.extend(step(
        2,
        "write_notes",
        json!({"text": "latest \"notes\"\n"}),
        ToolStatus::Ok,
    )?);
    events.extend(step(
        3,
        "write_notes",
        json!({"text": "rejected"}),
        ToolStatus::Error,
    )?);
    for number in 4..4 + reads {
        events.extend(step(
            number,
            "read_file",
            json!({"path": format!("{number}.rs")}),
            ToolStatus::Ok,
        )?);
    }
    Ok(events)
}
fn compacted(mut events: Vec<Event>) -> Result<Vec<Event>, Box<dyn Error>> {
    let plan = plan_compaction(&frames(events.clone()))?.ok_or("nothing to compact")?;
    events.push(Event::Compaction {
        through_seq: plan.through_seq,
        source: CompactionSource::Fallback,
        version: tqa_types::COMPACTION_VERSION,
        summary: plan.fallback,
    });
    Ok(events)
}
fn notes(messages: &[Message]) -> Result<Vec<String>, Box<dyn Error>> {
    messages
        .iter()
        .filter_map(|message| message.text.strip_prefix(RUN_NOTES_PREFIX))
        .map(|block| {
            let data = block.strip_suffix(']').ok_or("unterminated notes block")?;
            Ok(serde_json::from_str::<String>(data)?)
        })
        .collect()
}

#[test]
fn ctx_u_014_latest_successful_notes_follow_the_summary_verbatim() -> TestResult {
    let messages = project(&frames(compacted(history(8)?)?))?;
    let summary = messages
        .iter()
        .position(|message| message.text.starts_with(COMPACTION_SUMMARY_PREFIX))
        .ok_or("no summary")?;
    assert!(messages[summary + 1].text.starts_with(RUN_NOTES_PREFIX));
    assert_eq!(notes(&messages)?, vec!["latest \"notes\"\n".to_owned()]);
    Ok(())
}

#[test]
fn ctx_u_014_without_compaction_the_note_calls_are_already_in_history() -> TestResult {
    let messages = project(&frames(history(8)?))?;
    assert!(notes(&messages)?.is_empty());
    Ok(())
}

#[test]
fn ctx_u_014_notes_survive_a_second_compaction() -> TestResult {
    let mut events = compacted(history(8)?)?;
    for number in 12..22 {
        events.extend(step(
            number,
            "read_file",
            json!({"path": "x"}),
            ToolStatus::Ok,
        )?);
    }
    let messages = project(&frames(compacted(events)?))?;
    assert_eq!(notes(&messages)?, vec!["latest \"notes\"\n".to_owned()]);
    Ok(())
}

#[test]
fn ctx_u_014_no_notes_means_no_notes_message() -> TestResult {
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
    ];
    for number in 1..12 {
        events.extend(step(
            number,
            "read_file",
            json!({"path": "x"}),
            ToolStatus::Ok,
        )?);
    }
    let messages = project(&frames(compacted(events)?))?;
    assert!(notes(&messages)?.is_empty());
    Ok(())
}
