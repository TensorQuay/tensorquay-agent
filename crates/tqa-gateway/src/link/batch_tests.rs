//! GW-U-116: which held runtime frames are journaled together. The run of consecutive plain journal frames at the front
//! of the inbox (sequenced events with no side effect of their own) is one batch, up to 256 frames and 1 MiB counted
//! at each frame's full size; a run lifecycle frame, a frame that isn't a journal entry, a frame that doesn't parse
//! and the link's end each go on their own, in their place. A lone plain frame is received as before.
use super::inbox::{BATCH_BYTES, BATCH_FRAMES, HELD_BYTES, HELD_FRAMES, Held, Next, next};
use crate::GatewayError;
use std::collections::VecDeque;
use tqa_types::{Event, ServerFrame};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn event(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

fn delta(seq: u64, text: &str) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&event(
        seq,
        Event::TextDelta {
            text: text.to_owned(),
        },
    ))
}

fn started(seq: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(serde_json::to_vec(&event(
        seq,
        Event::RunStarted {
            run_id: "r-1".parse()?,
        },
    ))?)
}

fn completed(seq: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(serde_json::to_vec(&event(
        seq,
        Event::RunCompleted {
            run_id: "r-1".parse()?,
        },
    ))?)
}

fn heartbeat() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(serde_json::to_vec(&ServerFrame::Heartbeat {
        run_id: "r-1".parse()?,
    })?)
}

/// Each `next` as a short description: `batch 1-3`, `frame 4`, `frame ?` (not a sequenced event), `end`.
fn drain(mut held: VecDeque<Held>) -> Vec<String> {
    let mut taken = Vec::new();
    while let Some((next, _)) = next(&mut held) {
        taken.push(match next {
            Next::Batch(frames) => {
                let seqs: Vec<String> = frames
                    .iter()
                    .map(|frame| {
                        crate::wire::sequence(frame).map_or("?".into(), |seq| seq.to_string())
                    })
                    .collect();
                format!("batch {}-{}", seqs[0], seqs[seqs.len() - 1])
            }
            Next::Frame(bytes) => match ServerFrame::parse(&bytes)
                .ok()
                .and_then(|frame| crate::wire::sequence(&frame))
            {
                Some(seq) => format!("frame {seq}"),
                None => "frame ?".into(),
            },
            Next::End(_) => "end".into(),
        });
    }
    taken
}

fn frames(bytes: impl IntoIterator<Item = Vec<u8>>) -> VecDeque<Held> {
    bytes.into_iter().map(Held::Frame).collect()
}

// TEST: GW-U-116
#[test]
fn gw_u_116_consecutive_plain_frames_are_one_batch_in_order() -> TestResult {
    let held = frames(
        (1..=3)
            .map(|seq| delta(seq, "x"))
            .collect::<Result<Vec<_>, _>>()?,
    );
    assert_eq!(drain(held), ["batch 1-3"]);
    Ok(())
}

// TEST: GW-U-116
#[test]
fn gw_u_116_a_lone_plain_frame_is_received_as_before() -> TestResult {
    assert_eq!(drain(frames([delta(1, "x")?])), ["frame 1"]);
    Ok(())
}

// TEST: GW-U-116
#[test]
fn gw_u_116_a_run_lifecycle_frame_ends_a_batch_and_goes_alone() -> TestResult {
    let held = frames([
        delta(1, "x")?,
        delta(2, "x")?,
        started(3)?,
        delta(4, "x")?,
        delta(5, "x")?,
        completed(6)?,
        delta(7, "x")?,
    ]);
    assert_eq!(
        drain(held),
        ["batch 1-2", "frame 3", "batch 4-5", "frame 6", "frame 7"]
    );
    Ok(())
}

// TEST: GW-U-116
#[test]
fn gw_u_116_a_frame_that_is_not_a_journal_entry_or_does_not_parse_keeps_its_place() -> TestResult {
    let held = frames([
        delta(1, "x")?,
        delta(2, "x")?,
        heartbeat()?,
        delta(3, "x")?,
        b"{not json".to_vec(),
        delta(4, "x")?,
        delta(5, "x")?,
    ]);
    assert_eq!(
        drain(held),
        ["batch 1-2", "frame ?", "frame 3", "frame ?", "batch 4-5"]
    );
    Ok(())
}

// TEST: GW-U-116
#[test]
fn gw_u_116_the_links_end_waits_behind_the_batch() -> TestResult {
    let mut held = frames([delta(1, "x")?, delta(2, "x")?]);
    held.push_back(Held::End(Err(GatewayError::Io)));
    assert_eq!(drain(held), ["batch 1-2", "end"]);
    Ok(())
}

// TEST: GW-U-116
#[test]
fn gw_u_116_a_batch_holds_at_most_256_frames() -> TestResult {
    assert_eq!((BATCH_FRAMES, HELD_FRAMES), (256, 256));
    let held = frames(
        (1..=300)
            .map(|seq| delta(seq, "x"))
            .collect::<Result<Vec<_>, _>>()?,
    );
    assert_eq!(drain(held), ["batch 1-256", "batch 257-300"]);
    Ok(())
}

// TEST: GW-U-116
#[test]
fn gw_u_116_a_batch_holds_at_most_one_mebibyte_counted_at_full_frame_size() -> TestResult {
    assert_eq!((BATCH_BYTES, HELD_BYTES), (1024 * 1024, 1024 * 1024));
    // Frames of about 200 KiB (under the protocol's 256 KiB frame limit), as large tool output the store spills to a
    // blob: five fit in 1 MiB, the sixth doesn't.
    let big = "y".repeat(200 * 1024);
    let mut queue = frames(
        (1..=7)
            .map(|seq| delta(seq, &big))
            .collect::<Result<Vec<_>, _>>()?,
    );
    let (first, bytes) = next(&mut queue).ok_or("nothing taken")?;
    assert!(matches!(&first, Next::Batch(frames) if frames.len() == 5));
    assert!(
        bytes > 5 * 200 * 1024 && bytes <= BATCH_BYTES,
        "{bytes} bytes taken"
    );
    assert_eq!(drain(queue), ["batch 6-7"]);
    Ok(())
}
