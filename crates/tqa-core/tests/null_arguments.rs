//! CORE-U-056: a call that sends `null` for an unused optional argument and one that leaves it out record the same
//! `tool.call` arguments, the tool's real ones, and the prepared action and dispatch see exactly those.
mod support;
use serde_json::{Value, json};
use std::sync::Arc;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_testkit::FakeTool;
use tqa_tools::{ToolName, ToolSpec};
use tqa_types::Event;

/// A tool whose schema is strict-valid, like a builtin: every property required, the optional one nullable.
fn strict_tool() -> Result<Arc<FakeTool>, Box<dyn std::error::Error>> {
    let spec = ToolSpec {
        name: ToolName::new("read_file")?,
        description: "Read".into(),
        parameters: json!({"type":"object","additionalProperties":false,"required":["path","cursor"],
            "properties":{"path":{"type":"string"},"cursor":{"type":["string","null"]}}}),
        parallel_safe: true,
        changes_workspace: false,
    };
    Ok(Arc::new(
        FakeTool::new(spec, vec![output("one", 0), output("two", 0)]).with_null_means_absent(),
    ))
}

#[tokio::test]
async fn core_u_056_tool_call_records_the_tools_real_arguments() -> TestResult {
    let tool = strict_tool()?;
    let h = Harness::new(
        vec![
            calls(&[
                ("a", "read_file", r#"{"path":"x","cursor":null}"#),
                ("b", "read_file", r#"{"path":"x"}"#),
            ]),
            answer(),
        ],
        registry(std::slice::from_ref(&tool))?,
    )?;
    let sink = h.sink.clone();
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let recorded: Vec<Value> = sink
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::ToolCall { arguments, .. } => Some(arguments),
            _ => None,
        })
        .collect();
    assert_eq!(
        recorded,
        vec![json!({"path":"x"}), json!({"path":"x"})],
        "both record the real arguments"
    );
    assert_eq!(
        tool.calls(),
        vec![json!({"path":"x"}), json!({"path":"x"})],
        "dispatch sees them"
    );
    assert!(
        tool.prepared_calls()
            .iter()
            .all(|prepared| prepared["arguments"] == json!({"path":"x"})),
        "the prepared action (and so an approval's hash) sees them: {:?}",
        tool.prepared_calls()
    );
    Ok(())
}

/// An external (MCP-style) tool whose REQUIRED property is nullable: a deliberate `null` is recorded, prepared and
/// dispatched exactly as sent.
#[tokio::test]
async fn core_u_056_an_external_tools_required_null_is_kept_end_to_end() -> TestResult {
    let spec = ToolSpec {
        name: ToolName::new("file_issue")?,
        description: "File an issue".into(),
        parameters: json!({"type":"object","required":["title","assignee"],
            "properties":{"title":{"type":"string"},"assignee":{"type":["string","null"]}}}),
        parallel_safe: true,
        changes_workspace: false,
    };
    let tool = Arc::new(FakeTool::new(spec, vec![output("filed", 0)]));
    let h = Harness::new(
        vec![
            calls(&[("a", "file_issue", r#"{"title":"t","assignee":null}"#)]),
            answer(),
        ],
        registry(std::slice::from_ref(&tool))?,
    )?;
    let sink = h.sink.clone();
    let runner = Runner::new(h.deps)?;
    drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    let sent = json!({"title":"t","assignee":null});
    let recorded: Vec<Value> = sink
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::ToolCall { arguments, .. } => Some(arguments),
            _ => None,
        })
        .collect();
    assert_eq!(recorded, vec![sent.clone()]);
    assert_eq!(tool.calls(), vec![sent.clone()]);
    assert!(
        tool.prepared_calls()
            .iter()
            .all(|prepared| prepared["arguments"] == sent)
    );
    Ok(())
}
