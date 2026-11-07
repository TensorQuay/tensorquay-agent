//! LLM-U-010: pure retry decisions support event emission between attempts.
use std::{error::Error, time::Duration};
use tqa_llm::{LlmError, RetryPolicy};
use tqa_testkit::FixedJitter;

#[test]
fn llm_u_010_next_delay_retries_only_transient_errors_within_attempt_budget()
-> Result<(), Box<dyn Error>> {
    let policy = RetryPolicy::new(3, Duration::from_secs(1), Duration::from_secs(30), true)?;
    let mut jitter = FixedJitter::new(0.5)?;
    let error = LlmError::Http {
        status: 503,
        body_excerpt: "not logged".into(),
    };
    assert_eq!(
        policy.next_delay(1, &error, &mut jitter)?,
        Some(Duration::from_millis(750))
    );
    assert_eq!(
        policy.next_delay(2, &error, &mut jitter)?,
        Some(Duration::from_millis(1500))
    );
    assert_eq!(policy.next_delay(3, &error, &mut jitter)?, None);
    assert_eq!(policy.next_delay(4, &error, &mut jitter)?, None);
    assert!(policy.next_delay(0, &error, &mut jitter).is_err());
    for status in [400, 401, 404] {
        let error = LlmError::Http {
            status,
            body_excerpt: String::new(),
        };
        assert_eq!(policy.next_delay(1, &error, &mut jitter)?, None);
    }
    Ok(())
}

#[test]
fn llm_e_008_equal_jitter_preserves_a_restart_window_at_every_fraction()
-> Result<(), Box<dyn Error>> {
    let policy = RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(30), true)?;
    let error = LlmError::Http {
        status: 503,
        body_excerpt: String::new(),
    };
    for (fraction, expected) in [
        (0.0, Duration::from_secs(1)),
        (0.5, Duration::from_millis(1500)),
        (1.0 - f64::EPSILON, Duration::from_secs(2)),
    ] {
        let mut jitter = FixedJitter::new(fraction)?;
        let first = policy
            .next_delay(1, &error, &mut jitter)?
            .ok_or("missing first retry")?;
        let second = policy
            .next_delay(2, &error, &mut jitter)?
            .ok_or("missing second retry")?;
        assert!((Duration::from_secs(1)..=Duration::from_secs(2)).contains(&first));
        assert!((Duration::from_secs(2)..=Duration::from_secs(4)).contains(&second));
        assert!((Duration::from_secs(3)..=Duration::from_secs(6)).contains(&(first + second)));
        assert_eq!(policy.next_delay(3, &error, &mut jitter)?, None);
        assert_eq!(first, expected);
    }
    Ok(())
}

#[test]
fn llm_u_010_next_delay_honors_retry_after_and_saturates() -> Result<(), Box<dyn Error>> {
    let policy = RetryPolicy::new(100, Duration::from_secs(1), Duration::from_secs(30), false)?;
    let mut jitter = FixedJitter::new(0.5)?;
    for (requested, expected) in [(0, 0), (5, 5), (60, 30)] {
        let error = LlmError::RateLimited {
            retry_after: Some(Duration::from_secs(requested)),
        };
        assert_eq!(
            policy.next_delay(1, &error, &mut jitter)?,
            Some(Duration::from_secs(expected))
        );
    }
    assert_eq!(
        policy.next_delay(
            99,
            &LlmError::RateLimited { retry_after: None },
            &mut jitter
        )?,
        Some(Duration::from_secs(30))
    );
    Ok(())
}

#[test]
fn llm_u_032_a_relay_throttle_delay_is_its_retry_after_capped_by_the_policy()
-> Result<(), Box<dyn Error>> {
    // Any caller retrying through the policy waits what the relay asked, as for a provider's 429, never a backoff.
    let policy = RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(30), true)?;
    let mut jitter = FixedJitter::new(0.5)?;
    for scope in [tqa_llm::ThrottleScope::User, tqa_llm::ThrottleScope::Route] {
        for (requested, expected) in [(1, 1), (5, 5), (90, 30)] {
            let error = LlmError::Throttled {
                retry_after: Duration::from_secs(requested),
                scope,
            };
            assert_eq!(
                policy.next_delay(1, &error, &mut jitter)?,
                Some(Duration::from_secs(expected)),
                "{scope:?} {requested}s"
            );
        }
    }
    Ok(())
}
