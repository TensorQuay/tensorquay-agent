//! CORE-E-042: TOOL-U-146 through the runner, as the 18.0 soak's rehearsal met it (18 Nov 2026). The model's shell
//! call gives `timeout_ms` as `"30000"`, a `["integer", "null"]` property left as text by one model server: the call
//! runs, the run completes, and both the journal and the tool see `30000`. A near literal (`" 30000"`) stays text: it
//! is refused with what was sent, and three of them still stop the run, as CORE-E-041's guard does.
mod support;
use serde_json::json;
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_testkit::FakeTool;
use tqa_types::{ErrorCode, Event, ToolStatus};

/// The built-in shell's schema, as the runtime offers it (crates/tqa-tools/src/builtin/shell.rs).
const SHELL: &str = r#"{"type":"object","required":["command","cwd","timeout_ms"],"additionalProperties":false,
    "properties":{"command":{"type":"string"},"cwd":{"type":["string","null"]},
    "timeout_ms":{"type":["integer","null"],"minimum":1}}}"#;

/// The soak's call, as the engine gave it.
const SOAK_CALL: &str = r#"{"command":"printf soak-0 > soak.txt; sleep 20; cat soak.txt","cwd":"/workspace","timeout_ms":"30000"}"#;

fn shell() -> Result<Arc<FakeTool>, Box<dyn Error>> {
    let spec = tqa_tools::ToolSpec {
        name: tqa_tools::ToolName::new("shell")?,
        description: "Run a command.".into(),
        parameters: serde_json::from_str(SHELL)?,
        parallel_safe: false,
        changes_workspace: true,
    };
    let replies = (0..3).map(|_| output("soak-0", 0)).collect();
    // Our own tools normalise their arguments (TOOL-U-034); the shell is one.
    Ok(Arc::new(
        FakeTool::new(spec, replies).with_null_means_absent(),
    ))
}

async fn run(
    calls_made: &[&str],
    tool: &Arc<FakeTool>,
) -> Result<(RunOutcome, Vec<Event>), Box<dyn Error>> {
    let script = calls_made
        .iter()
        .enumerate()
        .map(|(index, arguments)| calls(&[(&format!("c{index}"), "shell", arguments)]))
        .chain(std::iter::once(answer()))
        .collect();
    let h = Harness::new(script, registry(std::slice::from_ref(tool))?)?;
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    Ok((outcome, h.sink.events()))
}

// TEST: CORE-E-042
#[tokio::test]
async fn core_e_042_a_number_written_as_text_runs_and_the_run_completes()
-> Result<(), Box<dyn Error>> {
    let tool = shell()?;
    let (outcome, events) = run(&[SOAK_CALL], &tool).await?;
    assert!(matches!(outcome, RunOutcome::Completed), "{outcome:?}");
    let sent = json!({"command": "printf soak-0 > soak.txt; sleep 20; cat soak.txt", "cwd": "/workspace",
        "timeout_ms": 30000});
    assert_eq!(tool.calls(), vec![sent.clone()], "the tool got the number");
    let journaled: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolCall { arguments, .. } => Some(arguments.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(journaled, vec![sent], "the journal shows what the tool got");
    let results = tool_results(&events);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, ToolStatus::Ok, "{results:?}");
    Ok(())
}

// TEST: CORE-E-042
#[tokio::test]
async fn core_e_042_a_near_literal_is_refused_by_name_and_the_guard_still_stops()
-> Result<(), Box<dyn Error>> {
    let tool = shell()?;
    let near = SOAK_CALL.replace(r#""30000""#, r#"" 30000""#);
    let (outcome, events) = run(&[&near, &near, &near], &tool).await?;
    assert!(
        matches!(
            &outcome,
            RunOutcome::Failed {
                code: ErrorCode::RepeatedFailure,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert!(
        tool.calls().is_empty(),
        "a refused call never reaches the tool"
    );
    let results = tool_results(&events);
    assert_eq!(results.len(), 3);
    for (_, status, text) in &results {
        assert_eq!(*status, ToolStatus::Error);
        assert!(
            text.contains("expected null or integer, got a string"),
            "{text}"
        );
    }
    Ok(())
}
