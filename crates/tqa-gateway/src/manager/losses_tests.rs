//! GW-U-072: the replacement policy for a sandbox that keeps stopping unexpectedly.
use super::{Losses, Next, Retry, WINDOW};
use std::time::Duration;

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// Loses the sandbox at `at` and replaces it as soon as the policy allows, returning when it did (`None` if it stopped).
fn lose_and_replace(losses: &mut Losses, at: Duration) -> Option<Duration> {
    let mut now = at;
    loop {
        match losses.next(now) {
            Next::Replace(_) => return Some(now),
            Next::Wait(recovery) => {
                now += secs(u64::from(recovery.retry_in_secs.unwrap_or(1)).max(1));
            }
            Next::Stop(_) => return None,
        }
    }
}

#[test]
fn gw_u_072_the_first_loss_is_replaced_at_once_then_each_waits_longer() {
    let mut losses = Losses::default();
    let first = losses.next(secs(100));
    assert!(
        matches!(first, Next::Replace(r) if r.losses == 1 && r.retry_in_secs == Some(0)),
        "{first:?}"
    );
    losses.replaced(secs(100));
    for (loss, delay) in [(2_u32, 5_u32), (3, 15), (4, 45)] {
        let at = secs(1000 + u64::from(loss));
        // Clamped to the window so the arithmetic stays inside it.
        let at = at.min((secs(100) + WINDOW).saturating_sub(secs(60)));
        let wait = losses.next(at);
        assert!(
            matches!(wait, Next::Wait(r) if r.losses == loss && r.retry_in_secs == Some(delay)),
            "{loss}: {wait:?}"
        );
        // Asking again while it waits counts no new loss and counts the delay down.
        let later = losses.next(at + secs(2));
        assert!(
            matches!(later, Next::Wait(r) if r.losses == loss && r.retry_in_secs == Some(delay - 2)),
            "{later:?}"
        );
        let due = losses.next(at + secs(u64::from(delay)));
        assert!(
            matches!(due, Next::Replace(r) if r.losses == loss),
            "{due:?}"
        );
        losses.replaced(at + secs(u64::from(delay)));
    }
}

#[test]
fn gw_u_072_the_fifth_loss_in_ten_minutes_stops_and_older_losses_age_out() {
    let mut losses = Losses::default();
    let mut now = secs(0);
    for _ in 0..4 {
        let replaced = lose_and_replace(&mut losses, now);
        assert!(replaced.is_some(), "four replacements are allowed");
        now = replaced.unwrap_or(now);
        losses.replaced(now);
        now += secs(1);
    }
    let stop = losses.next(now);
    assert!(
        matches!(stop, Next::Stop(r) if r.losses == 5 && r.retry_in_secs.is_none()),
        "{stop:?}"
    );
    // The same four losses spread over more than the window never stop the run.
    let mut spread = Losses::default();
    let mut at = secs(0);
    for _ in 0..8 {
        let replaced = lose_and_replace(&mut spread, at);
        assert!(
            replaced.is_some(),
            "losses spread past the window stopped at {at:?}"
        );
        at = replaced.unwrap_or(at);
        spread.replaced(at);
        at += secs(200);
    }
}

#[test]
fn gw_u_072_a_new_worker_starts_with_the_full_allowance() {
    let fresh = Losses::default().next(secs(5));
    assert!(
        matches!(fresh, Next::Replace(r) if r.losses == 1),
        "{fresh:?}"
    );
}

// TEST: GW-U-138
#[test]
fn gw_u_138_a_failed_replacement_is_retried_from_the_first_failure_overruns_collapsing() {
    let s = Duration::from_secs;
    let mut losses = Losses::default();
    assert_eq!(losses.create_retry(s(100)), None, "nothing failing");
    // The first failure at 100 s: tries at 105, 120, 165 …, counted from it.
    assert_eq!(losses.create_failed(s(100)), Retry::In(5));
    assert_eq!(losses.create_retry(s(103)), Some(Retry::In(2)));
    assert_eq!(losses.create_retry(s(105)), Some(Retry::Now));
    // That attempt failed at once: the next is due at 120 s.
    assert_eq!(losses.create_failed(s(105)), Retry::In(15));
    assert_eq!(losses.create_retry(s(120)), Some(Retry::Now));
    // An attempt that took until 300 s overran 165 and 255: they collapse into one attempt now, then 435.
    assert_eq!(losses.create_failed(s(300)), Retry::Now);
    assert_eq!(losses.create_retry(s(300)), Some(Retry::In(135)));
    assert_eq!(losses.create_retry(s(435)), Some(Retry::Now));
    assert_eq!(losses.create_failed(s(435)), Retry::In(180));
    assert_eq!(losses.create_retry(s(615)), Some(Retry::Now));
    assert_eq!(losses.create_failed(s(615)), Retry::In(85));
    // The last, at 700 s (600 after the first failure), fails too: no more.
    assert_eq!(losses.create_retry(s(700)), Some(Retry::Now));
    assert_eq!(losses.create_failed(s(700)), Retry::Exhausted);
    // A replacement that started ends the retrying.
    losses.created();
    assert_eq!(losses.create_retry(s(800)), None);
}
