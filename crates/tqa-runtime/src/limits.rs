//! Explicit process bounds, injectable for tests without adding environment configuration.
use crate::RuntimeError;
use std::time::Duration;

/// Bounded queue, history and I/O settings for one runtime process.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeLimits {
    /// Entries in action, control and journal-command queues.
    pub queue_capacity: usize,
    /// Requested gateway history page length.
    pub history_page: usize,
    /// Maximum encoded HTTP history page bytes.
    pub history_page_bytes: usize,
    /// Maximum encoded frames retained in the supervisor's history.
    pub history_bytes: usize,
    /// Maximum frames retained in the supervisor's history.
    pub history_frames: usize,
    /// Bound for a link, configuration, history or WAL acknowledgement operation.
    pub io_timeout: Duration,
    /// Initial registration deadline, including token binding retries.
    pub start_timeout: Duration,
    /// Supervisor heartbeat cadence while a run is active.
    pub heartbeat_interval: Duration,
    /// Full-jitter reconnect base.
    pub reconnect_base: Duration,
    /// Full-jitter reconnect ceiling.
    pub reconnect_cap: Duration,
    /// Bound for owned shutdown cleanup, inside the container's stop timeout.
    pub shutdown_timeout: Duration,
    /// Bound for escaped-child cleanup.
    pub reap_timeout: Duration,
    /// Maximum child processes inspected per cleanup pass.
    pub max_children: usize,
    /// Bound for the workspace flush before a tool result is journaled (`sync_before_result`), outside the append's
    /// `io_timeout`: a result whose flush does not finish in time is not journaled.
    pub workspace_sync_timeout: Duration,
    /// The most one step of a run waits for this link to come back after a model attempt failed while it was down
    /// (CORE-U-062); the run's own wall clock bounds it too.
    pub link_wait: Duration,
}

/// The default [`RuntimeLimits::workspace_sync_timeout`]. Well above `io_timeout`, because a flush after a large tool
/// output (an install, a build tree) writes that output back first; PERF-013 records the flush time after a large write
/// on the reference host, and this bound must stay above its p95.
pub const WORKSPACE_SYNC_TIMEOUT: Duration = Duration::from_mins(1);
impl RuntimeLimits {
    /// Approved queue/history/time defaults for the runtime image.
    pub const RECOMMENDED: Self = Self {
        queue_capacity: 64,
        history_page: 100,
        history_page_bytes: 32 * 1024 * 1024,
        history_bytes: 256 * 1024 * 1024,
        history_frames: 1_000_000,
        io_timeout: Duration::from_secs(5),
        start_timeout: Duration::from_mins(1),
        heartbeat_interval: Duration::from_secs(30),
        reconnect_base: Duration::from_millis(250),
        reconnect_cap: Duration::from_secs(5),
        shutdown_timeout: Duration::from_secs(8),
        reap_timeout: Duration::from_secs(1),
        max_children: 4096,
        link_wait: Duration::from_mins(2),
        workspace_sync_timeout: WORKSPACE_SYNC_TIMEOUT,
    };
    /// Rejects zero or internally inconsistent process bounds.
    ///
    /// # Errors
    /// Returns a configuration error before creating tasks or allocating queues.
    pub fn validate(self) -> Result<(), RuntimeError> {
        if [
            self.queue_capacity,
            self.history_page,
            self.history_page_bytes,
            self.history_bytes,
            self.history_frames,
            self.max_children,
        ]
        .contains(&0)
            || [
                self.io_timeout,
                self.start_timeout,
                self.heartbeat_interval,
                self.reconnect_base,
                self.reconnect_cap,
                self.shutdown_timeout,
                self.reap_timeout,
                self.workspace_sync_timeout,
                self.link_wait,
            ]
            .iter()
            .any(Duration::is_zero)
            || self.reconnect_base > self.reconnect_cap
            || self.history_page > self.history_frames
            || self.history_page_bytes > self.history_bytes
        {
            return Err(RuntimeError::Configuration("runtime limits"));
        }
        Ok(())
    }
}
