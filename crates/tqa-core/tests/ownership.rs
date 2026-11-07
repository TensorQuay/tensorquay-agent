//! CORE-E-030: cancellation is reflected before cleanup completes, with every worker joined.
mod support;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use support::*;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_tools::{Tool, ToolContext, ToolError, ToolOutput, ToolRegistry, ToolSpec};

struct CleanupTool {
    spec: ToolSpec,
    active: Arc<AtomicBool>,
    release: Arc<Notify>,
}
#[async_trait]
impl Tool for CleanupTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, _: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        self.active.store(true, Ordering::SeqCst);
        cx.cancel.cancelled().await;
        self.release.notified().await;
        self.active.store(false, Ordering::SeqCst);
        Err(ToolError::Cancelled)
    }
}
#[tokio::test]
async fn core_e_030_terminal_precedes_cleanup_but_run_owns_and_joins_it() -> TestResult {
    let active = Arc::new(AtomicBool::new(false));
    let release = Arc::new(Notify::new());
    let tool = CleanupTool {
        spec: spec("cleanup", true)?,
        active: active.clone(),
        release: release.clone(),
    };
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(tool))?;
    let h = Harness::new(
        vec![calls(&[("a", "cleanup", r#"{"path":"a"}"#)])],
        registry,
    )?;
    let cancel = CancellationToken::new();
    let runner = Runner::new(h.deps)?;
    let run = runner.run(&[], Some(turn()), cancel.clone(), CancellationToken::new());
    tokio::pin!(run);
    assert!(futures::poll!(&mut run).is_pending());
    for _ in 0..100 {
        tokio::task::yield_now().await;
        if active.load(Ordering::SeqCst) {
            break;
        }
    }
    assert!(active.load(Ordering::SeqCst));
    cancel.cancel();
    assert!(futures::poll!(&mut run).is_pending());
    assert_eq!(
        h.sink.types().last().map(String::as_str),
        Some("run.cancelled")
    );
    assert!(active.load(Ordering::SeqCst));
    let ended = h.sink.frames();
    release.notify_one();
    assert_eq!(run.await?, RunOutcome::Cancelled);
    assert!(!active.load(Ordering::SeqCst));
    assert_eq!(h.sink.frames(), ended);
    Ok(())
}
