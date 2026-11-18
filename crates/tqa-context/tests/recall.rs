//! CTX-U-023 (owner, 18 Nov 2026: after a summary, a question about something long ago must be answerable with a
//! tool): a compaction plan renders the turns it replaces as readable text, for the task's history file; and the
//! summary names that file, with a clause to search it before answering about anything earlier, only when its
//! `key_paths` hold one.
#[allow(
    dead_code,
    reason = "This suite uses only the frame builder from the shared vocabulary."
)]
#[path = "support/history.rs"]
mod support;
use serde_json::{Value, json};
use std::error::Error;
use support::frames;
use tqa_context::{COMPACTION_SUMMARY_PREFIX, history_path, plan_compaction, project};
use tqa_types::{CompactionSource, Event, FinishReason, ToolStatus};

type TestResult = Result<(), Box<dyn Error>>;

fn user(text: &str) -> Event {
    Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: text.into(),
        attachments: vec![],
    }
}

fn step(
    number: u32,
    name: &str,
    arguments: Value,
    result: &str,
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
            status: ToolStatus::Ok,
            content: result.into(),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: number,
            duration_ms: 0,
        },
    ])
}

/// A first message, a middle one with a detail, then enough later turns that the middle one is summarised.
fn long_task(later: u32) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut events = vec![
        user("start the report"),
        Event::RunStarted {
            run_id: "r".parse()?,
        },
    ];
    events.extend(step(
        1,
        "read_file",
        json!({"path": "plan.md"}),
        "the plan",
    )?);
    events.push(user("note: the code word is ORCHID-7"));
    events.extend(step(
        2,
        "shell",
        json!({"command": "ls reports"}),
        "q1.csv q2.csv",
    )?);
    for number in 3..3 + later {
        events.push(user(&format!("next part {number}")));
        events.extend(step(
            number,
            "read_file",
            json!({"path": format!("part{number}.md")}),
            "text",
        )?);
    }
    Ok(events)
}

// TEST: CTX-U-023
#[test]
fn ctx_u_023_the_replaced_turns_read_as_text_in_order() -> TestResult {
    let plan = plan_compaction(&frames(long_task(10)?))?.ok_or("nothing to compact")?;
    let text = plan.replaced_text();
    let word = text
        .find("the code word is ORCHID-7")
        .ok_or("the middle message isn't in the history")?;
    let call = text
        .find("ls reports")
        .ok_or("the shell call's arguments aren't there")?;
    let result = text
        .find("q1.csv q2.csv")
        .ok_or("the shell result isn't there")?;
    assert!(word < call && call < result, "in order:\n{text}");
    assert!(text.contains("shell"), "the tool's name:\n{text}");
    // Turns kept verbatim in the context aren't repeated in the history.
    assert!(
        !text.contains(&format!("next part {}", 3 + 9)),
        "the newest turn is kept, not replaced:\n{text}"
    );
    Ok(())
}

// TEST: CTX-U-023
#[test]
fn ctx_u_023_a_long_result_is_bounded_and_says_how_much_was_cut() -> TestResult {
    let mut events = long_task(10)?;
    let big = "x".repeat(40_000);
    // The middle turn's result becomes 40 KB.
    for event in &mut events {
        if let Event::ToolResult { content, .. } = event
            && content == "q1.csv q2.csv"
        {
            *content = format!("q1.csv q2.csv {big}");
        }
    }
    let plan = plan_compaction(&frames(events))?.ok_or("nothing to compact")?;
    let text = plan.replaced_text();
    assert!(text.contains("q1.csv q2.csv"), "the head is kept");
    assert!(text.len() < 30_000, "bounded: {} bytes", text.len());
    assert!(text.contains("more bytes"), "says what was cut");
    Ok(())
}

// TEST: CTX-U-023
#[test]
fn ctx_u_023_the_history_path_sits_beside_the_notes() -> TestResult {
    assert_eq!(history_path(&"c1".parse()?), ".tqa/c1/history.md");
    Ok(())
}

fn summary_text(key_paths: Vec<String>) -> Result<String, Box<dyn Error>> {
    let mut events = long_task(10)?;
    let plan = plan_compaction(&frames(events.clone()))?.ok_or("nothing to compact")?;
    let mut summary = plan.fallback;
    summary.key_paths = key_paths;
    events.push(Event::Compaction {
        through_seq: plan.through_seq,
        source: CompactionSource::Fallback,
        version: tqa_types::COMPACTION_VERSION,
        summary,
    });
    project(&frames(events))?
        .into_iter()
        .find(|message| message.text.starts_with(COMPACTION_SUMMARY_PREFIX))
        .map(|message| message.text)
        .ok_or_else(|| "no summary".into())
}

// TEST: CTX-U-023
#[test]
fn ctx_u_023_the_summary_names_the_history_file_only_when_there_is_one() -> TestResult {
    let with = summary_text(vec!["src/report.rs".into(), ".tqa/c1/history.md".into()])?;
    assert!(
        with.ends_with(
            "\nThe full text of the summarised turns is in .tqa/c1/history.md: search it (grep, then read_file) before \
             answering about anything earlier, and never guess."
        ),
        "{with}"
    );
    // A summary from before the history file (or whose write failed) reads as it always did.
    let without = summary_text(vec!["src/report.rs".into()])?;
    assert!(without.ends_with(']'), "{without}");
    assert!(!without.contains("history.md"), "{without}");
    Ok(())
}
