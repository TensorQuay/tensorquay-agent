//! Escaped descendants are cleaned only after the foreground shell has been joined.
#[cfg(target_os = "linux")]
mod display;
#[cfg(target_os = "linux")]
mod linux;
use crate::{RuntimeError, RuntimeLimits};
use async_trait::async_trait;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};
use tqa_types::{Clock, Sleeper};

/// What the reaper leaves alone among the runtime's direct children. The process groups the runtime itself spawned for
/// long-lived services (MCP servers) and the runtime's own browser on a desktop, each recorded at spawn by its leader's
/// pid: never a name or a command line, so a process a model starts cannot hide by imitating a server. And, in a
/// desktop session, the clients of its display (ADR-0039 section 10): an application the agent opens stays, as on a
/// person's desktop, told strictly by a socket connected to the display's own (`display.rs`).
#[derive(Clone, Debug, Default)]
pub struct SupervisedGroups(Arc<Mutex<Spared>>);

#[derive(Debug, Default)]
struct Spared {
    groups: BTreeSet<i32>,
    browser: Option<i32>,
    display: Option<PathBuf>,
}

impl SupervisedGroups {
    fn with<T>(&self, use_it: impl FnOnce(&mut Spared) -> T) -> T {
        use_it(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
    /// Records a group the runtime just spawned; its leader's pid is the group id.
    pub fn add(&self, leader: i32) {
        self.with(|spared| spared.groups.insert(leader));
    }
    /// Forgets a group once its service stopped.
    pub fn remove(&self, leader: i32) {
        self.with(|spared| spared.groups.remove(&leader));
    }
    /// Records the runtime's browser, the leader of its own group, or that there is none now.
    pub fn browser(&self, leader: Option<i32>) {
        self.with(|spared| spared.browser = leader);
    }
    /// From now on the clients of `display` (the X server's socket) are left alone: the session is a desktop's.
    pub fn display_clients(&self, display: &Path) {
        self.with(|spared| spared.display = Some(display.to_owned()));
    }
    /// Whose clients are left alone, when the session is a desktop's.
    #[must_use]
    pub fn display(&self) -> Option<PathBuf> {
        self.with(|spared| spared.display.clone())
    }
    /// Whether a direct child is a recorded group leader: a service's or the browser's.
    #[must_use]
    pub fn contains(&self, pid: i32) -> bool {
        self.with(|spared| spared.groups.contains(&pid) || spared.browser == Some(pid))
    }
}

/// Owns cleanup at a point where no shell call is in flight.
#[async_trait]
pub trait ProcessCleanup: Send + Sync {
    /// Kills and reaps escaped descendants within an explicit bound, except the supervised groups.
    ///
    /// # Errors
    /// Reports inspection, signal or cleanup deadline failures.
    async fn reap(&self) -> Result<(), RuntimeError>;
    /// The groups this cleanup leaves alone; a cleanup that reaps nothing needs none.
    fn supervised(&self) -> SupervisedGroups {
        SupervisedGroups::default()
    }
}

/// Linux child-subreaper cleanup; other supported hosts have no process-wide cleanup. The subreaper is a property of
/// the whole process and a reap kills every direct child this reaper did not record, so a process holds one reaper
/// (the runtime does); tests that build several in one process serialise them.
pub struct ProcessReaper {
    #[cfg(target_os = "linux")]
    inner: linux::Reaper,
}
impl ProcessReaper {
    /// Enables subreaper ownership before the runtime can create any subprocess.
    ///
    /// # Errors
    /// Rejects invalid limits or an inability to enable Linux subreaper ownership.
    pub fn new(
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
        limits: RuntimeLimits,
    ) -> Result<Self, RuntimeError> {
        limits.validate()?;
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                inner: linux::Reaper::new(clock, sleeper, limits)?,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (clock, sleeper);
            Ok(Self {})
        }
    }
}
#[async_trait]
impl ProcessCleanup for ProcessReaper {
    async fn reap(&self) -> Result<(), RuntimeError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.reap().await
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(())
        }
    }
    fn supervised(&self) -> SupervisedGroups {
        #[cfg(target_os = "linux")]
        {
            self.inner.supervised.clone()
        }
        #[cfg(not(target_os = "linux"))]
        {
            SupervisedGroups::default()
        }
    }
}
