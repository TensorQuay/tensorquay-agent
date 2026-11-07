//! The per-route request limiter (LLM-U-012): at most `concurrency` requests in flight and `requests_per_minute`
//! starts per rolling minute on one route, served in arrival order. A request waits at most the queue bound and is
//! then refused with a `Retry-After`; the caller never contacts the upstream for a refused request. The relay holds
//! one limiter per route, so the bound covers every conversation and user of that route.

use crate::Limits;
use std::{
    collections::VecDeque,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};

/// The rolling window that `requests_per_minute` counts starts in.
const WINDOW: Duration = Duration::from_mins(1);
/// The `Retry-After` for a request refused while waiting for a concurrency slot.
const CONCURRENCY_RETRY: Duration = Duration::from_secs(1);

/// How long a request may wait for its route, and how many may wait at once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueBounds {
    /// The longest a request waits before it is refused.
    pub wait: Duration,
    /// The most requests waiting on the route at once; one more is refused immediately.
    pub max_waiting: usize,
}

/// Why a request did not get its route; each refusal says when to try again.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LimiterRefusal {
    /// Too many requests are already waiting on this route.
    #[error("the route's request queue is full")]
    QueueFull {
        /// When to try again.
        retry_after: Duration,
    },
    /// The route had no capacity within the wait bound.
    #[error("the route had no capacity within the wait bound")]
    Timeout {
        /// When to try again.
        retry_after: Duration,
    },
    /// The caller gave up while waiting.
    #[error("the request was cancelled while waiting for its route")]
    Cancelled,
}

/// Held while a request is in flight; dropping it frees the concurrency slot.
#[derive(Debug)]
pub struct LimiterPermit {
    _concurrency: Option<OwnedSemaphorePermit>,
}

/// One route's limiter. Without limits it grants every request at once.
pub struct RouteLimiter {
    concurrency: Option<Arc<Semaphore>>,
    window: Option<(usize, Mutex<VecDeque<Instant>>)>,
    bounds: QueueBounds,
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    queued: AtomicUsize,
}

/// Counts one waiting request for as long as it waits, however the wait ends.
struct Waiting<'a>(&'a AtomicUsize);
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl RouteLimiter {
    /// A limiter for a route's declared `concurrency` and `requests_per_minute`.
    #[must_use]
    pub fn new(
        limits: Limits,
        bounds: QueueBounds,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Self {
        let size = |value: u32| usize::try_from(value).unwrap_or(usize::MAX);
        Self {
            concurrency: limits
                .concurrency
                .map(|slots| Arc::new(Semaphore::new(size(slots).min(Semaphore::MAX_PERMITS)))),
            window: limits
                .requests_per_minute
                .map(|limit| (size(limit), Mutex::new(VecDeque::new()))),
            bounds,
            clock,
            sleeper,
            queued: AtomicUsize::new(0),
        }
    }

    /// The requests waiting on this route now, for metrics and tests.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.queued.load(Ordering::Acquire)
    }

    /// Waits, in arrival order, until the route may start one more request.
    ///
    /// # Errors
    /// Refuses when the queue is full, when the route has no capacity within the wait bound, or on cancellation.
    pub async fn acquire(
        &self,
        cancel: &CancellationToken,
    ) -> Result<LimiterPermit, LimiterRefusal> {
        if self.concurrency.is_none() && self.window.is_none() {
            return Ok(LimiterPermit { _concurrency: None });
        }
        let _waiting = self.enter()?;
        let deadline = self.clock.now() + self.bounds.wait;
        let concurrency = match &self.concurrency {
            Some(semaphore) => {
                let acquire = semaphore.clone().acquire_owned();
                let permit = self.within(deadline, cancel, acquire).await?;
                Some(permit.map_err(|_| LimiterRefusal::Cancelled)?)
            }
            None => None,
        };
        if let Some((limit, window)) = &self.window {
            let mut starts = self.within(deadline, cancel, window.lock()).await?;
            self.take_slot(*limit, &mut starts, deadline, cancel)
                .await?;
        }
        Ok(LimiterPermit {
            _concurrency: concurrency,
        })
    }

    /// Starts a request only if the route can take it now, never queueing: for callers that must not wait behind
    /// real traffic, such as the route probe.
    ///
    /// # Errors
    /// Refuses with a `Retry-After` when a concurrency slot or a window slot is not free now.
    pub fn try_acquire(&self) -> Result<LimiterPermit, LimiterRefusal> {
        let busy = |retry_after| LimiterRefusal::QueueFull { retry_after };
        let concurrency = match &self.concurrency {
            Some(semaphore) => Some(
                semaphore
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| busy(CONCURRENCY_RETRY))?,
            ),
            None => None,
        };
        if let Some((limit, window)) = &self.window {
            let mut starts = window.try_lock().map_err(|_| busy(CONCURRENCY_RETRY))?;
            let now = self.clock.now();
            while starts
                .front()
                .is_some_and(|start| now.saturating_duration_since(*start) >= WINDOW)
            {
                starts.pop_front();
            }
            if starts.len() >= *limit {
                let slot = starts.front().map_or(now, |oldest| *oldest + WINDOW);
                return Err(busy(slot.saturating_duration_since(now)));
            }
            starts.push_back(now);
        }
        Ok(LimiterPermit {
            _concurrency: concurrency,
        })
    }

    fn enter(&self) -> Result<Waiting<'_>, LimiterRefusal> {
        let before = self.queued.fetch_add(1, Ordering::AcqRel);
        let waiting = Waiting(&self.queued);
        if before >= self.bounds.max_waiting {
            return Err(LimiterRefusal::QueueFull {
                retry_after: CONCURRENCY_RETRY,
            });
        }
        Ok(waiting)
    }

    /// Records a start in the rolling window, waiting for the oldest start to leave it when the window is full.
    /// A start the window cannot allow before the deadline is refused at once, with the time until it could.
    async fn take_slot(
        &self,
        limit: usize,
        starts: &mut VecDeque<Instant>,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<(), LimiterRefusal> {
        let now = self.clock.now();
        while starts
            .front()
            .is_some_and(|start| now.saturating_duration_since(*start) >= WINDOW)
        {
            starts.pop_front();
        }
        if starts.len() >= limit
            && let Some(oldest) = starts.front()
        {
            let slot = *oldest + WINDOW;
            if slot > deadline {
                return Err(LimiterRefusal::Timeout {
                    retry_after: slot.saturating_duration_since(now),
                });
            }
            tokio::select! {biased;
                () = cancel.cancelled() => return Err(LimiterRefusal::Cancelled),
                () = self.sleeper.sleep(slot.saturating_duration_since(now)) => {}
            }
            starts.pop_front();
        }
        starts.push_back(self.clock.now());
        Ok(())
    }

    /// Runs one wait until the deadline; past it the request is refused, and cancellation ends it at once.
    async fn within<T>(
        &self,
        deadline: Instant,
        cancel: &CancellationToken,
        wait: impl Future<Output = T>,
    ) -> Result<T, LimiterRefusal> {
        let remaining = deadline.saturating_duration_since(self.clock.now());
        tokio::select! {biased;
            () = cancel.cancelled() => Err(LimiterRefusal::Cancelled),
            value = wait => Ok(value),
            () = self.sleeper.sleep(remaining) => Err(LimiterRefusal::Timeout { retry_after: CONCURRENCY_RETRY }),
        }
    }
}
