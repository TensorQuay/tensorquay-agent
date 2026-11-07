//! Runtime-backed implementations of the shared monotonic time contracts.

use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};
use tqa_types::{Clock, Sleeper};

/// Reads the operating system's monotonic clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Uses the current async runtime's timers; dropping the future cancels the wait.
#[derive(Clone, Copy, Debug, Default)]
pub struct TokioSleeper;
impl Sleeper for TokioSleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}
