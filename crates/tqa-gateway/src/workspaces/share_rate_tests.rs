//! GW-I-049: the 11th share-by-email attempt in a minute is refused; other callers and a later window are not.
use super::*;

#[test]
fn gw_i_049_the_eleventh_attempt_in_the_window_is_refused() {
    let rate = ShareRate::new();
    let (owner, other) = (Uuid::now_v7(), Uuid::now_v7());
    let start = Instant::now();
    for _ in 0..ATTEMPTS {
        assert!(rate.check(owner, start).is_ok());
    }
    assert!(matches!(rate.check(owner, start), Err(GatewayError::Busy)));
    assert!(rate.check(other, start).is_ok());
    assert!(rate.check(owner, start + WINDOW).is_ok());
}
