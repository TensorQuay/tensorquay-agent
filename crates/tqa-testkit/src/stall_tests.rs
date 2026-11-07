//! TK-I-007's and GW-I-181's rule: a delayed-ACK stall shows as rounds over 20 ms; up to two are allowed for scheduling
//! hiccups on a loaded host, three or more fail, listing each late round.
use super::{LATE_ROUNDS_ALLOWED, STALL, check, check_paired};
use std::time::Duration;

fn rounds(late: &[(usize, u64)]) -> Vec<Duration> {
    let mut times = vec![Duration::from_millis(1); 20];
    for (index, millis) in late {
        times[*index] = Duration::from_millis(*millis);
    }
    times
}

// TEST: TK-I-007
#[test]
fn two_late_rounds_are_hiccups_and_pass() {
    assert_eq!((STALL, LATE_ROUNDS_ALLOWED), (Duration::from_millis(20), 2));
    assert_eq!(check("replies", &rounds(&[(3, 89), (11, 211)])), Ok(()));
    assert_eq!(check("replies", &rounds(&[])), Ok(()));
}

// TEST: TK-I-007
#[test]
fn three_late_rounds_fail_and_name_each_one() {
    assert_eq!(
        check("replies", &rounds(&[(2, 41), (5, 41), (9, 42)])),
        Err(
            "3 of 20 replies took over 20ms (at most 2 may, for scheduling hiccups): \
             round 3 41ms, round 6 41ms, round 10 42ms"
                .to_owned()
        )
    );
}

// TEST: TK-I-007
#[test]
fn the_unfixed_stall_and_a_partial_one_both_fail() {
    let unfixed: Vec<(usize, u64)> = (1..20).map(|index| (index, 41)).collect();
    assert!(
        check("replies", &rounds(&unfixed)).is_err(),
        "19 of 20 at 41 ms"
    );
    let partial: Vec<(usize, u64)> = (0..9).map(|index| (index * 2, 41)).collect();
    assert!(
        check("replies", &rounds(&partial)).is_err(),
        "9 of 20, which a median passes"
    );
}

/// 20 rounds whose baseline and measured span both take `load` ms, the rounds in `stalled` 40 ms more.
fn paired(load: &[u64], stalled: &[usize]) -> Vec<(Duration, Duration)> {
    (0..20)
        .map(|index| {
            let base = Duration::from_millis(load[index % load.len()]);
            let extra = if stalled.contains(&index) { 40 } else { 0 };
            (base + Duration::from_millis(extra), base)
        })
        .collect()
}

// TEST: GW-I-181
#[test]
fn a_loaded_host_slows_the_baseline_too_and_passes() {
    // GW-I-181 at load 30: rounds of 23 to 225 ms, no stall; the fixed bound failed 16 and 19 of 20.
    let load = [23, 93, 127, 41, 75, 150, 225, 58];
    assert!(
        check(
            "relayed streams",
            &paired(&load, &[]).iter().map(|r| r.0).collect::<Vec<_>>()
        )
        .is_err()
    );
    assert_eq!(check_paired("relayed streams", &paired(&load, &[])), Ok(()));
    assert_eq!(
        check_paired("relayed streams", &paired(&load, &[4, 15])),
        Ok(()),
        "two hiccups"
    );
}

// TEST: GW-I-181
#[test]
fn a_stall_over_its_baseline_fails_under_load_too_naming_each_round() {
    let unfixed: Vec<usize> = (1..20).collect();
    assert!(
        check_paired("relayed streams", &paired(&[1], &unfixed)).is_err(),
        "idle host"
    );
    assert!(
        check_paired("relayed streams", &paired(&[60, 140], &unfixed)).is_err(),
        "loaded host"
    );
    let partial: Vec<usize> = (0..10).map(|index| index * 2).collect();
    assert!(
        check_paired("relayed streams", &paired(&[30], &partial)).is_err(),
        "every other round"
    );
    assert_eq!(
        check_paired("relayed streams", &paired(&[5], &[2, 5, 9])),
        Err("3 of 20 relayed streams took over 20ms longer than their baseline (at most 2 may, for scheduling \
             hiccups): round 3 45ms (baseline 5ms), round 6 45ms (baseline 5ms), round 10 45ms (baseline 5ms)"
            .to_owned())
    );
}

// TEST: GW-I-181
#[test]
fn a_round_faster_than_its_baseline_is_never_late() {
    let faster = vec![(Duration::from_millis(1), Duration::from_millis(90)); 20];
    assert_eq!(check_paired("relayed streams", &faster), Ok(()));
}
