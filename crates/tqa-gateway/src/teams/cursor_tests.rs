//! GW-U-126: the agent-team read cursor (ADR-0034, the slice S1 contract): a read's answer is recorded per key and a
//! repeat answers the same; the reader's seq advances only with the stored result of that read; recorded answers are
//! pruned with the advance and when their run ends.
use super::Cursor;

#[test]
fn gw_u_126_a_read_answers_what_is_after_the_last_read_seq_in_order_up_to_its_limit() {
    let mut cursor = Cursor::new(3);
    assert_eq!(
        cursor.answer("k1", "run-1", &[9, 2, 4, 7, 3], 50),
        vec![4, 7, 9]
    );
    let mut cursor = Cursor::new(0);
    assert_eq!(cursor.answer("k1", "run-1", &[1, 2, 3, 4], 2), vec![1, 2]);
}

#[test]
fn gw_u_126_a_repeat_answers_the_same_even_after_newer_messages() {
    let mut cursor = Cursor::new(0);
    assert_eq!(cursor.answer("k1", "run-1", &[1, 2], 50), vec![1, 2]);
    assert_eq!(cursor.answer("k1", "run-1", &[1, 2, 3, 4], 50), vec![1, 2]);
    assert_eq!(
        cursor.answer("k2", "run-1", &[1, 2, 3, 4], 50),
        vec![3, 4],
        "a new key starts after what is still outstanding"
    );
    let mut cursor = Cursor::new(0);
    assert_eq!(cursor.answer("empty", "run-1", &[], 50), Vec::<u64>::new());
    assert_eq!(cursor.answer("empty", "run-1", &[1], 50), Vec::<u64>::new());
}

#[test]
fn gw_u_126_the_seq_advances_only_with_the_stored_result_of_that_read() {
    let mut cursor = Cursor::new(0);
    cursor.answer("k1", "run-1", &[1, 2], 50);
    assert_eq!(cursor.last_read(), 0);
    assert!(cursor.result_stored("k1"));
    assert_eq!(cursor.last_read(), 2);
    assert_eq!(cursor.answer("k3", "run-1", &[1, 2, 3], 50), vec![3]);
}

#[test]
fn gw_u_126_a_result_for_an_unknown_key_changes_nothing() {
    let mut cursor = Cursor::new(5);
    cursor.answer("k1", "run-1", &[6, 7], 50);
    assert!(!cursor.result_stored("other"));
    assert_eq!(cursor.last_read(), 5);
    assert_eq!(cursor.recorded(), 1);
}

#[test]
fn gw_u_126_an_older_result_stored_late_never_moves_the_seq_back() {
    let mut cursor = Cursor::new(0);
    cursor.answer("early", "run-1", &[1], 50);
    cursor.answer("late", "run-1", &[1, 2, 3], 50);
    assert!(
        cursor.result_stored("late"),
        "late answered 2 and 3, after early's 1"
    );
    assert_eq!(cursor.last_read(), 3);
    assert!(!cursor.result_stored("early"), "pruned with the advance");
    assert_eq!(cursor.last_read(), 3);
}

#[test]
fn gw_u_126_recorded_answers_are_pruned_with_the_advance_and_when_their_run_ends() {
    let mut cursor = Cursor::new(0);
    cursor.answer("a", "run-1", &[1, 2], 50);
    cursor.answer("b", "run-1", &[1, 2, 3], 50);
    cursor.answer("c", "run-2", &[1, 2, 3, 4], 50);
    assert_eq!(cursor.recorded(), 3);
    assert!(cursor.result_stored("b"));
    assert_eq!(
        cursor.recorded(),
        1,
        "a and b are at or below the new seq; c (seq 4) is not"
    );
    cursor.answer("d", "run-2", &[4, 5], 50);
    cursor.run_ended("run-2");
    assert_eq!(cursor.recorded(), 0);
    assert_eq!(cursor.last_read(), 3, "a run's end advances nothing");
    assert_eq!(cursor.answer("e", "run-3", &[4, 5], 50), vec![4, 5]);
}

#[test]
fn gw_u_126_an_empty_answer_is_pruned_when_its_result_is_stored() {
    let mut cursor = Cursor::new(4);
    cursor.answer("empty", "run-1", &[], 50);
    assert!(cursor.result_stored("empty"));
    assert_eq!(cursor.last_read(), 4);
    assert_eq!(cursor.recorded(), 0);
}

#[test]
fn gw_u_126_a_read_starts_after_the_answers_still_outstanding() {
    // Two reads before either result is stored (review H1): the second never answers what the first did.
    let mut cursor = Cursor::new(0);
    assert_eq!(cursor.answer("first", "run-1", &[1, 2], 50), vec![1, 2]);
    assert_eq!(cursor.answer("second", "run-1", &[1, 2, 3], 50), vec![3]);
    assert_eq!(
        cursor.answer("third", "run-1", &[1, 2, 3], 50),
        Vec::<u64>::new()
    );
    assert!(cursor.result_stored("first"));
    assert_eq!(cursor.last_read(), 2);
    assert!(cursor.result_stored("second"));
    assert_eq!(cursor.last_read(), 3);
}
