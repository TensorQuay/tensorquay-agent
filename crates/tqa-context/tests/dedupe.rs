//! CTX-U-012: a tool result identical to a later one becomes a stub naming the later call; changed content stays.
#[allow(
    dead_code,
    reason = "This suite uses only the frame builder from the shared vocabulary."
)]
#[path = "support/history.rs"]
mod support;
use serde_json::json;
use std::error::Error;
use support::frames;
use tqa_context::{DEDUPE_MIN_BYTES, Role, dedupe_stub, project};
use tqa_types::{Event, FinishReason, ToolCallId, ToolStatus};

type TestResult = Result<(), Box<dyn Error>>;

/// One step that reads `path` and gets `content`.
fn read(step: u32, path: &str, content: &str) -> Result<Vec<Event>, Box<dyn Error>> {
    let call_id: ToolCallId = format!("c{step}").parse()?;
    Ok(vec![
        Event::StepStarted { step, attempt: 1 },
        Event::ToolCall {
            call_id: call_id.clone(),
            name: "read_file".into(),
            arguments: json!({"path": path}),
        },
        Event::ModelCompleted {
            step,
            attempt: 1,
            finish_reason: FinishReason::ToolCalls,
        },
        Event::ToolResult {
            call_id,
            status: ToolStatus::Ok,
            content: content.into(),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step,
            duration_ms: 0,
        },
    ])
}

fn run(steps: Vec<Vec<Event>>) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut events = vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "task".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: "r-1".parse()?,
        },
    ];
    events.extend(steps.into_iter().flatten());
    events.push(Event::RunCompleted {
        run_id: "r-1".parse()?,
    });
    Ok(events)
}

#[test]
fn ctx_u_012_repeated_reads_keep_only_the_latest_copy_and_changed_content_stays() -> TestResult {
    let original = format!("fn main() {{}}\n{}", "// padding\n".repeat(40));
    let modified = format!("{original}// changed\n");
    let small = "ok";
    assert!(original.len() >= DEDUPE_MIN_BYTES && small.len() < DEDUPE_MIN_BYTES);
    let events = run(vec![
        read(1, "src/main.rs", &original)?,
        read(2, "src/main.rs", &original)?,
        read(3, "src/main.rs", &original)?,
        read(4, "src/main.rs", &modified)?,
        read(5, "src/main.rs", &modified)?,
        read(6, "status", small)?,
        read(7, "status", small)?,
    ])?;
    let messages = project(&frames(events))?;
    let results: Vec<(String, String)> = messages
        .iter()
        .filter(|message| message.role == Role::Tool)
        .map(|message| {
            (
                message
                    .tool_call_id
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                message.text.clone(),
            )
        })
        .collect();
    let latest_original: ToolCallId = "c3".parse()?;
    let latest_modified: ToolCallId = "c5".parse()?;
    let expected = [
        ("c1", dedupe_stub(&latest_original)),
        ("c2", dedupe_stub(&latest_original)),
        ("c3", original.clone()),
        ("c4", dedupe_stub(&latest_modified)),
        ("c5", modified.clone()),
        ("c6", small.to_owned()),
        ("c7", small.to_owned()),
    ];
    let expected: Vec<(String, String)> = expected
        .into_iter()
        .map(|(id, text)| (id.to_owned(), text))
        .collect();
    assert_eq!(results, expected);
    // Every result keeps its call: the stub replaces content, never the pairing.
    let calls = messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .count();
    assert_eq!(calls, results.len());
    Ok(())
}

#[test]
fn ctx_u_012_the_stub_names_the_later_call_in_one_line() -> TestResult {
    let stub = dedupe_stub(&"call-9".parse()?);
    assert!(stub.contains("call-9") && !stub.contains('\n') && stub.len() < DEDUPE_MIN_BYTES);
    Ok(())
}
