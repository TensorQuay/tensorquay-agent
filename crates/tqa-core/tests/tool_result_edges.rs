//! CORE-U-018 and CTX-U-006 edges: a tool's own cancellation is recorded as cancelled, and a result that cannot be
//! preserved in its spill file becomes a bounded error instead of an unbounded or lost result.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_testkit::FakeToolReply;
use tqa_tools::ToolError;
use tqa_types::ToolStatus;

#[tokio::test]
async fn core_u_018_a_tool_that_reports_cancellation_records_a_cancelled_result() -> TestResult {
    let reply = FakeToolReply {
        delay: std::time::Duration::ZERO,
        result: Err(ToolError::Cancelled),
    };
    let tool = fake("read_file", true, vec![reply])?;
    let h = Harness::new(
        vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
        registry(&[tool])?,
    )?;
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(
        outcome,
        RunOutcome::Completed,
        "the run goes on to the next step"
    );
    let results = tool_results(&sink.events());
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].1,
        ToolStatus::Cancelled,
        "not an error: {results:?}"
    );
    Ok(())
}

#[tokio::test]
async fn ctx_u_006_a_result_whose_spill_cannot_be_written_becomes_a_bounded_error() -> TestResult {
    let big = "x".repeat(10_000);
    let tool = fake("read_file", true, vec![output(&big, 0)])?;
    let mut h = Harness::new(
        vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
        registry(&[tool])?,
    )?;
    h.deps.limits.tool_result_bytes = 1024;
    // A plain file where the spill directory's parent should be: the spill write fails.
    std::fs::write(h.directory.path().join(".tqa"), "not a directory")?;
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    let results = tool_results(&sink.events());
    assert_eq!(results.len(), 1);
    let (_, status, content) = &results[0];
    assert_eq!(*status, ToolStatus::Error);
    assert!(
        content.contains("tool result could not be preserved"),
        "{content}"
    );
    assert!(
        !content.contains(&big[..100]),
        "the unbounded output never reaches the model"
    );
    Ok(())
}
