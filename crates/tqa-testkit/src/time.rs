//! Shared manual time; watch notifications avoid polling and lost wakeups.

use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tqa_types::{Clock, Sleeper};

/// A cheap-to-clone manually advanced monotonic clock.
#[derive(Clone, Debug)]
pub struct FakeClock {
    time: watch::Sender<Instant>,
}

impl FakeClock {
    /// Creates a stationary clock with an arbitrary monotonic origin.
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: watch::channel(Instant::now()).0,
        }
    }
    /// Advances shared time and wakes every sleeper whose deadline has elapsed.
    ///
    /// # Errors
    /// Returns `InvalidInput` if the supplied duration exceeds the platform's instant range.
    pub fn advance(&self, duration: Duration) -> io::Result<()> {
        let mut overflow = false;
        self.time.send_modify(|now| {
            if let Some(next) = now.checked_add(duration) {
                *now = next;
            } else {
                overflow = true;
            }
        });
        if overflow {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fake clock overflow",
            ));
        }
        Ok(())
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}
impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.time.borrow()
    }
}

/// Records waits and resolves them only when the shared clock reaches their deadline.
#[derive(Clone, Debug)]
pub struct FakeSleeper {
    clock: FakeClock,
    // Clones share a history so a test can inspect a retry future without taking ownership of it.
    durations: Arc<Mutex<Vec<Duration>>>,
    pending: Arc<Mutex<Vec<Duration>>>,
    /// The names of the pending waits that have one (`Sleeper::sleep_named`).
    named: Arc<Mutex<Vec<&'static str>>>,
}

impl FakeSleeper {
    /// Connects a sleeper to an explicitly supplied fake clock.
    #[must_use]
    pub fn new(clock: FakeClock) -> Self {
        Self {
            clock,
            durations: Arc::default(),
            pending: Arc::default(),
            named: Arc::default(),
        }
    }
    /// Returns requested durations in polling order.
    #[must_use]
    pub fn durations(&self) -> Vec<Duration> {
        self.durations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Returns the current waits, excluding completed or cancelled futures.
    /// A test can wait for transport I/O to settle before advancing model time.
    #[must_use]
    pub fn pending_durations(&self) -> Vec<Duration> {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether a wait with this name is pending.
    #[must_use]
    pub fn pending_named(&self, name: &str) -> bool {
        self.named
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&name)
    }

    async fn wait_named(&self, duration: Duration, name: &'static str) {
        self.named
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name);
        let _named = Named {
            names: &self.named,
            name,
        };
        self.wait_until(duration).await;
    }

    async fn wait_until(&self, duration: Duration) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(duration);
        let _pending = Pending {
            waits: &self.pending,
            duration,
        };
        self.durations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(duration);
        let deadline = self.clock.now().checked_add(duration);
        let mut receiver = self.clock.time.subscribe();
        loop {
            let now = *receiver.borrow_and_update();
            if deadline.is_some_and(|deadline| now >= deadline) {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

struct Pending<'a> {
    waits: &'a Mutex<Vec<Duration>>,
    duration: Duration,
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let mut waits = self.waits.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(index) = waits.iter().position(|duration| *duration == self.duration) {
            waits.swap_remove(index);
        }
    }
}

struct Named<'a> {
    names: &'a Mutex<Vec<&'static str>>,
    name: &'static str,
}
impl Drop for Named<'_> {
    fn drop(&mut self) {
        let mut names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(index) = names.iter().position(|name| *name == self.name) {
            names.swap_remove(index);
        }
    }
}

impl Sleeper for FakeSleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.wait_until(duration))
    }
    fn sleep_named(
        &self,
        duration: Duration,
        name: &'static str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.wait_named(duration, name))
    }
}
