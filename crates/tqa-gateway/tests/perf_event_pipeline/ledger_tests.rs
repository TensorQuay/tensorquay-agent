//! PERF-002's accounting, without a gateway: the pacing schedule, the exactly-once receipt ledger, the burst rule and
//! the lag summary.
use super::{Pacing, Receipts, burst_in_time, lag_summary};
use std::time::Duration;

#[test]
fn ten_thousand_a_second_is_ten_events_per_millisecond_on_a_fixed_schedule() {
    let pacing = Pacing::per_second(10_000);
    assert_eq!(pacing.due(0), Duration::ZERO);
    assert_eq!(pacing.due(9), Duration::ZERO);
    assert_eq!(pacing.due(10), Duration::from_millis(1));
    assert_eq!(pacing.due(9_999), Duration::from_millis(999));
    assert_eq!(
        pacing.span(10_000),
        Duration::from_secs(1),
        "a burst of 10,000 spans one second"
    );
}

#[test]
fn receipts_must_be_every_sequence_once_and_in_order() {
    let mut receipts = Receipts::default();
    assert!(receipts.record(1).is_ok() && receipts.record(2).is_ok());
    assert_eq!(
        receipts.record(2),
        Err("sequence 2 arrived again after 2".into()),
        "duplicate"
    );
    assert_eq!(
        receipts.record(4),
        Err("sequence 4 arrived after 2: 3 is missing".into()),
        "gap"
    );
    assert_eq!(
        receipts.record(1),
        Err("sequence 1 arrived again after 2".into()),
        "reorder"
    );
    assert!(receipts.record(3).is_ok());
    assert_eq!(receipts.last(), 3);
    assert_eq!(receipts.complete(3), Ok(()));
    assert_eq!(
        receipts.complete(5),
        Err("only 3 of 5 events arrived".into())
    );
}

#[test]
fn a_burst_sent_late_fails_with_the_rate_it_reached() {
    assert!(burst_in_time(Duration::from_millis(1_050), 10_000, Duration::from_secs(1)).is_ok());
    let slow = burst_in_time(Duration::from_millis(1_250), 10_000, Duration::from_secs(1));
    assert_eq!(slow, Err("the burst of 10000 events took 1.250 s to send (8000 events/s): the link held the sender back".into()));
}

#[test]
fn the_lag_summary_is_p50_and_p99_in_milliseconds() {
    let lags: Vec<u64> = (1..=100).map(|ms| ms * 1_000_000).collect();
    let summary = lag_summary(&lags);
    assert_eq!(summary, Ok((50.0, 99.0)));
    assert!(lag_summary(&[]).is_err());
}

#[test]
fn the_rate_is_the_rows_ten_thousand_unless_a_recorded_variant_names_another() {
    assert_eq!(super::rate(None), Ok(10_000), "the TEST-PLAN row's rate");
    assert_eq!(
        super::rate(Some("1000")),
        Ok(1_000),
        "10 % of it, for low-rate lag"
    );
    for refused in ["0", "999", "1500", "abc", "-1000", "20000000"] {
        assert!(
            super::rate(Some(refused)).is_err(),
            "{refused}: a whole number of thousands, 1,000 to 100,000"
        );
    }
}
