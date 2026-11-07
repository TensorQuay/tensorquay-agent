//! GW-U-043: the per-user connector rate (ADR-0031, `mcp.calls_per_minute`).
use std::time::Duration;

use tokio::time::Instant;

use super::{MAX_USERS, Rate};

fn user(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

#[test]
fn gw_u_043_a_user_may_send_a_minutes_worth_at_once_then_at_the_refill_rate() {
    let start = Instant::now();
    let mut rate = Rate::default();
    for _ in 0..60 {
        assert_eq!(rate.admit(user(1), 1, 60, start), Ok(()));
    }
    assert_eq!(
        rate.admit(user(1), 1, 60, start),
        Err(1),
        "empty: one a second"
    );
    assert_eq!(
        rate.admit(user(2), 1, 60, start),
        Ok(()),
        "users are independent"
    );
    let later = start + Duration::from_secs(1);
    assert_eq!(rate.admit(user(1), 1, 60, later), Ok(()));
    assert_eq!(rate.admit(user(1), 1, 60, later), Err(1));
    assert_eq!(
        rate.admit(user(1), 5, 60, later),
        Err(5),
        "the wait covers the whole cost"
    );
}

#[test]
fn gw_u_043_each_message_of_a_batch_costs_one_and_a_huge_batch_needs_a_full_bucket() {
    let start = Instant::now();
    let mut rate = Rate::default();
    assert_eq!(rate.admit(user(1), 8, 10, start), Ok(()));
    assert_eq!(rate.admit(user(1), 3, 10, start), Err(6), "two left");
    assert_eq!(rate.admit(user(1), 2, 10, start), Ok(()));
    let full = start + Duration::from_mins(1);
    assert_eq!(
        rate.admit(user(1), 32, 10, full),
        Ok(()),
        "a batch larger than the bucket passes from a full one"
    );
    assert_eq!(rate.admit(user(1), 32, 10, full), Err(60));
}

#[test]
fn gw_u_043_a_changed_limit_applies_at_the_next_call() {
    let start = Instant::now();
    let mut rate = Rate::default();
    assert_eq!(rate.admit(user(1), 10, 60, start), Ok(()));
    // Lowered to 5: the 50 credits left are capped at the new bucket at once.
    assert_eq!(rate.admit(user(1), 5, 5, start), Ok(()));
    assert_eq!(rate.admit(user(1), 1, 5, start), Err(12));
    // Raised to 600: ten a second refill from the next call.
    let later = start + Duration::from_secs(1);
    assert_eq!(rate.admit(user(1), 10, 600, later), Ok(()));
    assert_eq!(rate.admit(user(1), 1, 600, later), Err(1));
}

#[test]
fn gw_u_043_buckets_never_grow_past_the_user_bound() {
    let start = Instant::now();
    let mut rate = Rate::default();
    for n in 0..u128::try_from(MAX_USERS).unwrap_or(0) {
        assert_eq!(rate.admit(user(n), 1, 60, start), Ok(()));
    }
    assert_eq!(
        rate.admit(user(u128::MAX), 1, 60, start),
        Err(1),
        "every bucket is in use"
    );
    assert_eq!(rate.users(), MAX_USERS);
    // A second later every bucket is full again: carrying nothing, they are forgotten to make room.
    let later = start + Duration::from_secs(1);
    assert_eq!(rate.admit(user(u128::MAX), 1, 60, later), Ok(()));
    assert_eq!(rate.users(), 1);
}
