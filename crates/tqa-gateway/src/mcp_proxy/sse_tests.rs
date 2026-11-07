//! GW-U-039: the MCP proxy's event-stream framer (ADR-0031 decision 6).
use super::{Event, Framer, TooLarge};

fn event(data: &str) -> Event {
    Event {
        data: data.to_owned(),
        ..Event::default()
    }
}

#[test]
fn gw_u_039_whole_events_come_out_however_the_bytes_arrive() -> Result<(), TooLarge> {
    let stream = b"event: message\nid: 7\ndata: {\"a\":1}\n\ndata: second\n\n";
    let mut whole = Framer::new(1024);
    let all = whole.push(stream)?;
    assert_eq!(
        all,
        [
            Event {
                kind: Some("message".into()),
                id: Some("7".into()),
                retry: None,
                data: "{\"a\":1}".into(),
            },
            event("second")
        ]
    );
    // One byte at a time gives the same events.
    let mut split = Framer::new(1024);
    let mut pieces = Vec::new();
    for byte in stream {
        pieces.extend(split.push(&[*byte])?);
    }
    assert_eq!(pieces, all);
    Ok(())
}

#[test]
fn gw_u_039_the_specifications_line_rules_hold() -> Result<(), TooLarge> {
    let mut framer = Framer::new(1024);
    let events = framer.push(
        b": a comment\r\ndata:no-space\r\ndata: two\r\n\r\n\
          retry: 1500\ndata\nunknown: x\nid: with\0nul\n\n\
          event: empty\n\n\
          retry: soon\ndata: x\n\n",
    )?;
    assert_eq!(
        events,
        [
            event("no-space\ntwo"),
            Event {
                retry: Some("1500".into()),
                ..event("")
            },
            event("x"),
        ],
        "CRLF, a bare field, multi-line data, comments and unknown fields; an event without data is dropped; \
         an invalid retry or an id with NUL is ignored"
    );
    Ok(())
}

#[test]
fn gw_u_039_an_event_larger_than_the_bound_ends_the_stream() -> Result<(), TooLarge> {
    let mut framer = Framer::new(16);
    assert_eq!(framer.push(b"data: 0123456789\n\n")?, [event("0123456789")]);
    assert_eq!(framer.push(b"data: 01234"), Ok(vec![]));
    assert_eq!(
        framer.push(b"56789\ndata: x"),
        Err(TooLarge),
        "the bound counts the whole unfinished event"
    );
    let mut line = Framer::new(8);
    assert_eq!(
        line.push(b"data: 12345"),
        Err(TooLarge),
        "and a line that never ends"
    );
    Ok(())
}

#[test]
fn gw_u_039_events_encode_back_to_the_wire() {
    let full = Event {
        kind: Some("message".into()),
        id: Some("7".into()),
        retry: Some("10".into()),
        data: "one\ntwo".into(),
    };
    assert_eq!(
        full.encode(),
        "event: message\nid: 7\nretry: 10\ndata: one\ndata: two\n\n"
    );
    assert_eq!(event("{}").encode(), "data: {}\n\n");
    let mut framer = Framer::new(1024);
    assert_eq!(
        framer.push(full.encode().as_bytes()),
        Ok(vec![full]),
        "round trip"
    );
}
