//! Registering the built-in environments until the container engine answers (GW-U-143, GW-I-213): an engine that
//! doesn't answer at start (a loaded host, a daemon still starting) used to leave a new install without Workstation
//! until the gateway's next start. The listing is now tried again in the background with a back-off: the first retry
//! after [`FIRST_RETRY`], each next three times as long up to [`MAX_RETRY`], each within a fifth either way so gateways
//! sharing an engine don't retry in step (ENGINEERING-GUARDRAILS §1.4). It stops once the engine answers, at the
//! gateway's shutdown, or when storing fails (as a start's did); its owner is the gateway's task tracker (§1.10). One
//! WARN when the engine first doesn't answer and one line when it answers again, never one per retry.
use std::{future::Future, time::Duration};

use tokio_util::sync::CancellationToken;

/// The wait before the first retry.
pub(crate) const FIRST_RETRY: Duration = Duration::from_secs(5);
/// The longest wait between two retries.
pub(crate) const MAX_RETRY: Duration = Duration::from_mins(2);
/// How much longer each wait is than the one before, until the cap.
const GROWTH: u32 = 3;

/// One listing's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// The engine answered and the built-ins it has are registered.
    Done,
    /// The engine didn't answer: try again later.
    NoAnswer,
    /// Storing the built-ins failed: a later start tries again.
    Failed,
}

/// The wait before retry `attempt` (1 for the first): [`FIRST_RETRY`] times [`GROWTH`] for each earlier retry, capped at
/// [`MAX_RETRY`], then scaled by four fifths to six fifths by `fraction` (in [0, 1); anything else is clamped) and
/// capped again.
pub(crate) fn retry_wait(attempt: u32, fraction: f64) -> Duration {
    let mut wait = FIRST_RETRY;
    for _ in 1..attempt.min(32) {
        wait = wait.saturating_mul(GROWTH).min(MAX_RETRY);
    }
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.5
    };
    wait.mul_f64(0.8 + 0.4 * fraction).min(MAX_RETRY)
}

/// Runs `attempt` until it is [`Attempt::Done`], waiting [`retry_wait`] between two, with `jitter` drawing each wait's
/// fraction. Answers how many listings the engine didn't answer before the one it did, or `None` when the retries ended
/// without an answer (the gateway stopping, or storing failing).
pub(crate) async fn retrying<A, F>(
    cancel: &CancellationToken,
    mut attempt: A,
    mut jitter: impl FnMut() -> f64,
) -> Option<u32>
where
    A: FnMut() -> F,
    F: Future<Output = Attempt>,
{
    let mut unanswered = 0_u32;
    loop {
        match attempt().await {
            Attempt::Done => {
                if unanswered > 0 {
                    tracing::info!(
                        unanswered,
                        "built-in environments refreshed: the container engine answered again"
                    );
                }
                return Some(unanswered);
            }
            Attempt::Failed => return None,
            Attempt::NoAnswer => {
                unanswered = unanswered.saturating_add(1);
                if unanswered == 1 {
                    tracing::warn!(
                        "built-in environments not refreshed: the container engine didn't answer; trying again in \
                         the background until it does"
                    );
                }
            }
        }
        tokio::select! {
            biased;
            () = cancel.cancelled() => return None,
            () = tokio::time::sleep(retry_wait(unanswered, jitter())) => {}
        }
    }
}

#[cfg(test)]
#[path = "builtin_retry_tests.rs"]
mod tests;
