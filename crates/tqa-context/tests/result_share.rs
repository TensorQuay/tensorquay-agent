//! CTX-E-004: a tool result may take at most a share of the input capacity still free when it is recorded, never less
//! than the floor that keeps its head, tail and spill notice, and never more than the configured result cap.
use tqa_context::{MAX_RESULT_WINDOW_SHARE, MIN_RESULT_BYTES, estimate_bytes, window_result_cap};

const LIMIT: usize = 64 * 1024;

#[test]
fn ctx_e_004_a_large_free_window_leaves_the_configured_cap() {
    assert_eq!(window_result_cap(LIMIT, 1000, 200_000), LIMIT);
}

#[test]
fn ctx_e_004_the_free_share_bounds_the_cap_in_estimator_bytes() {
    let (needed, capacity) = (584, 3584);
    let cap = window_result_cap(LIMIT, needed, capacity);
    let (numerator, denominator) = MAX_RESULT_WINDOW_SHARE;
    let share = (u64::from(capacity) - needed) * numerator / denominator;
    assert_eq!(cap, usize::try_from(share * 7 / 2).unwrap_or(usize::MAX));
    assert!(
        u64::from(estimate_bytes(cap).tokens) <= share,
        "the cap never costs more tokens than its share"
    );
    assert!(cap < LIMIT);
}

#[test]
fn ctx_e_004_a_nearly_full_or_overfull_window_keeps_the_floor() {
    assert_eq!(window_result_cap(LIMIT, 3500, 3584), MIN_RESULT_BYTES);
    assert_eq!(
        window_result_cap(LIMIT, 10_000, 3584),
        MIN_RESULT_BYTES,
        "needed above capacity saturates"
    );
}

#[test]
fn ctx_e_004_the_floor_never_raises_a_smaller_configured_cap_and_huge_windows_saturate() {
    assert_eq!(
        window_result_cap(MIN_RESULT_BYTES, 0, 3584),
        MIN_RESULT_BYTES
    );
    assert_eq!(
        window_result_cap(usize::MAX, 0, u32::MAX),
        usize::try_from(u64::from(u32::MAX) / 2 * 7 / 2).unwrap_or(usize::MAX)
    );
}
