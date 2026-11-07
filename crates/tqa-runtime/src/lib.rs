//! The process that runs the agent loop inside a container or microVM sandbox, with an
//! outbound WebSocket gateway link, protocol handling, run supervision, activeness reporting
//! and health checks.

mod channels;
mod commands;
mod configuration;
mod decision_relay;
mod desktop;
mod error;
mod history;
mod journal;
mod limits;
mod link;
pub mod mcp;
mod process_cleanup;
mod provider;
mod run_services;
mod runtime;
mod screen_bridge;
mod screen_store;
mod session;
mod shell;
mod supervisor;
mod teams;
mod time;
mod wall_clock;
mod workspace;
mod workspace_service;

#[cfg(feature = "testing")]
pub mod testing;

pub use configuration::{RuntimeConfig, RuntimeInputs, sync_before_result};
pub use decision_relay::{RelayDecisionEngine, RelayEngineConfig};
pub use error::{IoCause, RuntimeError};
pub use history::merge_history;
pub use journal::{Journal, JournalOptions, JournalSession, SyncFs, WorkspaceSync};
pub use limits::RuntimeLimits;
pub use process_cleanup::{ProcessCleanup, ProcessReaper, SupervisedGroups};
pub use runtime::{Runtime, RuntimeDeps, RuntimeExit};
pub use wall_clock::{SystemWallClock, WallClock};

/// Returns this crate's package name.
#[must_use]
pub fn crate_name() -> &'static str {
    "tqa-runtime"
}

#[cfg(test)]
mod tests {
    #[test]
    fn returns_crate_name() {
        assert_eq!(super::crate_name(), "tqa-runtime");
    }
}
