use std::time::Duration;

/// Monotonic time for sandbox idle policy, independent of database and socket I/O deadlines.
#[async_trait::async_trait]
pub trait IdleClock: Send + Sync {
    /// Returns a monotonically increasing duration from an arbitrary fixed epoch.
    fn now(&self) -> Duration;

    /// Wakes policy polling after an externally controlled clock advances.
    /// The default clock is polled by the manager's timer; shutdown cancels this wait.
    async fn changed(&self) {
        std::future::pending::<()>().await;
    }
}

pub(crate) struct SystemClock(tokio::time::Instant);

impl SystemClock {
    pub fn new() -> Self {
        Self(tokio::time::Instant::now())
    }
}

impl IdleClock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}
