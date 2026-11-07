//! GW-U-202: an eviction counts as in flight, holding back every other eviction, only while it is really under way.
use super::{EVICTION_BOUND, still_evicting};
use std::time::Duration;

// TEST: GW-U-202
#[test]
fn gw_u_202_an_eviction_is_in_flight_only_while_its_victim_is_being_retired() {
    let soon = Duration::from_secs(1);
    assert!(
        still_evicting(Some(false), soon),
        "its slot there, its destroy not failed: in flight"
    );
    assert!(!still_evicting(None, soon), "its slot released: done");
    assert!(
        !still_evicting(Some(true), soon),
        "its destroy failed (unconfirmed, retried in the background): the next pass picks another victim"
    );
    assert!(
        !still_evicting(Some(false), EVICTION_BOUND),
        "past the bound (the slot reused, or the retire never reported): no longer holds the others back"
    );
    let just_under = EVICTION_BOUND.saturating_sub(Duration::from_millis(1));
    assert!(still_evicting(Some(false), just_under));
}
