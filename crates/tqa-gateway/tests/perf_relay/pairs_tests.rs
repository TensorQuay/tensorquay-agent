//! PERF-003's accounting, without a gateway: the alternating pair order, the relay's added time per pair (which can be
//! negative within noise), the summary and its two p50 gates, and the content oracle.
use super::{Path, Timing, added, content_deltas, order, summary, verdict};

#[test]
fn pairs_alternate_which_path_goes_first() {
    assert_eq!(order(0), [Path::Direct, Path::Relay]);
    assert_eq!(order(1), [Path::Relay, Path::Direct]);
    assert_eq!(order(50), [Path::Direct, Path::Relay]);
}

#[test]
fn the_added_time_is_relay_minus_direct_per_pair_and_may_be_negative() {
    let ms = |value: i64| value * 1_000_000;
    let pairs = [
        (
            Timing {
                first_token_ns: ms(2),
                total_ns: ms(5),
            },
            Timing {
                first_token_ns: ms(4),
                total_ns: ms(9),
            },
        ),
        (
            Timing {
                first_token_ns: ms(3),
                total_ns: ms(6),
            },
            Timing {
                first_token_ns: ms(2),
                total_ns: ms(5),
            },
        ),
    ];
    let (total, first_token) = added(&pairs);
    assert_eq!(total, [ms(4), ms(-1)]);
    assert_eq!(first_token, [ms(2), ms(-1)]);
}

#[test]
fn the_summary_is_p50_and_p99_in_milliseconds_and_the_gates_are_on_p50() {
    let total: Vec<i64> = (1..=50).map(|ms| ms * 400_000).collect(); // 0.4 to 20 ms
    let first_token: Vec<i64> = (1..=50).map(|ms| ms * 800_000).collect(); // 0.8 to 40 ms
    let (total, first_token) = (summary(&total), summary(&first_token));
    assert_eq!(total, Ok((10.0, 20.0)));
    assert_eq!(first_token, Ok((20.0, 40.0)));
    assert!(verdict(10.0, 20.0).is_ok(), "both at their p50 limit: met");
    assert_eq!(
        verdict(10.5, 3.0),
        Err("the relay adds 10.500 ms at p50 per request (target ≤ 10 ms)".into())
    );
    assert_eq!(
        verdict(1.0, 20.5),
        Err("the relay adds 20.500 ms at p50 to the first token (target ≤ 20 ms)".into())
    );
    assert!(summary(&[]).is_err());
}

#[test]
fn both_paths_must_carry_the_same_content() {
    let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n: keep-alive\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\ndata: {\"choices\":[],\"usage\":{}}\n\ndata: [DONE]\n\n";
    assert_eq!(content_deltas(stream), ["Hel", "lo"]);
    assert!(content_deltas("data: not json\n\n").is_empty());
}
