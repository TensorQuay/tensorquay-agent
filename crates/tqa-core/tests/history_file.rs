//! CORE-E-043 (owner, 18 Nov 2026: after a summary, a question about something long ago must be answerable with a
//! tool): each compaction appends the turns it replaces to the task's `.tqa/<id>/history.md`, once per `through_seq`,
//! and names that file in the summary's `key_paths`, so the projected summary tells the model to search it. A run that
//! never compacts writes nothing; a history the runner can't write leaves the compaction as it was before.
mod support;
use serde_json::json;
use std::error::Error;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner, UserTurn};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_testkit::ProviderReply;
use tqa_tools::ToolRegistry;
use tqa_types::{CompactionSummary, Event, FinishReason as WireFinish, ServerFrame, ToolStatus};

const HISTORY: &str = ".tqa/conversation/history.md";
const RESULT_BYTES: usize = 1050;

fn completed_run(index: usize, seq: usize) -> Result<Vec<Event>, Box<dyn Error>> {
    let run_id: tqa_types::RunId = format!("r-{seq}").parse()?;
    let call_id: tqa_types::ToolCallId = format!("c{index}").parse()?;
    let text = if index == 3 {
        "note: the code word is ORCHID-7".to_owned()
    } else {
        format!("request {index}")
    };
    Ok(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text,
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: run_id.clone(),
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
        Event::ToolCall {
            call_id: call_id.clone(),
            name: "read_file".into(),
            arguments: json!({"path": format!("src/{index}.rs")}),
        },
        Event::ModelCompleted {
            step: 1,
            attempt: 1,
            finish_reason: WireFinish::ToolCalls,
        },
        Event::ToolResult {
            call_id,
            status: ToolStatus::Ok,
            content: format!("{index:04}{}", "r".repeat(RESULT_BYTES - 4)),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::StepStarted {
            step: 2,
            attempt: 1,
        },
        Event::TextDelta {
            text: format!("answer {index}"),
        },
        Event::ModelCompleted {
            step: 2,
            attempt: 1,
            finish_reason: WireFinish::Stop,
        },
        Event::StepCompleted {
            step: 2,
            duration_ms: 0,
        },
        Event::RunCompleted { run_id },
    ])
}

fn long_history(runs: usize) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let mut events = Vec::new();
    for index in 1..=runs {
        let seq = events.len() + 1;
        events.extend(completed_run(index, seq)?);
    }
    history(events)
}

fn text_reply(text: &str) -> ProviderReply {
    reply(vec![
        StreamChunk::TextDelta(text.into()),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ])
}

fn model_summary() -> CompactionSummary {
    CompactionSummary {
        goal: "answer the requests".into(),
        key_paths: vec!["src/1.rs".into()],
        ..CompactionSummary::default()
    }
}

/// A run on a small window over `frames`; returns the outcome, the journal and the workspace (kept alive).
async fn run(
    frames: &[ServerFrame],
    script: Vec<ProviderReply>,
    before: impl FnOnce(&std::path::Path) -> Result<(), Box<dyn Error>>,
) -> Result<(RunOutcome, Vec<Event>, tempfile::TempDir), Box<dyn Error>> {
    let mut h = Harness::new(script, ToolRegistry::new())?;
    h.deps.route.context_window = Some(8192);
    h.deps.route.max_output = Some(1024);
    h.deps.limits.max_output_tokens = 1_000_000;
    h.sink_with(MemorySink::new(frames));
    before(h.directory.path())?;
    let sink = h.sink.clone();
    let Harness {
        directory, deps, ..
    } = h;
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "what was the code word?".into(),
        attachments: vec![],
    };
    let outcome = Runner::new(deps)?
        .run(
            frames,
            Some(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    Ok((outcome, sink.events(), directory))
}

fn summaries(events: &[Event]) -> Vec<CompactionSummary> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Compaction { summary, .. } => Some(summary.clone()),
            _ => None,
        })
        .collect()
}

// TEST: CORE-E-043
#[tokio::test]
async fn core_e_043_a_compaction_writes_the_replaced_turns_and_names_the_file()
-> Result<(), Box<dyn Error>> {
    let frames = long_history(20)?;
    let summary = serde_json::to_string(&model_summary())?;
    let (outcome, events, directory) =
        run(&frames, vec![text_reply(&summary), answer()], |_| Ok(())).await?;
    assert_eq!(outcome, RunOutcome::Completed);
    let written = summaries(&events);
    assert_eq!(written.len(), 1, "one compaction");
    assert_eq!(
        written[0].key_paths,
        vec!["src/1.rs".to_owned(), HISTORY.to_owned()],
        "the model's paths, then it"
    );
    let text = std::fs::read_to_string(directory.path().join(HISTORY))?;
    assert!(
        text.contains("the code word is ORCHID-7"),
        "the middle detail is in the history"
    );
    assert!(text.contains("src/3.rs"), "with its tool call");
    assert_eq!(
        text.matches("<!-- through_seq ").count(),
        1,
        "one section:\n{text}"
    );
    Ok(())
}

// TEST: CORE-E-043
#[tokio::test]
async fn core_e_043_a_retried_compaction_appends_its_section_once() -> Result<(), Box<dyn Error>> {
    let frames = long_history(20)?;
    let summary = serde_json::to_string(&model_summary())?;
    // A first attempt wrote the section and stopped before journaling the compaction.
    let (_, events, directory) =
        run(&frames, vec![text_reply(&summary), answer()], |_| Ok(())).await?;
    let first = std::fs::read_to_string(directory.path().join(HISTORY))?;
    let through = events
        .iter()
        .find_map(|event| match event {
            Event::Compaction { through_seq, .. } => Some(*through_seq),
            _ => None,
        })
        .ok_or("no compaction")?;
    let (_, _, again) = run(&frames, vec![text_reply(&summary), answer()], |root| {
        let path = root.join(HISTORY);
        std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        std::fs::write(path, &first)?;
        Ok(())
    })
    .await?;
    let text = std::fs::read_to_string(again.path().join(HISTORY))?;
    assert_eq!(
        text.matches(&format!("<!-- through_seq {through} -->"))
            .count(),
        1,
        "{text}"
    );
    assert_eq!(text, first, "nothing added for the same through_seq");
    Ok(())
}

// TEST: CORE-E-043
#[tokio::test]
async fn core_e_043_a_run_that_never_compacts_writes_no_history() -> Result<(), Box<dyn Error>> {
    let frames = long_history(2)?;
    let (outcome, events, directory) = run(&frames, vec![answer()], |_| Ok(())).await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert!(summaries(&events).is_empty());
    assert!(!directory.path().join(HISTORY).exists());
    Ok(())
}

// TEST: CORE-E-043
#[tokio::test]
async fn core_e_043_a_history_that_cannot_be_written_leaves_the_compaction_as_before()
-> Result<(), Box<dyn Error>> {
    let frames = long_history(20)?;
    let summary = serde_json::to_string(&model_summary())?;
    // `history.md` is a directory: the write fails.
    let (outcome, events, _directory) =
        run(&frames, vec![text_reply(&summary), answer()], |root| {
            std::fs::create_dir_all(root.join(HISTORY))?;
            Ok(())
        })
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        summaries(&events),
        vec![model_summary()],
        "no history path named"
    );
    Ok(())
}
