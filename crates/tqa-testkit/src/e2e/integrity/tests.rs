use super::*;
use tqa_types::Event;

fn frame(seq: u64, text: &str) -> ServerFrame {
    ServerFrame::Event {
        seq: Seq::from(seq),
        run_id: None,
        step: None,
        ts: "2026-09-26T00:00:00Z".into(),
        event: Event::TextDelta { text: text.into() },
    }
}
fn log(seqs: &[u64]) -> Vec<ServerFrame> {
    seqs.iter().map(|seq| frame(*seq, "x")).collect()
}

#[test]
fn tk_u_003_a_contiguous_replay_covering_the_stream_is_intact() -> Result<(), serde_json::Error> {
    // Unsequenced frames are ignored wherever they appear.
    let unsequenced = ServerFrame::Heartbeat {
        run_id: serde_json::from_value(serde_json::json!("r-1"))?,
    };
    let mut streamed = log(&[1, 2, 4]);
    streamed.insert(1, unsequenced.clone());
    let mut replayed = log(&[1, 2, 3, 4]);
    replayed.push(unsequenced);
    assert_eq!(verify(&streamed, &replayed), Ok(()));
    assert_eq!(verify(&[], &[]), Ok(()));
    Ok(())
}

#[test]
fn tk_u_003_gaps_duplicates_and_mismatches_are_the_first_fault() {
    let gap = |expected: u64, got: u64| IntegrityFault::Gap {
        expected: expected.into(),
        got: got.into(),
    };
    let cases = [
        (log(&[1]), log(&[1, 3]), gap(2, 3)),
        (log(&[]), log(&[2, 3]), gap(1, 2)),
        (log(&[]), log(&[1, 1]), gap(2, 1)),
        (
            log(&[1, 2, 2]),
            log(&[1, 2]),
            IntegrityFault::Duplicate { seq: 2.into() },
        ),
        (
            log(&[2, 1]),
            log(&[1, 2]),
            IntegrityFault::Duplicate { seq: 1.into() },
        ),
        (
            vec![frame(1, "changed")],
            log(&[1]),
            IntegrityFault::Mismatch { seq: 1.into() },
        ),
        (
            log(&[1, 2]),
            log(&[1]),
            IntegrityFault::Mismatch { seq: 2.into() },
        ),
    ];
    for (streamed, replayed, fault) in cases {
        assert_eq!(verify(&streamed, &replayed), Err(fault));
        assert_eq!(
            fault.seq(),
            match fault {
                IntegrityFault::Gap { got, .. } => got,
                IntegrityFault::Duplicate { seq } | IntegrityFault::Mismatch { seq } => seq,
            }
        );
    }
}
