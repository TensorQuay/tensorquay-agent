//! LLM-E-008: retries are bounded, deterministic, capped and cancellable.

use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    task::Poll,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{LlmError, RetryPolicy, TimeoutKind, retry};
use tqa_testkit::{FakeClock, FakeSleeper, FixedJitter};

#[test]
fn llm_e_008_policy_rejects_invalid_inputs() {
    assert!(RetryPolicy::new(0, Duration::from_secs(1), Duration::from_secs(2), true).is_err());
    assert!(RetryPolicy::new(1, Duration::ZERO, Duration::from_secs(2), true).is_err());
    assert!(RetryPolicy::new(1, Duration::from_secs(2), Duration::from_secs(1), true).is_err());
}

#[test]
fn llm_e_008_status_matrix_and_attempt_counts() -> Result<(), Box<dyn Error>> {
    for status in [408, 429, 500, 502, 503, 504, 400, 401, 403, 404] {
        let clock = FakeClock::new();
        let sleeper = FakeSleeper::new(clock.clone());
        let policy = RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(3), true)?;
        let mut jitter = FixedJitter::new(0.5)?;
        let cancel = CancellationToken::new();
        let attempts = Arc::new(AtomicU32::new(0));
        let count = attempts.clone();
        let mut task = tokio_test::task::spawn(retry(
            &policy,
            &clock,
            &sleeper,
            &mut jitter,
            &cancel,
            move || {
                count.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Err::<(), _>(LlmError::Http {
                    status,
                    body_excerpt: "unavailable".into(),
                }))
            },
        ));
        if [408, 429, 500, 502, 503, 504].contains(&status) {
            assert!(task.poll().is_pending());
            assert_eq!(sleeper.durations(), [Duration::from_millis(1500)]);
            clock.advance(Duration::from_millis(1500))?;
            assert!(task.poll().is_pending());
            assert_eq!(
                sleeper.durations(),
                [Duration::from_millis(1500), Duration::from_millis(2250)]
            );
            clock.advance(Duration::from_millis(2250))?;
            assert!(matches!(
                task.poll(),
                Poll::Ready(Err(LlmError::Http { .. }))
            ));
            assert_eq!(attempts.load(Ordering::SeqCst), 3);
        } else {
            assert!(matches!(
                task.poll(),
                Poll::Ready(Err(LlmError::Http { .. }))
            ));
            assert_eq!(attempts.load(Ordering::SeqCst), 1);
            assert!(sleeper.durations().is_empty());
        }
    }
    Ok(())
}

#[test]
fn llm_e_008_retry_after_is_honoured_and_capped() -> Result<(), Box<dyn Error>> {
    for retry_after in [Duration::from_secs(2), Duration::from_secs(90)] {
        let clock = FakeClock::new();
        let sleeper = FakeSleeper::new(clock.clone());
        let policy = RetryPolicy::new(2, Duration::from_secs(1), Duration::from_secs(5), true)?;
        let mut jitter = FixedJitter::new(0.25)?;
        let cancel = CancellationToken::new();
        let mut task = tokio_test::task::spawn(retry(
            &policy,
            &clock,
            &sleeper,
            &mut jitter,
            &cancel,
            || {
                std::future::ready(Err::<(), _>(LlmError::RateLimited {
                    retry_after: Some(retry_after),
                }))
            },
        ));
        assert!(task.poll().is_pending());
        let expected = retry_after.min(Duration::from_secs(5));
        assert_eq!(sleeper.durations(), [expected]);
        clock.advance(expected)?;
        assert!(matches!(
            task.poll(),
            Poll::Ready(Err(LlmError::RateLimited { .. }))
        ));
    }
    Ok(())
}

#[test]
fn llm_e_008_cancellation_interrupts_attempt_and_backoff() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let sleeper = FakeSleeper::new(clock.clone());
    let policy = RetryPolicy::new(2, Duration::from_secs(1), Duration::from_secs(1), false)?;
    let mut jitter = FixedJitter::new(0.0)?;
    let cancel = CancellationToken::new();
    let mut task = tokio_test::task::spawn(retry(
        &policy,
        &clock,
        &sleeper,
        &mut jitter,
        &cancel,
        std::future::pending::<Result<(), LlmError>>,
    ));
    assert!(task.poll().is_pending());
    cancel.cancel();
    assert!(matches!(task.poll(), Poll::Ready(Err(LlmError::Cancelled))));
    drop(task);
    let cancel = CancellationToken::new();
    let mut task = tokio_test::task::spawn(retry(
        &policy,
        &clock,
        &sleeper,
        &mut jitter,
        &cancel,
        || std::future::ready(Err::<(), _>(LlmError::Timeout(TimeoutKind::Idle))),
    ));
    assert!(task.poll().is_pending());
    cancel.cancel();
    assert!(matches!(task.poll(), Poll::Ready(Err(LlmError::Cancelled))));
    Ok(())
}
