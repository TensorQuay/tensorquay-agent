//! GW-U-062: an event stream labelled text/plain is still an event stream, and nothing else is.
use super::stream_kind::{StreamKind, classify};

#[test]
fn gw_u_062_only_a_text_plain_body_starting_with_an_sse_field_is_a_mislabelled_stream() {
    for (content_type, first, kind) in [
        (
            Some("text/event-stream"),
            "data: {}",
            StreamKind::EventStream,
        ),
        (
            Some("text/event-stream; charset=utf-8"),
            "",
            StreamKind::EventStream,
        ),
        (
            Some("text/plain; charset=utf-8"),
            "event: response.created\ndata: {}",
            StreamKind::Mislabelled,
        ),
        (
            Some("text/plain"),
            "data: {\"choices\":[]}",
            StreamKind::Mislabelled,
        ),
        (
            Some("text/plain"),
            "\r\n\ndata: {}",
            StreamKind::Mislabelled,
        ),
        (
            Some("text/plain"),
            "{\"error\":\"data: no\"}",
            StreamKind::Other,
        ),
        (Some("text/plain"), ": keep-alive\n", StreamKind::Other),
        (Some("text/plain"), "", StreamKind::Other),
        (Some("application/json"), "data: {}", StreamKind::Other),
        (None, "data: {}", StreamKind::Other),
    ] {
        assert_eq!(
            classify(content_type, first.as_bytes()),
            kind,
            "{content_type:?} {first:?}"
        );
    }
}

#[test]
fn gw_u_062_a_comment_first_or_a_field_split_across_chunks_is_still_a_stream() {
    use super::stream_kind::{SNIFF_BYTES, Sniffer};
    for (chunks, kind) in [
        (vec!["da", "ta: {}\n"], StreamKind::Mislabelled),
        (
            vec![": ping\n", "\n", "event: x\n"],
            StreamKind::Mislabelled,
        ),
        (
            vec![":", " keep", "-alive\r\ndata: {}"],
            StreamKind::Mislabelled,
        ),
        (vec!["ev", "ent", ": x"], StreamKind::Mislabelled),
        (vec!["{\"error\":1}"], StreamKind::Other),
        (vec!["d", "one\n"], StreamKind::Other),
    ] {
        let mut sniffer = Sniffer::default();
        let decided = chunks
            .iter()
            .find_map(|chunk| sniffer.push(chunk.as_bytes()));
        assert_eq!(
            decided.unwrap_or_else(|| sniffer.finish()),
            kind,
            "{chunks:?}"
        );
    }
    // Undecided until a line ends: a partial field or comment waits for more.
    for start in ["da", ":", "", "\r"] {
        assert_eq!(Sniffer::default().push(start.as_bytes()), None, "{start:?}");
    }
    // The body ends undecided: not a stream.
    let mut sniffer = Sniffer::default();
    assert_eq!(sniffer.push(b"da"), None);
    assert_eq!(sniffer.finish(), StreamKind::Other);
    // A comment that never ends is decided at the bound: not a stream.
    let mut sniffer = Sniffer::default();
    let long = format!(":{}", "x".repeat(SNIFF_BYTES));
    assert_eq!(sniffer.push(long.as_bytes()), Some(StreamKind::Other));
    // classify on a whole first chunk agrees.
    assert_eq!(
        classify(Some("text/plain"), b": keep-alive\ndata: {}"),
        StreamKind::Mislabelled
    );
}
