//! Start-up's named steps: a failed step is logged at ERROR with its name and cause before the gateway exits, and the
//! sandbox backend's connect outlasts a briefly starved engine (GW-U-060, GW-U-071).

use std::{future::Future, time::Duration};

use tqa_sandbox::SandboxError;

use crate::GatewayError;

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;

/// How long start-up keeps trying a backend connect whose engine calls timed out, by default (`[sandbox]
/// startup_budget_seconds`): one full container create (60 s) and a retry fit, and it ends before the gateway image's
/// health check gives up (a 30-second start period plus 12 checks 5 seconds apart, deploy/README.md).
pub(crate) const BACKEND_DEADLINE: Duration = Duration::from_secs(80);

const FIRST_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// The error a failed start-up step exits with, logged at ERROR naming the step and its bounded cause: an exit is
/// never silent. A timeout becomes the step's own `StartupTimeout`.
pub(crate) fn failed(step: &'static str, error: GatewayError) -> GatewayError {
    match error {
        GatewayError::Timeout => {
            tracing::error!(step, "a start-up step timed out");
            GatewayError::StartupTimeout(step)
        }
        other => {
            // Gateway errors are fixed, secret-free phrases.
            tracing::error!(step, cause = %other, "a start-up step failed");
            other
        }
    }
}

/// Runs `attempt` until it does anything but time out, waiting 1, 2, 4, 8 and then 10 s between attempts. Each attempt
/// is given what is left of `deadline` and cuts its own bounds to it, so none runs past it (GW-U-123); a timeout is
/// retried only while the next attempt starts within it; any other error returns at once.
pub(crate) async fn retry_timeouts<T, F, Fut>(
    deadline: Duration,
    mut attempt: F,
) -> Result<T, SandboxError>
where
    F: FnMut(Duration) -> Fut,
    Fut: Future<Output = Result<T, SandboxError>>,
{
    let started = tokio::time::Instant::now();
    let mut backoff = FIRST_BACKOFF;
    let mut attempts: u32 = 1;
    loop {
        match attempt(deadline.saturating_sub(started.elapsed())).await {
            Err(SandboxError::Timeout) if started.elapsed() + backoff < deadline => {
                tracing::warn!(
                    attempts,
                    retry_in_seconds = backoff.as_secs(),
                    "the sandbox backend's engine timed out at start-up; retrying"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                attempts += 1;
            }
            result => return result,
        }
    }
}
