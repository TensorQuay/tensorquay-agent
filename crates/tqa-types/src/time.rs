//! Runtime-independent time boundaries for deterministic deadlines and retries.

use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};

/// Supplies monotonic time without requiring a particular async runtime.
pub trait Clock: Send + Sync {
    /// Returns the current monotonic instant.
    fn now(&self) -> Instant;
}

/// Creates cancellable waits; dropping a returned future abandons that wait.
pub trait Sleeper: Send + Sync {
    /// Resolves after the supplied duration according to this sleeper's clock.
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// As [`Self::sleep`], naming the wait so a test's fake clock can tell it from the others; a real sleeper ignores
    /// the name.
    fn sleep_named(
        &self,
        duration: Duration,
        _name: &'static str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.sleep(duration)
    }
}
