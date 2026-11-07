//! GW-U-147: the screen relay's filter of what a viewer sends (ADR-0039 section 5, GW-I-218's unit half).
use super::{ClientFilter, Refused};
use std::time::{Duration, Instant};

/// A filter past the RFB 3.8 handshake, and what it forwarded of it.
fn ready(now: Instant) -> Result<(ClientFilter, Vec<u8>), Refused> {
    let mut filter = ClientFilter::new();
    let forwarded = filter.feed(b"RFB 003.008\n\x01\x00", false, now)?;
    Ok((filter, forwarded))
}

fn key(down: bool) -> Vec<u8> {
    vec![4, u8::from(down), 0, 0, 0, 0, 0, 0x61]
}

fn pointer() -> Vec<u8> {
    vec![5, 1, 0, 10, 0, 20]
}

fn paste(text: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut message = vec![6, 0, 0, 0];
    message.extend_from_slice(&u32::try_from(text.len())?.to_be_bytes());
    message.extend_from_slice(text);
    Ok(message)
}

fn update(incremental: bool) -> Vec<u8> {
    vec![3, u8::from(incremental), 0, 0, 0, 0, 5, 0, 3, 32]
}

// TEST: GW-U-147
#[test]
fn gw_u_147_the_handshake_passes_and_the_session_is_always_shared()
-> Result<(), Box<dyn std::error::Error>> {
    let now = Instant::now();
    let (_, forwarded) = ready(now)?;
    // The client asked for an exclusive session (0); the server is told shared (1), so no viewer drops another.
    assert_eq!(forwarded, b"RFB 003.008\n\x01\x01");
    // RFB 3.3: the server picks the security; the client's next byte is ClientInit.
    let mut old = ClientFilter::new();
    assert_eq!(
        old.feed(b"RFB 003.003\n\x00", false, now)?,
        b"RFB 003.003\n\x01"
    );
    for refused in [
        &b"RFB 004.001\n"[..],
        b"HTTP/1.1 200\n",
        b"RFB 003.008\n\x02",
    ] {
        let mut filter = ClientFilter::new();
        assert_eq!(
            filter.feed(refused, false, now),
            Err(Refused::Handshake),
            "{refused:?}"
        );
    }
    Ok(())
}

// TEST: GW-U-147
#[test]
fn gw_u_147_a_view_only_viewer_sends_no_input_and_the_holder_does()
-> Result<(), Box<dyn std::error::Error>> {
    let now = Instant::now();
    let (mut viewer, _) = ready(now)?;
    let mut input = key(true);
    input.extend(pointer());
    input.extend(paste(b"secret")?);
    assert_eq!(
        viewer.feed(&input, false, now)?,
        b"",
        "dropped, whatever the page sends"
    );
    let (mut holder, _) = ready(now)?;
    assert_eq!(holder.feed(&input, true, now)?, input);
    // The formats and encodings always pass.
    let mut setup = vec![0_u8; 20];
    setup.extend([2, 0, 0, 2, 0, 0, 0, 7, 0xff, 0xff, 0xff, 0x21]);
    assert_eq!(viewer.feed(&setup, false, now)?, setup);
    Ok(())
}

// TEST: GW-U-147
#[test]
fn gw_u_147_messages_split_across_frames_are_framed_whole() -> Result<(), Box<dyn std::error::Error>>
{
    let now = Instant::now();
    let (mut holder, _) = ready(now)?;
    let message = paste(b"a long address")?;
    let (head, tail) = message.split_at(5);
    assert_eq!(
        holder.feed(head, true, now)?,
        b"",
        "incomplete: held, not passed in part"
    );
    assert_eq!(holder.feed(tail, true, now)?, message);
    Ok(())
}

// TEST: GW-U-147
#[test]
fn gw_u_147_an_unknown_or_misframed_message_refuses_the_viewer()
-> Result<(), Box<dyn std::error::Error>> {
    let now = Instant::now();
    for (message, refused) in [
        (vec![150, 0, 0, 0, 0, 0, 0, 0, 0, 0], Refused::Unknown(150)),
        (vec![255, 0], Refused::Unknown(255)),
        // Past the encodings' bound.
        (vec![2, 0, 0, 65], Refused::Malformed),
        // The extended clipboard (a negative length), which the server never offers.
        (vec![6, 0, 0, 0, 0xff, 0xff, 0xff, 0xf0], Refused::Malformed),
        // A paste past 256 KiB.
        (vec![6, 0, 0, 0, 0, 4, 0, 1], Refused::Malformed),
    ] {
        let (mut filter, _) = ready(now)?;
        assert_eq!(
            filter.feed(&message, true, now),
            Err(refused),
            "{message:?}"
        );
    }
    Ok(())
}

// TEST: GW-U-147
#[test]
fn gw_u_147_update_requests_pass_ten_a_second_and_a_sooner_one_is_held()
-> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let (mut filter, _) = ready(start)?;
    assert_eq!(filter.feed(&update(true), false, start)?, update(true));
    let soon = start + Duration::from_millis(30);
    assert_eq!(
        filter.feed(&update(true), false, soon)?,
        b"",
        "held, not dropped"
    );
    // A full request held after an incremental one stays full.
    assert_eq!(filter.feed(&update(false), false, soon)?, b"");
    assert_eq!(filter.feed(&update(true), false, soon)?, b"");
    assert_eq!(filter.next_due(), Some(start + Duration::from_millis(100)));
    assert_eq!(filter.due(start + Duration::from_millis(99)), None);
    assert_eq!(
        filter.due(start + Duration::from_millis(100)),
        Some(update(false)),
        "the one held, sent when due, full"
    );
    assert_eq!(filter.next_due(), None);
    let later = start + Duration::from_millis(250);
    assert_eq!(filter.feed(&update(true), false, later)?, update(true));
    Ok(())
}

fn set_encodings(encodings: &[i32]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut message = vec![2, 0];
    message.extend_from_slice(&u16::try_from(encodings.len())?.to_be_bytes());
    for encoding in encodings {
        message.extend_from_slice(&encoding.to_be_bytes());
    }
    Ok(message)
}

// TEST: GW-U-147
#[test]
fn gw_u_147_set_encodings_keeps_only_what_adds_no_client_message()
-> Result<(), Box<dyn std::error::Error>> {
    let now = Instant::now();
    let (mut filter, _) = ready(now)?;
    // noVNC's list: Tight, ZRLE, Hextile, Raw, CopyRect, quality and compression levels, the cursor, desktop size,
    // last rect and name, and the ones that bring a client message the filter doesn't frame (QEMU key -258, extended
    // desktop size -308, xvp -309, fence -312, continuous updates -313, the extended clipboard).
    let offered = [
        7,
        16,
        5,
        0,
        1,
        -26,
        -250,
        -239,
        -223,
        -224,
        -307,
        -258,
        -308,
        -309,
        -312,
        -313,
        i32::from_be_bytes([0xc0, 0xa1, 0xe5, 0xce]),
        1_000,
    ];
    let kept = [7, 16, 5, 0, 1, -26, -250, -239, -223, -224, -307];
    assert_eq!(
        filter.feed(&set_encodings(&offered)?, false, now)?,
        set_encodings(&kept)?
    );
    // None left is still a well-formed message.
    assert_eq!(
        filter.feed(&set_encodings(&[-312])?, false, now)?,
        set_encodings(&[])?
    );
    Ok(())
}

// TEST: GW-U-147
#[test]
fn gw_u_147_a_hand_back_releases_what_the_holder_still_has_down()
-> Result<(), Box<dyn std::error::Error>> {
    let now = Instant::now();
    let (mut filter, _) = ready(now)?;
    assert_eq!(filter.release(), None, "nothing pressed");
    // Shift (0xffe1) and "a" down, "a" up again, the left button down at (10, 20).
    let shift = vec![4, 1, 0, 0, 0, 0, 0xff, 0xe1];
    let mut input = shift.clone();
    input.extend(key(true));
    input.extend(key(false));
    input.extend(pointer());
    assert_eq!(filter.feed(&input, true, now)?, input);
    let release = filter.release().ok_or("something to release")?;
    assert_eq!(
        release,
        [vec![4, 0, 0, 0, 0, 0, 0xff, 0xe1], vec![5, 0, 0, 10, 0, 20]].concat()
    );
    assert_eq!(filter.release(), None, "released once");
    // A viewer that doesn't hold the screen presses nothing: its input is dropped, never tracked.
    assert!(filter.feed(&shift, false, now)?.is_empty());
    assert_eq!(filter.release(), None);
    Ok(())
}
