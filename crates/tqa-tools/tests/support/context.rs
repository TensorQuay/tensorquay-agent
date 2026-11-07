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
    }
}
