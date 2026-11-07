//! GW-U-151: a screenshot upload put together (ADR-0039 section 9).
use super::{Step, drop_unfinished, feed};
use base64::Engine as _;
use sha2::Digest as _;
use std::time::{Duration, Instant};
use tqa_types::StoreRefusal;

/// A small PNG-headed image and its id.
fn image(len: usize) -> (Vec<u8>, String) {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(len, 7);
    let id = crate::hex::hex(&sha2::Sha256::digest(&png));
    (png, id)
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// TEST: GW-U-151
#[test]
fn gw_u_151_chunks_in_order_make_the_whole_png() {
    let now = Instant::now();
    let (png, id) = image(1000);
    let mut slot = None;
    assert_eq!(
        feed(&mut slot, (&id, 0, false, &b64(&png[..600])), now),
        Step::More
    );
    assert_eq!(
        feed(&mut slot, (&id, 1, true, &b64(&png[600..])), now),
        Step::Whole(png)
    );
    assert!(slot.is_none(), "done: nothing left in flight");
}

// TEST: GW-U-151
#[test]
fn gw_u_151_a_refused_upload_is_answered_once() {
    let now = Instant::now();
    let (png, id) = image(1000);
    let mut slot = None;
    // A chunk out of order.
    assert_eq!(
        feed(&mut slot, (&id, 1, false, &b64(&png[..600])), now),
        Step::Refused(StoreRefusal::StoreFailed)
    );
    assert_eq!(
        feed(&mut slot, (&id, 2, true, &b64(&png[600..])), now),
        Step::More,
        "the rest ignored"
    );
    assert!(slot.is_none());
    // Past 512 KiB: refused as soon as the total passes it.
    let big = vec![0_u8; 96 * 1024];
    let mut chunk = 0;
    let refused = loop {
        let step = feed(&mut slot, (&id, chunk, false, &b64(&big)), now);
        if step != Step::More {
            break step;
        }
        chunk += 1;
    };
    assert_eq!(refused, Step::Refused(StoreRefusal::TooLarge));
    assert_eq!(chunk, 5, "the sixth chunk passes 512 KiB");
}

// TEST: GW-U-151
#[test]
fn gw_u_151_not_a_png_or_not_its_id_is_a_bad_image() {
    let now = Instant::now();
    let (png, id) = image(100);
    let mut slot = None;
    let other = "0".repeat(64);
    assert_eq!(
        feed(&mut slot, (&other, 0, true, &b64(&png)), now),
        Step::Refused(StoreRefusal::BadImage),
        "the SHA-256 isn't its id"
    );
    let text = b"plain text, not an image";
    let text_id = crate::hex::hex(&sha2::Sha256::digest(text));
    assert_eq!(
        feed(&mut slot, (&text_id, 0, true, &b64(text)), now),
        Step::Refused(StoreRefusal::BadImage),
        "not a PNG"
    );
    assert_eq!(
        feed(&mut slot, (&id, 0, true, "!!!!"), now),
        Step::Refused(StoreRefusal::BadImage),
        "not base64"
    );
}

// TEST: GW-U-151
#[test]
fn gw_u_151_an_unfinished_upload_is_dropped_by_the_next() {
    let now = Instant::now();
    let (first, first_id) = image(1000);
    let (second, second_id) = image(900);
    let mut slot = None;
    assert_eq!(
        feed(&mut slot, (&first_id, 0, false, &b64(&first[..500])), now),
        Step::More
    );
    // Another id starts afresh.
    assert_eq!(
        feed(&mut slot, (&second_id, 0, true, &b64(&second)), now),
        Step::Whole(second)
    );
    // The same id, after 10 s without a chunk: started again from chunk 0.
    assert_eq!(
        feed(&mut slot, (&first_id, 0, false, &b64(&first[..500])), now),
        Step::More
    );
    let later = now + Duration::from_secs(11);
    assert_eq!(
        feed(&mut slot, (&first_id, 1, true, &b64(&first[500..])), later),
        Step::Refused(StoreRefusal::StoreFailed),
        "its first half was dropped"
    );
}

// TEST: GW-U-151
#[test]
fn gw_u_151_an_upload_left_unfinished_is_freed_after_ten_seconds()
-> Result<(), Box<dyn std::error::Error>> {
    let now = Instant::now();
    let (png, id) = image(1000);
    let mut slot = None;
    assert_eq!(
        feed(&mut slot, (&id, 0, false, &b64(&png[..500])), now),
        Step::More
    );
    let mut uploads = std::collections::HashMap::new();
    uploads.insert(
        "0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6b".parse()?,
        slot.ok_or("an upload in flight")?,
    );
    // A runtime that died mid-upload never sends the rest: kept for 10 s, then freed.
    drop_unfinished(&mut uploads, now + Duration::from_secs(10));
    assert_eq!(uploads.len(), 1);
    drop_unfinished(&mut uploads, now + Duration::from_secs(11));
    assert!(uploads.is_empty());
    Ok(())
}

// TEST: GW-U-151
#[test]
fn gw_u_151_the_same_id_begun_again_after_an_abandoned_upload_is_put_together_afresh() {
    // Two captures of an unchanged screen share an id. The runtime gave up on the first half way (its deadline), then
    // sends the second from chunk 0 within 10 s: it starts afresh, never refused as out of order.
    let now = Instant::now();
    let (png, id) = image(200 * 1024);
    let chunks: Vec<String> = png.chunks(96 * 1024).map(b64).collect();
    let mut slot = None;
    assert_eq!(
        feed(&mut slot, (&id, 0, false, &chunks[0]), now),
        Step::More
    );
    assert_eq!(
        feed(&mut slot, (&id, 1, false, &chunks[1]), now),
        Step::More
    );
    let later = now + Duration::from_secs(2);
    for (index, chunk) in chunks.iter().enumerate() {
        let last = index + 1 == chunks.len();
        let step = feed(
            &mut slot,
            (&id, u32::try_from(index).unwrap_or(u32::MAX), last, chunk),
            later,
        );
        if last {
            assert_eq!(step, Step::Whole(png.clone()));
        } else {
            assert_eq!(step, Step::More);
        }
    }
}
