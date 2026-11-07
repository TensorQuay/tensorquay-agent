//! TOOL-U-037 (the loop's half): a tool may decline to be offered, and the model call after it is offered again carries
//! it, every later call the same list: the tools prefix changes exactly once (ADR-0034: a task's lead tools appear from
//! the model call after its first successful `start_member`).
mod support;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_tools::{Tool, ToolContext, ToolError, ToolOutput, ToolRegistry, ToolSpec};

/// `start`: always offered; its call sets the shared flag, as `start_member`'s `lead_tools: true` does.
struct Start {
    spec: ToolSpec,
    leading: Arc<AtomicBool>,
}

/// `assign`: offered only once the flag is set.
struct Gated {
    spec: ToolSpec,
    leading: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl Tool for Start {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, _: serde_json::Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        self.leading.store(true, Ordering::SeqCst);
        Ok(ToolOutput::text("started"))
    }
}

#[async_trait::async_trait]
impl Tool for Gated {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn offered(&self) -> bool {
        self.leading.load(Ordering::SeqCst)
    }
    async fn call(&self, _: serde_json::Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::text("sent"))
    }
}

#[tokio::test]
async fn tool_u_037_a_declined_tool_is_offered_from_the_call_after_its_flag_and_the_prefix_changes_once()
-> TestResult {
    let leading = Arc::new(AtomicBool::new(false));
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(Start {
        spec: spec("start", false)?,
        leading: leading.clone(),
    }))?;
    tools.register(Arc::new(Gated {
        spec: spec("assign", false)?,
        leading: leading.clone(),
    }))?;
    let h = Harness::new(
        vec![
            calls(&[("c1", "start", r#"{"path":"a"}"#)]),
            calls(&[("c2", "assign", r#"{"path":"b"}"#)]),
            answer(),
        ],
        tools,
    )?;
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let offered: Vec<Vec<String>> = h
        .provider
        .requests()
        .iter()
        .map(|request| request.tools.iter().map(|tool| tool.name.clone()).collect())
        .collect();
    assert_eq!(
        offered,
        [
            vec!["start".to_owned()],
            vec!["assign".to_owned(), "start".to_owned()],
            vec!["assign".to_owned(), "start".to_owned()],
        ],
        "assign is offered from the call after start, and the list stays the same after that"
    );
    Ok(())
}
