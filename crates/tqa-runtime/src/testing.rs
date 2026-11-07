//! Non-default test seams. This module and its call sites are absent from production builds.
pub use crate::channels::GatewayChannels;
pub use crate::decision_relay::{RelayDecisionEngine, RelayEngineConfig};
pub use crate::link::{PROBE_WAIT, SETTLE_WAIT};
pub use crate::mcp::{
    CallOutput, ListedTool, McpCallError, McpFailure, StdioServer,
    gateway::{CallFailure, GatewayServer, NOTICE_BOUND as MCP_NOTICE_BOUND},
    supervisor::McpSupervisor,
    tool::McpTool,
};
pub use crate::supervisor::LINK_END_WAIT;
pub use crate::workspace::{WorkspaceFailure, WorkspaceInitializer};
use std::sync::{
    Arc, Condvar, Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;
/// Repository transport shared by runtime fixtures imported from other workspace crates.
pub use tqa_tools::GitCloner;

/// A deterministic pause point for testing a stalled supervisor or an append boundary.
pub struct TestGate {
    paused: watch::Sender<bool>,
    entered: AtomicBool,
    changed: Notify,
    blocking: Mutex<bool>,
    wake: Condvar,
}
impl Default for TestGate {
    fn default() -> Self {
        Self {
            paused: watch::channel(false).0,
            entered: AtomicBool::new(false),
            changed: Notify::new(),
            blocking: Mutex::new(false),
            wake: Condvar::new(),
        }
    }
}
impl TestGate {
    /// Pauses subsequent checkpoints and clears their observation marker.
    pub fn pause(&self) {
        self.entered.store(false, Ordering::SeqCst);
        *self.blocking.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.paused.send_replace(true);
    }
    /// Releases both asynchronous and blocking checkpoints.
    pub fn resume(&self) {
        *self.blocking.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.paused.send_replace(false);
        self.wake.notify_all();
    }
    /// Whether a paused checkpoint has been reached since the gate was paused.
    #[must_use]
    pub fn entered(&self) -> bool {
        self.entered.load(Ordering::SeqCst)
    }
    /// Waits for the paused checkpoint to be reached, with a bounded test-control deadline.
    ///
    /// # Errors
    /// Reports an unobserved checkpoint after five real seconds.
    pub async fn wait_until_entered(&self) -> Result<(), std::io::Error> {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.wait_entered())
            .await
            .map_err(std::io::Error::other)
    }
    async fn wait_entered(&self) {
        loop {
            let changed = self.changed.notified();
            if self.entered.load(Ordering::SeqCst) {
                return;
            }
            changed.await;
        }
    }
    pub(crate) async fn checkpoint(&self, cancel: &CancellationToken) {
        let mut paused = self.paused.subscribe();
        while *paused.borrow_and_update() {
            self.mark();
            tokio::select! {biased;()=cancel.cancelled()=>return,result=paused.changed()=>{if result.is_err(){return;}}}
        }
    }
    pub(crate) fn checkpoint_blocking(&self) {
        let mut paused = self.blocking.lock().unwrap_or_else(PoisonError::into_inner);
        while *paused {
            self.mark();
            paused = self
                .wake
                .wait(paused)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
    fn mark(&self) {
        self.entered.store(true, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
}
/// Injected test gates and storage failure; no value is discoverable from process configuration.
#[derive(Clone, Default)]
pub struct TestingHooks {
    /// Pause a run before service preparation to exercise decisions arriving during resume.
    pub before_run: Arc<TestGate>,
    /// Pause the supervisor while its link tasks remain alive.
    pub supervisor: Arc<TestGate>,
    /// Pause immediately before the WAL append.
    pub before_append: Arc<TestGate>,
    /// Pause the connected link right after it forwards an action, so a shutdown can be ordered before its next turn.
    pub after_action: Arc<TestGate>,
    /// Pause every durable frame write on the link (a stalled gateway, as seen from the runtime).
    pub before_write: Arc<TestGate>,
    /// Set once the supervisor has told its link to stop, so a test can order that before the link's next turn.
    pub link_stopping: Arc<AtomicBool>,
    /// Pause the link after it has let go of its channels but before its task ends: the moment a supervisor on
    /// another thread can see them closed before it can see how the link ended (RT-I-038).
    pub link_finished: Arc<TestGate>,
    /// Force subsequent journal appends to fail without modifying the file.
    pub fail_appends: Arc<AtomicBool>,
    /// Force WAL synchronization to fail before any event notification is published.
    pub fail_syncs: Arc<AtomicBool>,
    /// Fail one sequenced socket write before delivery; zero disables this fault.
    pub fail_delivery_at: Arc<AtomicU64>,
    /// Largest WAL page handed to a replay caller; observes the buffer bound without counting WAL storage.
    pub replay_page_peak: Arc<AtomicUsize>,
    /// The desktop browser's profile directory instead of `/run/tqa-browser/profile`, so a test can see it emptied.
    pub browser_profile: Arc<std::sync::Mutex<Option<std::path::PathBuf>>>,
    /// The desktop browser's starts after its profile was emptied. With `browser_profile` set, a start only makes the
    /// empty profile directory, as the browser does, so a test needs no browser.
    pub browser_reopened: Arc<AtomicUsize>,
    /// How long such a start takes, in milliseconds: a slow launch, so a test can tell an answer sent after the start
    /// from one sent before it (RT-I-056).
    pub browser_reopen_ms: Arc<AtomicU64>,
    /// Such a start isn't ready for a person's keys (its window never took the focus in time).
    pub browser_not_ready: Arc<AtomicBool>,
}
