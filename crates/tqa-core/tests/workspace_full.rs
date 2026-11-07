//! CORE-U-049: a run tells its user once that the workspace is full (SBX-C-015), before the result of the first call
//! that failed for lack of space, and never for other failures.
mod support;
use std::time::Duration;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::Runner;
use tqa_testkit::FakeToolReply;
use tqa_tools::{ToolError, ToolOutput, WorkspaceRoot};
use tqa_types::{Event, ToolStatus};

fn failing(error: ToolError) -> FakeToolReply {
    FakeToolReply {
        delay: Duration::ZERO,
        result: Err(error),
    }
}

fn exited(code: i32) -> FakeToolReply {
    let mut output = ToolOutput::text("write error: No space left on device");
    output.exit_code = Some(code);
    FakeToolReply {
        delay: Duration::ZERO,
        result: Ok(output),
    }
}

/// The calls named by every `workspace.full`, and whether each came just before its call's `tool.result`.
fn full_events(events: &[Event]) -> Vec<(String, bool)> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            Event::WorkspaceFull { call_id } => Some((
                call_id.to_string(),
                matches!(events.get(index + 1),
                    Some(Event::ToolResult { call_id: next, .. }) if next == call_id),
            )),
            _ => None,
        })
        .collect()
}

async fn run(h: Harness) -> TestResult {
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn core_u_049_a_file_tool_out_of_space_is_reported_once_before_its_result() -> TestResult {
    let tool = fake(
        "write_file",
        false,
        vec![
            failing(ToolError::WorkspaceFull),
            failing(ToolError::WorkspaceFull),
        ],
    )?;
    let h = Harness::new(
        vec![
            calls(&[("a", "write_file", r#"{"path":"a"}"#)]),
            calls(&[("b", "write_file", r#"{"path":"b"}"#)]),
            answer(),
        ],
        registry(&[tool])?,
    )?;
    let sink = h.sink.clone();
    run(h).await?;
    assert_eq!(full_events(&sink.events()), vec![("a".into(), true)]);
    let results = tool_results(&sink.events());
    assert!(
        results
            .iter()
            .all(|(_, status, text)| *status == ToolStatus::Error
                && text.starts_with("workspace_full: ")),
        "{results:?}"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_049_a_failed_command_counts_only_when_the_workspace_has_no_space() -> TestResult {
    for (full, replies, expected) in [
        // procfs reports no available blocks: a failed command there ran out of space.
        (true, vec![exited(1)], vec![("a".to_owned(), true)]),
        (true, vec![exited(0)], vec![]),
        (false, vec![exited(1)], vec![]),
    ] {
        let tool = fake("shell", false, replies)?;
        let mut h = Harness::new(
            vec![calls(&[("a", "shell", r#"{"path":"a"}"#)]), answer()],
            registry(&[tool])?,
        )?;
        if full {
            h.deps.workspace = WorkspaceRoot::open("/proc")?;
        }
        let sink = h.sink.clone();
        run(h).await?;
        assert_eq!(full_events(&sink.events()), expected, "full: {full}");
    }
    Ok(())
}

#[tokio::test]
async fn core_u_049_other_failures_never_report_a_full_workspace() -> TestResult {
    for error in [
        ToolError::Io("StorageFull is only ever WorkspaceFull".into()),
        ToolError::Timeout,
        ToolError::NotFound,
    ] {
        let tool = fake("write_file", false, vec![failing(error)])?;
        let mut h = Harness::new(
            vec![calls(&[("a", "write_file", r#"{"path":"a"}"#)]), answer()],
            registry(&[tool])?,
        )?;
        // Even with the filesystem full: only a call that failed for lack of space is reported.
        h.deps.workspace = WorkspaceRoot::open("/proc")?;
        let sink = h.sink.clone();
        run(h).await?;
        assert_eq!(full_events(&sink.events()), vec![]);
    }
    Ok(())
}

#[tokio::test]
async fn core_u_049_a_resumed_run_does_not_report_it_again() -> TestResult {
    let mut events = initial()?;
    events.extend([
        call("a")?,
        model(),
        Event::WorkspaceFull {
            call_id: "a".parse()?,
        },
        Event::ToolResult {
            call_id: "a".parse()?,
            status: ToolStatus::Error,
            content: ToolError::WorkspaceFull.model_message(),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 10,
        },
    ]);
    let history = history(events)?;
    let tool = fake("write_file", false, vec![failing(ToolError::WorkspaceFull)])?;
    let mut h = Harness::new(
        vec![calls(&[("b", "write_file", r#"{"path":"b"}"#)]), answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.sink_with(MemorySink::new(&history));
    let sink = h.sink.clone();
    Runner::new(h.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(
        tool.calls().len(),
        1,
        "the resumed run made the second call"
    );
    assert_eq!(
        full_events(&sink.events()),
        vec![("a".into(), true)],
        "only the journaled one"
    );
    Ok(())
}
