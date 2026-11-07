//! RT-I-005: recovery joins the authoritative gateway prefix and unacknowledged WAL tail.
use tqa_runtime::merge_history;
use tqa_types::{ErrorCode, Seq, ServerFrame};

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn frame(seq: u64) -> ServerFrame {
    ServerFrame::Error {
        target: None,
        seq: Some(Seq::from(seq)),
        code: ErrorCode::Io,
        message: "test event".into(),
        retryable: false,
    }
}

#[test]
fn rt_i_005_history_rejects_gaps_duplicates_reordering_and_partial_prefixes() {
    for stored in [
        vec![frame(2)],
        vec![frame(1), frame(3)],
        vec![frame(1), frame(1)],
        vec![frame(2), frame(1)],
    ] {
        assert!(merge_history(stored, Vec::new(), Seq::from(2)).is_err());
    }
    assert!(merge_history(vec![frame(1)], Vec::new(), Seq::from(2)).is_err());
    for tail in [
        vec![(Seq::from(3), frame(3))],
        vec![(Seq::from(2), frame(2)), (Seq::from(2), frame(2))],
        vec![(Seq::from(2), frame(3))],
    ] {
        assert!(merge_history(vec![frame(1)], tail, Seq::from(1)).is_err());
    }
}

#[test]
fn rt_i_005_history_uses_the_stored_prefix_then_only_unacknowledged_records() -> TestResult {
    let stored = vec![frame(1), frame(2)];
    let tail = (1..=4).map(|seq| (Seq::from(seq), frame(seq))).collect();
    let history = merge_history(stored, tail, Seq::from(2))?;
    assert_eq!(history, (1..=4).map(frame).collect::<Vec<_>>());
    assert!(merge_history(Vec::new(), Vec::new(), Seq::ZERO)?.is_empty());
    assert_eq!(
        merge_history(Vec::new(), vec![(Seq::from(1), frame(1))], Seq::ZERO)?,
        vec![frame(1)]
    );
    Ok(())
}

#[test]
fn rt_i_005_history_rejects_unsequenced_gateway_frames() {
    let control = ServerFrame::Error {
        target: None,
        seq: None,
        code: ErrorCode::Busy,
        message: "busy".into(),
        retryable: true,
    };
    assert!(merge_history(vec![control], Vec::new(), Seq::from(1)).is_err());
}
