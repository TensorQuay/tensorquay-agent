//! An engine call retried while the container engine is too busy to answer in time, inside one create budget: a VM
//! host's create-and-start (SBX-F-028), a new image's helper (SBX-F-030) and a Docker sandbox's container
//! (SBX-U-014). The budget bounds a whole start, so a person waits at most that long before a start fails
//! (SBX-F-031).

use std::time::{Duration, SystemTime};

use tokio::time::Instant;

use crate::SandboxError;

/// Attempts in all for one engine call, the first included. Fixed, not configured: the bound a person feels is the
/// create budget (`[sandbox] create_budget_seconds`), which cuts every attempt and wait; more attempts than fit in it
/// never start, so a setting here would add a knob without changing that bound (documented in the gateway README and
/// the administrator's guide).
const ATTEMPTS: u32 = 3;
/// The first wait after a timeout; each next wait doubles it (1 s, then 2 s, each plus up to a quarter as jitter).
/// Fixed for the same reason as [`ATTEMPTS`]: short against a create's own bound (60 s), so the budget, not the
/// waits, decides how long a start may take.
const FIRST_BACKOFF: Duration = Duration::from_secs(1);
/// The default create budget: what a person waits, at most, for the engine work of one start (`[sandbox]
/// create_budget_seconds`).
pub const DEFAULT_CREATE_BUDGET: Duration = Duration::from_mins(2);

/// The time one start's retried engine calls share, from the first of them. Every attempt's own bound is cut to what is
/// left, and no attempt or back-off starts once it is spent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Budget {
    deadline: Instant,
}

impl Budget {
    /// A budget of `total` from now.
    pub(crate) fn new(total: Duration) -> Self {
        Self {
            deadline: Instant::now() + total,
        }
    }

    /// What is left of it.
    pub(crate) fn left(self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

/// How an engine call is retried when the engine times out (a loaded host): a bounded number of attempts with
/// exponential back-off and jitter (ENGINEERING-GUARDRAILS §1.4), all inside one [`Budget`]. Only a timeout is
/// retried, and each attempt must be safe to repeat (a timed-out create removes, or collides with, what the engine made
/// late); any other error returns at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Retry {
    attempts: u32,
    first_backoff: Duration,
}

impl Default for Retry {
    fn default() -> Self {
        Self::new(ATTEMPTS, FIRST_BACKOFF)
    }
}

impl Retry {
    /// `attempts` in all (at least one), the first wait `first_backoff`.
    pub(crate) fn new(attempts: u32, first_backoff: Duration) -> Self {
        Self {
            attempts: attempts.max(1),
            first_backoff,
        }
    }

    /// Runs `attempt` until it does anything but time out, the attempts are spent or `budget` is; the last timeout is
    /// returned. Each attempt gets its number (from 1) and its bound: `per_call`, cut to what is left of `budget`.
    /// `doing` names the call in the warning ("starting a VM host"), so a busy host's log says which run was slow.
    pub(crate) async fn run<T, F, Fut>(
        self,
        doing: &'static str,
        budget: Budget,
        per_call: Duration,
        mut attempt: F,
    ) -> Result<T, SandboxError>
    where
        F: FnMut(u32, Duration) -> Fut,
        Fut: Future<Output = Result<T, SandboxError>>,
    {
        let mut backoff = self.first_backoff;
        let mut number = 1;
        loop {
            let bound = per_call.min(budget.left());
            if bound.is_zero() {
                return Err(SandboxError::Timeout);
            }
            let result = attempt(number, bound).await;
            if !matches!(result, Err(SandboxError::Timeout)) || number >= self.attempts {
                return result;
            }
            let wait = backoff + jitter(backoff);
            // A wait that would spend the rest of the budget leaves no time for the attempt after it.
            if wait >= budget.left() {
                return result;
            }
            tracing::warn!(
                attempts_left = self.attempts - number,
                retry_in_ms = wait.as_millis(),
                budget_left_ms = budget.left().as_millis(),
                "the container engine timed out {doing}; retrying"
            );
            tokio::time::sleep(wait).await;
            backoff = backoff.saturating_mul(2);
            number += 1;
        }
    }
}

/// Up to a quarter of `backoff`, so calls retried together don't hit the engine at once.
fn jitter(backoff: Duration) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    backoff.mul_f64(f64::from(nanos % 1000) / 4000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sbx_f_028_jitter_stays_within_a_quarter() {
        for millis in [1, 1000, 4000] {
            let backoff = Duration::from_millis(millis);
            assert!(jitter(backoff) <= backoff / 4);
        }
    }

    /// An engine refusal that is not a timeout.
    fn refused() -> std::future::Ready<Result<(), SandboxError>> {
        std::future::ready(Err(SandboxError::Backend("start failed".into())))
    }

    #[tokio::test]
    async fn sbx_f_028_only_a_timeout_is_retried() {
        let mut calls = 0;
        let result = Retry::new(3, Duration::from_millis(1))
            .run(
                "starting a VM host",
                Budget::new(Duration::from_mins(1)),
                Duration::from_secs(10),
                |_, _| {
                    calls += 1;
                    refused()
                },
            )
            .await;
        assert_eq!(
            (result, calls),
            (Err(SandboxError::Backend("start failed".into())), 1)
        );
    }

    #[test]
    fn sbx_f_028_at_least_one_attempt() {
        assert_eq!(Retry::new(0, Duration::ZERO), Retry::new(1, Duration::ZERO));
    }

    /// Runs a retry whose every attempt takes its whole bound and times out, as a busy engine's call does, on paused
    /// time; returns each attempt's (number, bound), the result and the time it took.
    async fn timing_out(
        per_call: Duration,
        budget: Duration,
    ) -> (Vec<(u32, Duration)>, Result<(), SandboxError>, Duration) {
        let started = Instant::now();
        let mut attempts = Vec::new();
        let result = Retry::default()
            .run(
                "creating a sandbox container",
                Budget::new(budget),
                per_call,
                |number, bound| {
                    attempts.push((number, bound));
                    async move {
                        tokio::time::sleep(bound).await;
                        Err::<(), _>(SandboxError::Timeout)
                    }
                },
            )
            .await;
        (attempts, result, started.elapsed())
    }

    // TEST: SBX-F-031
    #[tokio::test(start_paused = true)]
    async fn sbx_f_031_attempts_share_the_budget_and_the_last_is_cut_to_what_is_left() {
        let (attempts, result, took) =
            timing_out(Duration::from_mins(1), Duration::from_mins(2)).await;
        assert_eq!(result, Err(SandboxError::Timeout));
        // 60 s, a 1-1.25 s wait, then what is left of 120 s; no third attempt fits after the next wait.
        assert_eq!(attempts.len(), 2, "{attempts:?}");
        assert_eq!(attempts[0], (1, Duration::from_mins(1)));
        assert!(
            attempts[1].1 < Duration::from_mins(1)
                && attempts[1].1 >= Duration::from_millis(58_750),
            "{attempts:?}"
        );
        assert!(took <= Duration::from_mins(2), "{took:?}");
    }

    // TEST: SBX-F-031
    #[tokio::test(start_paused = true)]
    async fn sbx_f_031_a_short_per_call_bound_keeps_its_three_attempts_inside_the_budget() {
        let (attempts, result, took) =
            timing_out(Duration::from_secs(10), Duration::from_mins(2)).await;
        assert_eq!(result, Err(SandboxError::Timeout));
        assert_eq!(
            attempts.iter().map(|(_, bound)| *bound).collect::<Vec<_>>(),
            vec![Duration::from_secs(10); 3],
            "the budget doesn't cut a bound it has room for"
        );
        assert!(took < Duration::from_secs(40), "{took:?}");
    }

    // TEST: SBX-F-031
    #[tokio::test(start_paused = true)]
    async fn sbx_f_031_a_spent_budget_starts_no_attempt() {
        let (attempts, result, _) = timing_out(Duration::from_secs(10), Duration::ZERO).await;
        assert_eq!(result, Err(SandboxError::Timeout));
        assert!(attempts.is_empty(), "{attempts:?}");
    }
}
