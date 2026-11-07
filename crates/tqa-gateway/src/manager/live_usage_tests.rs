//! GW-U-052: the live quota's margin and its measurement schedule (N4).
use super::{MAX_MARGIN, MIN_MARGIN, Sample, hard_limit, next_due};
use std::time::{Duration, Instant};

const GIB: u64 = 1 << 30;
const INTERVAL: Duration = Duration::from_secs(30);

// TEST: GW-U-052
#[test]
fn gw_u_052_the_margin_is_a_percentage_of_the_quota_within_its_bounds() {
    // 10 % of 10 GiB is 1 GiB.
    assert_eq!(hard_limit(10 * GIB, 10), 11 * GIB);
    // 10 % of 1 GiB is below the floor.
    assert_eq!(hard_limit(GIB, 10), GIB + MIN_MARGIN);
    // 10 % of 1 TiB is above the cap.
    assert_eq!(hard_limit(1024 * GIB, 10), 1024 * GIB + MAX_MARGIN);
    // No overflow at the extremes.
    assert_eq!(hard_limit(u64::MAX, 100), u64::MAX);
}

// TEST: GW-U-052
#[test]
fn gw_u_052_a_steady_workspace_far_from_its_limit_is_measured_at_the_interval() {
    let then = Instant::now();
    let last = Sample {
        at: then + INTERVAL,
        bytes: GIB,
    };
    let previous = Some(Sample {
        at: then,
        bytes: GIB,
    });
    assert_eq!(
        next_due(
            INTERVAL,
            11 * GIB,
            last,
            previous,
            Duration::from_millis(20)
        ),
        INTERVAL
    );
    assert_eq!(
        next_due(INTERVAL, 11 * GIB, last, None, Duration::ZERO),
        INTERVAL
    );
}

// TEST: GW-U-052
#[test]
fn gw_u_052_near_the_limit_it_is_measured_every_five_seconds() {
    let now = Instant::now();
    let last = Sample {
        at: now,
        bytes: 9 * GIB,
    };
    assert_eq!(
        next_due(INTERVAL, 11 * GIB, last, None, Duration::ZERO),
        Duration::from_secs(5)
    );
}

// TEST: GW-U-052
#[test]
fn gw_u_052_fast_growth_brings_the_next_measurement_to_half_the_time_to_the_limit() {
    let then = Instant::now();
    // 1 GiB in 10 s; 4 GiB left: the limit is 40 s away, so the next measurement is due in 20 s.
    let last = Sample {
        at: then + Duration::from_secs(10),
        bytes: 7 * GIB,
    };
    let previous = Some(Sample {
        at: then,
        bytes: 6 * GIB,
    });
    assert_eq!(
        next_due(
            Duration::from_mins(1),
            11 * GIB,
            last,
            previous,
            Duration::ZERO
        ),
        Duration::from_secs(20)
    );
    // Much faster growth hits the floor of 2 s, never sooner.
    let previous = Some(Sample {
        at: then + Duration::from_secs(9),
        bytes: GIB,
    });
    assert_eq!(
        next_due(INTERVAL, 11 * GIB, last, previous, Duration::ZERO),
        Duration::from_secs(2)
    );
}

// TEST: GW-U-052
#[test]
fn gw_u_052_an_expensive_walk_is_spaced_by_ten_times_its_cost() {
    let now = Instant::now();
    let last = Sample {
        at: now,
        bytes: 9 * GIB,
    };
    // Near the limit (5 s), but the last walk took 3 s: not again for 30 s.
    assert_eq!(
        next_due(INTERVAL, 11 * GIB, last, None, Duration::from_secs(3)),
        Duration::from_secs(30)
    );
    // A cost larger than the interval stretches it too.
    assert_eq!(
        next_due(
            INTERVAL,
            11 * GIB,
            Sample {
                at: now,
                bytes: GIB
            },
            None,
            Duration::from_secs(6)
        ),
        Duration::from_mins(1)
    );
}
