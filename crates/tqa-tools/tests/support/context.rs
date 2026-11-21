//! Shared explicit test context.
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tqa_testkit::WorkspaceFixture;
use tqa_tools::{ToolContext, ToolLimits};
use tqa_types::{Clock, Sleeper};

struct RealTime;
impl Clock for RealTime {
    fn now(&self) -> Instant {
        Instant::now()
    }
}
impl Sleeper for RealTime {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

pub(crate) fn context(fixture: &WorkspaceFixture) -> ToolContext {
    ToolContext {
        workspace: fixture.workspace().clone(),
        cancel: tokio_util::sync::CancellationToken::new(),
        clock: Arc::new(RealTime),
        sleeper: Arc::new(RealTime),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
        // Memory on, as a run with TQA_MEMORY=1 has it: the file tools guard, keep and record notes (ADR-0041). A test
        // of memory off sets this to None.
        memory: note_writer(),
    }
}

/// The writer the file tools record for a note in these tests (always parses).
pub(crate) fn note_writer() -> Option<tqa_tools::NoteWriter> {
    Some(tqa_tools::NoteWriter {
        task: "01a1f9c3-f35b-7207-bbdf-1540a49b989f".parse().ok()?,
        run: "r-1".parse().ok()?,
        call: "call-1".parse().ok()?,
    })
}
