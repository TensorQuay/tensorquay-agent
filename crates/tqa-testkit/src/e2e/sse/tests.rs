use super::*;
use tqa_types::{Event, Seq};

fn frame(seq: u64) -> ServerFrame {
    ServerFrame::Event {
        seq: Seq::from(seq),
        run_id: None,
        step: None,
        ts: "2026-09-26T00:00:00Z".into(),
        event: Event::TextDelta {
            text: format!("t{seq}"),
        },
    }
}
fn wire(seq: u64, ending: &str) -> Result<String, serde_json::Error> {
    Ok(format!(
        "id: {seq}{ending}data: {}{ending}{ending}",
        serde_json::to_string(&frame(seq))?
    ))
}

#[test]
fn tk_u_006_frames_survive_any_chunking_and_keep_alives_are_skipped()
-> Result<(), Box<dyn std::error::Error>> {
    let text = format!(
        ": keep-alive\n\n{}{}: keep-alive\n\n{}",
        wire(1, "\n")?,
        wire(2, "\r\n")?,
        wire(3, "\n")?
    );
    for size in [1, 2, 7, 64, text.len()] {
        let mut decoder = SseDecoder::default();
        let mut frames = Vec::new();
        for chunk in text.as_bytes().chunks(size) {
            frames.extend(decoder.push(chunk)?);
        }
        assert_eq!(
            frames,
            vec![frame(1), frame(2), frame(3)],
            "chunk size {size}"
        );
    }
    Ok(())
}

#[test]
fn tk_u_006_malformed_and_oversized_events_stop_the_stream() {
    assert_eq!(
        SseDecoder::default().push(b"data: {not json}\n\n"),
        Err(SseError::Malformed)
    );
    assert_eq!(
        SseDecoder::default().push(b"data: \xff\n\n"),
        Err(SseError::Malformed)
    );
    let mut decoder = SseDecoder::default();
    let unterminated = vec![b'x'; MAX_EVENT_BYTES + 1];
    assert_eq!(decoder.push(&unterminated), Err(SseError::TooLarge));
}
