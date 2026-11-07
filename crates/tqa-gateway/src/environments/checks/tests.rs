//! GW-I-077 (unit): the retry backoff after the engine could not answer.
use super::{MAX_RETRY, MIN_RETRY, next_retry};
use std::time::Duration;

// TEST: GW-I-026
#[test]
fn gw_i_077_retries_start_at_30_s_double_and_stop_at_10_min() {
    assert_eq!(MIN_RETRY, Duration::from_secs(30));
    assert_eq!(MAX_RETRY, Duration::from_mins(10));
    let mut waits = Vec::new();
    let mut retry = None;
    for _ in 0..7 {
        let next = next_retry(retry);
        waits.push(next.as_secs());
        retry = Some(next);
    }
    assert_eq!(waits, [30, 60, 120, 240, 480, 600, 600]);
}
