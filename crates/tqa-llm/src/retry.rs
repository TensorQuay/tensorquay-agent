//! Explicit retry policy with injectable jitter, monotonic time and cancellation.

use crate::LlmError;
use rand::Rng as _;
use std::{future::Future, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};

/// Supplies jitter fractions without exposing a random-library type.
pub trait Jitter: Send {
    /// Returns a finite value in the half-open interval [0, 1).
    fn next_fraction(&mut self) -> f64;
}

/// Uses the calling thread's random generator without retaining a thread-bound handle.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadRngJitter;

impl Jitter for ThreadRngJitter {
    fn next_fraction(&mut self) -> f64 {
        rand::rng().random()
    }
}

/// A rejected retry-policy constructor argument.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RetryPolicyError {
    /// At least the initial attempt is required.
    #[error("max_attempts must be at least one")]
    ZeroAttempts,
    /// Back-off cannot have a zero base duration.
    #[error("retry base must be positive")]
    ZeroBase,
    /// The cap must permit the base duration.
    #[error("retry cap must be at least the base")]
    CapBelowBase,
}

/// A validated retry policy; `max_attempts` includes the initial operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    max_attempts: u32,
    base: Duration,
    cap: Duration,
    jitter: bool,
}

impl RetryPolicy {
    /// Constructs an explicit policy; jitter draws between half and all of the exponential delay.
    ///
    /// # Errors
    /// Rejects zero attempts, zero base delay and a cap below the base.
    pub fn new(
        max_attempts: u32,
        base: Duration,
        cap: Duration,
        jitter: bool,
    ) -> Result<Self, RetryPolicyError> {
        if max_attempts == 0 {
            return Err(RetryPolicyError::ZeroAttempts);
        }
        if base.is_zero() {
            return Err(RetryPolicyError::ZeroBase);
        }
        if cap < base {
            return Err(RetryPolicyError::CapBelowBase);
        }
        Ok(Self {
            max_attempts,
            base,
            cap,
            jitter,
        })
    }
    /// Computes the delay after a one-based local attempt, without sleeping or retaining state.
    ///
    /// # Errors
    /// Rejects attempt zero and invalid injected jitter.
    pub fn next_delay(
        &self,
        attempt: u32,
        error: &LlmError,
        jitter: &mut dyn Jitter,
    ) -> Result<Option<Duration>, LlmError> {
        if attempt == 0 {
            return Err(LlmError::Protocol("retry attempt must be positive".into()));
        }
        if attempt >= self.max_attempts || !error.retryable() {
            return Ok(None);
        }
        let mut backoff = self.base;
        for _ in 1..attempt {
            backoff = backoff.saturating_mul(2).min(self.cap);
            if backoff == self.cap {
                break;
            }
        }
        delay(self, backoff, jitter, error).map(Some)
    }
}

fn delay(
    policy: &RetryPolicy,
    backoff: Duration,
    jitter: &mut dyn Jitter,
    error: &LlmError,
) -> Result<Duration, LlmError> {
    if let LlmError::RateLimited {
        retry_after: Some(duration),
    }
    | LlmError::Unavailable {
        retry_after: Some(duration),
    }
    | LlmError::GatewayRefused {
        retry_after: Some(duration),
        ..
    } = error
    {
        return Ok((*duration).min(policy.cap));
    }
    // The relay's own throttle always carries its wait.
    if let LlmError::Throttled { retry_after, .. } = error {
        return Ok((*retry_after).min(policy.cap));
    }
    if !policy.jitter {
        return Ok(backoff);
    }
    let fraction = jitter.next_fraction();
    if !(0.0..1.0).contains(&fraction) {
        return Err(LlmError::Protocol(
            "jitter must be finite and within [0, 1)".into(),
        ));
    }
    Ok(backoff.mul_f64(f64::midpoint(1.0, fraction)))
}

/// Retries transient failures under an explicit policy, including cancellation while sleeping.
///
/// # Errors
/// Returns the final operation error, invalid jitter, or Cancelled; partial streams are never replayed here.
#[allow(
    clippy::too_many_arguments,
    reason = "The public retry contract injects policy, time, jitter, cancellation and operation separately."
)]
pub async fn retry<T, F, Fut>(
    policy: &RetryPolicy,
    clock: &dyn Clock,
    sleeper: &dyn Sleeper,
    jitter: &mut dyn Jitter,
    cancel: &CancellationToken,
    mut op: F,
) -> Result<T, LlmError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, LlmError>>,
{
    let mut attempt = 1;
    loop {
        if cancel.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        let started = clock.now();
        tracing::debug!(attempt, "completion attempt");
        let result = tokio::select! { biased; () = cancel.cancelled() => return Err(LlmError::Cancelled), result = op() => result };
        let error = match result {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        tracing::debug!(attempt, code = error.code(), elapsed = ?clock.now().saturating_duration_since(started), "completion attempt failed");
        let Some(duration) = policy.next_delay(attempt, &error, jitter)? else {
            return Err(error);
        };
        tracing::debug!(attempt, delay = ?duration, "completion retry waiting");
        tokio::select! { biased; () = cancel.cancelled() => return Err(LlmError::Cancelled), () = sleeper.sleep(duration) => {} }
        attempt += 1;
    }
}
