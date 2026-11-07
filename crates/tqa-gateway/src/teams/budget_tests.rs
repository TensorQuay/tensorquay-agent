//! GW-I-107 (part): the 80 % line, crossed once.
use super::crossed;

// TEST: GW-I-107
#[test]
fn gw_i_107_only_the_spend_that_crosses_80_percent_tells_the_lead() {
    assert!(!crossed(0, 79, 100, 80), "under the line");
    assert!(crossed(79, 80, 100, 80), "reaching it exactly");
    assert!(crossed(40, 120, 100, 80), "one spend past both lines");
    assert!(!crossed(80, 90, 100, 80), "already past: no second notice");
    assert!(crossed(0, 2_000_000, 2_000_000, 80));
    assert!(
        !crossed(i64::MAX - 1, i64::MAX, i64::MAX / 2, 80),
        "no overflow"
    );
}
