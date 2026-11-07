//! A conversation's sandbox that keeps stopping unexpectedly (a full disk makes its runtime exit on every write) is
//! replaced with a growing delay, and past a limit not at all: 0, 5, 15 and 45 seconds before the first to fourth
//! replacement within 10 minutes; a fifth loss in that window retires the sandbox as failed until the owner acts. The
//! run stays unfinished, so the owner's next message resumes it. Each worker starts with the full allowance.
use std::{collections::VecDeque, time::Duration};
use tqa_types::SandboxRecovery;

/// The rolling window losses are counted in.
pub(super) const WINDOW: Duration = Duration::from_mins(10);
/// The delay before the first, second, third and fourth replacement in the window; one more loss stops.
const DELAYS: [Duration; 4] = [
    Duration::from_secs(0),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(45),
];

/// When a replacement whose create failed is tried again, counted from its first failure: 5, 20, 65, 155, 335, 515 and
/// 600 s (the full-disk P0 of 3 Oct 2026: a disk full for 100 s outlasted the loss schedule's 65 s). Times an attempt
/// overran collapse into one attempt at once; after the last, the replacing stops until the owner acts.
const CREATE_RETRIES: [Duration; 7] = [
    Duration::from_secs(5),
    Duration::from_secs(20),
    Duration::from_secs(65),
    Duration::from_secs(155),
    Duration::from_secs(335),
    Duration::from_secs(515),
    Duration::from_mins(10),
];

/// A replacement's create that failed: when it first failed, and the next of [`CREATE_RETRIES`] not yet passed.
#[derive(Debug, Clone, Copy)]
struct Failing {
    first: Duration,
    next: usize,
}

/// When to try a failed replacement again.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Retry {
    /// Not yet: in this many seconds (rounded up).
    In(u32),
    /// Now.
    Now,
    /// Never again: every retry failed.
    Exhausted,
}

/// What to do about a missing sandbox now.
#[derive(Debug)]
pub(super) enum Next {
    /// Replace it now.
    Replace(SandboxRecovery),
    /// Leave it missing for now; the replacement is due later.
    Wait(SandboxRecovery),
    /// Do not replace it: retire it as failed.
    Stop(SandboxRecovery),
}

/// One worker's recent replacements, on the manager's idle clock.
#[derive(Debug, Default)]
pub(super) struct Losses {
    replaced: VecDeque<Duration>,
    /// The loss being handled: when its replacement is due, and its count in the window.
    pending: Option<(Duration, u32)>,
    /// The replacement whose create is failing, while it is retried.
    failing: Option<Failing>,
}

impl Losses {
    /// The decision for a sandbox found missing at `now`. The first call for a loss counts it; later calls before the
    /// replacement count it down.
    pub(super) fn next(&mut self, now: Duration) -> Next {
        if let Some((due, losses)) = self.pending {
            return if now >= due {
                Next::Replace(recovery(losses, Some(0)))
            } else {
                Next::Wait(recovery(losses, Some(seconds(due.saturating_sub(now)))))
            };
        }
        while self
            .replaced
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= WINDOW)
        {
            self.replaced.pop_front();
        }
        let losses = u32::try_from(self.replaced.len() + 1).unwrap_or(u32::MAX);
        let Some(delay) = DELAYS.get(self.replaced.len()) else {
            return Next::Stop(recovery(losses, None));
        };
        self.pending = Some((now + *delay, losses));
        if delay.is_zero() {
            Next::Replace(recovery(losses, Some(0)))
        } else {
            Next::Wait(recovery(losses, Some(seconds(*delay))))
        }
    }

    /// Whether a loss is being handled (counted, its replacement not yet started).
    pub(super) const fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The replacement for the pending loss started at `now`.
    pub(super) fn replaced(&mut self, now: Duration) {
        self.pending = None;
        self.replaced.push_back(now);
    }

    /// The losses counted in the window, for the notices: the replacements started.
    pub(super) fn count(&self) -> u32 {
        u32::try_from(self.replaced.len()).unwrap_or(u32::MAX)
    }

    /// The replacement's create failed at `now`: when to try again. The first failure starts the schedule; every
    /// time an attempt overran collapses into one attempt now.
    pub(super) fn create_failed(&mut self, now: Duration) -> Retry {
        let failing = self.failing.get_or_insert(Failing {
            first: now,
            next: 0,
        });
        let mut passed = false;
        while CREATE_RETRIES
            .get(failing.next)
            .is_some_and(|at| failing.first + *at <= now)
        {
            failing.next += 1;
            passed = true;
        }
        if passed {
            return Retry::Now;
        }
        match CREATE_RETRIES.get(failing.next) {
            Some(at) => Retry::In(seconds((failing.first + *at).saturating_sub(now))),
            None => Retry::Exhausted,
        }
    }

    /// Whether a failed replacement is being retried, and if so when: `None` when none is.
    pub(super) fn create_retry(&mut self, now: Duration) -> Option<Retry> {
        let failing = self.failing.as_mut()?;
        let Some(at) = CREATE_RETRIES.get(failing.next) else {
            return Some(Retry::Exhausted);
        };
        let due = failing.first + *at;
        if now >= due {
            // This attempt takes the due time: the next failure counts from the one after it.
            failing.next += 1;
            Some(Retry::Now)
        } else {
            Some(Retry::In(seconds(due.saturating_sub(now))))
        }
    }

    /// The replacement started: its creates no longer fail.
    pub(super) fn created(&mut self) {
        self.failing = None;
    }
}

const fn recovery(losses: u32, retry_in_secs: Option<u32>) -> SandboxRecovery {
    SandboxRecovery {
        losses,
        retry_in_secs,
    }
}

/// Whole seconds, rounded up, so a client never shows 0 while it still waits.
fn seconds(duration: Duration) -> u32 {
    let whole = duration.as_secs() + u64::from(duration.subsec_nanos() > 0);
    u32::try_from(whole).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "losses_tests.rs"]
mod tests;
