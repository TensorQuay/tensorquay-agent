//! GW-U-107 (ADR-0037 §6, response scrubbing): a key's value and its common encodings are replaced in answers, in
//! headers and across any split of a streamed body.
//! GW-U-121 (ADR-0037 §6): the shared scrubbers, one per person and address, forgotten with a key's change,
//! bounded.
use super::{Scrubber, Scrubbers};
use base64::Engine as _;
use proptest::prelude::*;

const VALUE: &str = "s3cr3t/value+=?";
const PLACEHOLDER: &str = "tqa_ph_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";

fn scrubber() -> Scrubber {
    let mut scrubber = Scrubber::default();
    scrubber.learn(VALUE.as_bytes(), PLACEHOLDER, Some("x-access-token"));
    scrubber
}

fn basic(password: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{password}"))
}

/// The body as the scrubber streams it, cut into `cuts`.
fn streamed(scrubber: &Scrubber, body: &[u8], cuts: &[usize]) -> Vec<u8> {
    let mut stream = scrubber.stream();
    let mut out = Vec::new();
    let mut start = 0;
    for &cut in cuts.iter().chain(std::iter::once(&body.len())) {
        let cut = cut.clamp(start, body.len());
        out.extend(stream.push(&body[start..cut]));
        start = cut;
    }
    out.extend(stream.finish());
    out
}

// TEST: GW-U-107
#[test]
fn gw_u_107_the_value_and_its_encodings_become_the_placeholder() {
    let scrubber = scrubber();
    let url = "s3cr3t%2Fvalue%2B%3D%3F";
    let body = format!("a {VALUE} b {url} c {} d", basic(VALUE));
    let expected = format!("a {PLACEHOLDER} b {PLACEHOLDER} c {} d", basic(PLACEHOLDER));
    assert_eq!(scrubber.header(body.as_bytes()), expected.as_bytes());
    assert_eq!(
        streamed(&scrubber, body.as_bytes(), &[]),
        expected.as_bytes()
    );
    // Every split of the body, a byte at a time included, scrubs the same.
    for cut in 0..body.len() {
        assert_eq!(
            streamed(&scrubber, body.as_bytes(), &[cut]),
            expected.as_bytes(),
            "cut at {cut}"
        );
    }
    let bytes: Vec<usize> = (1..body.len()).collect();
    assert_eq!(
        streamed(&scrubber, body.as_bytes(), &bytes),
        expected.as_bytes()
    );
    // Nothing learned yet, or nothing to find: the body is unchanged, held back only as long as it must be.
    assert_eq!(streamed(&Scrubber::default(), b"plain", &[2]), b"plain");
    assert_eq!(scrubber.header(b"no secret here"), b"no secret here");
    // Learned twice, or a second key: each is scrubbed.
    let mut two = scrubber.clone();
    two.learn(
        b"other-value-1",
        "tqa_ph_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefg",
        None,
    );
    two.learn(VALUE.as_bytes(), PLACEHOLDER, Some("x-access-token"));
    assert_eq!(
        streamed(&two, format!("{VALUE}|other-value-1").as_bytes(), &[5, 20]),
        format!("{PLACEHOLDER}|tqa_ph_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefg").as_bytes()
    );
}

proptest! {
    // TEST: GW-U-107
    /// Whatever surrounds the value and wherever the body is cut, no copy of the value is left.
    #[test]
    fn gw_u_107_no_split_leaves_the_value(
        before in "[ -~]{0,40}",
        after in "[ -~]{0,40}",
        cuts in proptest::collection::vec(0usize..120, 0..6),
    ) {
        let scrubber = scrubber();
        let body = format!("{before}{VALUE}{after}{VALUE}");
        let mut cuts = cuts;
        cuts.sort_unstable();
        let out = streamed(&scrubber, body.as_bytes(), &cuts);
        let text = String::from_utf8_lossy(&out);
        prop_assert!(!text.contains(VALUE), "{}", text);
        prop_assert_eq!(out, scrubber.header(body.as_bytes()));
    }
}

// TEST: GW-U-121
#[test]
fn gw_u_121_a_persons_connections_to_an_address_share_what_their_swaps_taught()
-> Result<(), &'static str> {
    let scrubbers = Scrubbers::default();
    let (person, other) = (uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(2));
    let first = scrubbers.of(person, "api.example", 443);
    first
        .lock()
        .map_err(|_| "lock")?
        .learn(VALUE.as_bytes(), PLACEHOLDER, None);
    let shared = |who, host, port| -> Result<bool, &'static str> {
        Ok(!scrubbers
            .of(who, host, port)
            .lock()
            .map_err(|_| "lock")?
            .is_empty())
    };
    assert!(
        shared(person, "api.example", 443)?,
        "a later connection of theirs"
    );
    assert!(!shared(other, "api.example", 443)?, "another person's");
    assert!(!shared(person, "api.example", 8443)?, "another port");
    assert!(!shared(person, "other.example", 443)?, "another host");
    scrubbers.forget("api.example", 443);
    assert!(
        !shared(person, "api.example", 443)?,
        "forgotten with the key's change"
    );
    assert!(
        !first.lock().map_err(|_| "lock")?.is_empty(),
        "an open connection keeps what it holds"
    );
    Ok(())
}

// TEST: GW-U-121
#[test]
fn gw_u_121_the_shared_scrubbers_are_bounded_and_drop_the_unheld_first() -> Result<(), &'static str>
{
    let scrubbers = Scrubbers::default();
    let person = uuid::Uuid::from_u128(1);
    let held = scrubbers.of(person, "held.example", 443);
    held.lock()
        .map_err(|_| "lock")?
        .learn(VALUE.as_bytes(), PLACEHOLDER, None);
    for port in 1..super::MAX_SCRUBBERS {
        let _unheld = scrubbers.of(
            person,
            "unheld.example",
            u16::try_from(port).map_err(|_| "a port")?,
        );
    }
    // Full: a new address drops every entry no connection holds, and the held one stays.
    let fresh = scrubbers.of(person, "new.example", 443);
    assert!(fresh.lock().map_err(|_| "lock")?.is_empty());
    assert!(
        !scrubbers
            .of(person, "held.example", 443)
            .lock()
            .map_err(|_| "lock")?
            .is_empty()
    );
    assert_eq!(scrubbers.len(), 2, "the held one and the new one");
    Ok(())
}

// TEST: GW-U-107
/// The value's other common spellings in an answer are scrubbed too: percent-encoding in
/// lowercase hex, JSON's escaped slash, and the value's own base64 (standard and URL-safe).
#[test]
fn gw_u_107_the_values_other_spellings_are_scrubbed() {
    let scrubber = scrubber();
    let lower = super::url_encoded(VALUE.as_bytes()).to_ascii_lowercase();
    let spellings = [
        lower,
        VALUE.replace('/', "\\/").into_bytes(),
        base64::engine::general_purpose::STANDARD
            .encode(VALUE)
            .into_bytes(),
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(VALUE)
            .into_bytes(),
    ];
    for spelling in spellings {
        let mut answer = b"before ".to_vec();
        answer.extend_from_slice(&spelling);
        answer.extend_from_slice(b" after");
        let cleaned = scrubber.header(&answer);
        assert!(
            !cleaned
                .windows(spelling.len())
                .any(|window| window == spelling.as_slice()),
            "{}",
            String::from_utf8_lossy(&spelling)
        );
    }
}

// TEST: GW-U-107
/// A header whose scrubbed value isn't a valid header value is dropped, never sent on as it came (fail closed).
#[test]
fn gw_u_107_a_header_that_cant_be_scrubbed_is_dropped() -> Result<(), Box<dyn std::error::Error>> {
    let mut scrubber = Scrubber::default();
    // A replacement no header may hold, so the scrubbed value can't be one.
    scrubber.learn(VALUE.as_bytes(), "line\nbreak", None);
    let mut headers = hyper::HeaderMap::new();
    headers.insert("x-echo", format!("you sent {VALUE}").parse()?);
    headers.insert("x-other", "kept".parse()?);
    scrubber.headers(&mut headers);
    assert!(headers.get("x-echo").is_none(), "{headers:?}");
    assert_eq!(
        headers
            .get("x-other")
            .map(hyper::header::HeaderValue::as_bytes),
        Some(b"kept".as_slice())
    );
    Ok(())
}

// TEST: GW-U-121
/// A swap's value is learned into its shared scrubber, or the swap doesn't happen: a poisoned lock reports that it
/// couldn't learn, so the proxy refuses rather than sending a value it couldn't scrub back.
#[test]
fn gw_u_121_a_value_that_cant_be_learned_is_reported() -> Result<(), &'static str> {
    let shared = std::sync::Arc::new(std::sync::Mutex::new(Scrubber::default()));
    assert!(super::learn_shared(
        &shared,
        VALUE.as_bytes(),
        PLACEHOLDER,
        None
    ));
    assert!(!shared.lock().map_err(|_| "lock")?.is_empty());
    let poisoned = std::sync::Arc::new(std::sync::Mutex::new(Scrubber::default()));
    let holder = poisoned.clone();
    let _ = std::thread::spawn(move || {
        let _held = holder.lock();
        // Unwinds while holding the lock, which poisons it.
        std::panic::resume_unwind(Box::new("poisons the lock"));
    })
    .join();
    assert!(poisoned.is_poisoned());
    assert!(!super::learn_shared(
        &poisoned,
        VALUE.as_bytes(),
        PLACEHOLDER,
        None
    ));
    Ok(())
}

/// Whatever of `text` decodes as base64 in either alphabet, at each of the four character phases.
fn decodings(text: &[u8]) -> Vec<Vec<u8>> {
    let mut found = Vec::new();
    for engine in [
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        for phase in 0..4 {
            let decoded = text
                .get(phase..)
                .unwrap_or_default()
                .chunks_exact(4)
                .filter_map(|chunk| engine.decode(chunk).ok())
                .flatten()
                .collect();
            found.push(decoded);
        }
    }
    found
}

// TEST: GW-U-107
/// The value inside a longer base64 blob (an echo of the request, encoded whole) is scrubbed at each of the three
/// byte alignments, in both alphabets: no eight bytes of it decode from the answer.
#[test]
fn gw_u_107_the_value_inside_a_longer_base64_blob_is_scrubbed_at_any_alignment() {
    let scrubber = scrubber();
    for engine in [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::URL_SAFE,
    ] {
        for offset in 0..3 {
            let echo = format!(
                "{}{{\"headers\":{{\"authorization\":\"Bearer {VALUE}\"}},\"ok\":true}}",
                "x".repeat(offset)
            );
            let answer = engine.encode(&echo).into_bytes();
            let cleaned = scrubber.header(&answer);
            for decoded in decodings(&cleaned) {
                assert!(
                    !decoded
                        .windows(8)
                        .any(|window| VALUE.as_bytes().windows(8).any(|part| part == window)),
                    "offset {offset}: {}",
                    String::from_utf8_lossy(&cleaned)
                );
            }
        }
    }
}

// TEST: GW-U-107
/// A value too short for its fragment inside a blob to be told from unrelated text learns no fragment: look-alike
/// data passes unchanged.
#[test]
fn gw_u_107_a_short_values_fragment_doesnt_scrub_look_alike_text() {
    let mut scrubber = Scrubber::default();
    scrubber.learn(b"abcdefghi", PLACEHOLDER, None);
    // "FiY2RlZmdoa": the characters only the value decides when it starts at byte 1 of a blob.
    let text = b"id=FiY2RlZmdoa7 next=AFiY2RlZmdoaQ".to_vec();
    assert_eq!(scrubber.header(&text), text);
}

// TEST: GW-U-121
/// Under the bound, a scrubber no connection holds any more is kept for the person's next connection to that address,
/// whatever other addresses they reach meanwhile; only a full map drops the unheld.
#[test]
fn gw_u_121_under_the_bound_an_unheld_scrubber_is_kept() -> Result<(), &'static str> {
    let scrubbers = Scrubbers::default();
    let person = uuid::Uuid::from_u128(1);
    scrubbers
        .of(person, "api.example", 443)
        .lock()
        .map_err(|_| "lock")?
        .learn(VALUE.as_bytes(), PLACEHOLDER, None);
    let _elsewhere = scrubbers.of(person, "other.example", 443);
    assert_eq!(scrubbers.len(), 2);
    assert!(
        !scrubbers
            .of(person, "api.example", 443)
            .lock()
            .map_err(|_| "lock")?
            .is_empty(),
        "the closed connection's values are kept for the next one"
    );
    Ok(())
}

// TEST: GW-U-121
/// A full map never grows past the bound, and an address it already holds is found there, unheld or not: only a new
/// address makes room.
#[test]
fn gw_u_121_a_full_map_stays_at_the_bound_and_finds_what_it_holds() -> Result<(), &'static str> {
    let scrubbers = Scrubbers::default();
    let person = uuid::Uuid::from_u128(1);
    scrubbers
        .of(person, "known.example", 443)
        .lock()
        .map_err(|_| "lock")?
        .learn(VALUE.as_bytes(), PLACEHOLDER, None);
    for port in 1..super::MAX_SCRUBBERS {
        let _unheld = scrubbers.of(
            person,
            "unheld.example",
            u16::try_from(port).map_err(|_| "a port")?,
        );
    }
    assert_eq!(scrubbers.len(), super::MAX_SCRUBBERS);
    assert!(
        !scrubbers
            .of(person, "known.example", 443)
            .lock()
            .map_err(|_| "lock")?
            .is_empty(),
        "an address already held is found, not dropped to make room"
    );
    assert_eq!(
        scrubbers.len(),
        super::MAX_SCRUBBERS,
        "nothing dropped for it"
    );
    let mut held = Vec::new();
    for port in 1..=super::MAX_SCRUBBERS {
        held.push(scrubbers.of(
            person,
            "held.example",
            u16::try_from(port).map_err(|_| "a port")?,
        ));
        assert!(
            scrubbers.len() <= super::MAX_SCRUBBERS,
            "{} scrubbers after {port} new addresses",
            scrubbers.len()
        );
    }
    Ok(())
}

// TEST: GW-U-121
/// A key's change forgets only its own host and port: the same host's other ports and the same port on other hosts
/// keep what they learned.
#[test]
fn gw_u_121_forgetting_an_address_keeps_the_others() -> Result<(), &'static str> {
    let scrubbers = Scrubbers::default();
    let person = uuid::Uuid::from_u128(1);
    let addresses = [
        ("api.example", 443),
        ("api.example", 8443),
        ("other.example", 443),
    ];
    for (host, port) in addresses {
        scrubbers
            .of(person, host, port)
            .lock()
            .map_err(|_| "lock")?
            .learn(VALUE.as_bytes(), PLACEHOLDER, None);
    }
    scrubbers.forget("api.example", 443);
    for (host, port) in addresses {
        let learned = !scrubbers
            .of(person, host, port)
            .lock()
            .map_err(|_| "lock")?
            .is_empty();
        assert_eq!(
            learned,
            (host, port) != ("api.example", 443),
            "{host}:{port}"
        );
    }
    Ok(())
}

// TEST: GW-U-107
/// Each spelling cut at every byte of a streamed body comes out as the whole body scrubbed at once: a needle split
/// across two chunks, one byte short of its end included, is still replaced whole.
#[test]
fn gw_u_107_a_needle_cut_at_any_byte_is_still_replaced() {
    let scrubber = scrubber();
    for (needle, _) in &scrubber.pairs {
        let mut body = b"before ".to_vec();
        body.extend_from_slice(needle);
        body.extend_from_slice(b" after");
        let whole = scrubber.header(&body);
        for cut in 0..=body.len() {
            assert_eq!(
                streamed(&scrubber, &body, &[cut]),
                whole,
                "cut at {cut} of a {}-byte needle",
                needle.len()
            );
        }
    }
}

// TEST: GW-U-107
/// A chunk is held back only as far as the longest needle could still need: the rest goes out at once, so a slow stream
/// (server-sent events) isn't stalled.
#[test]
fn gw_u_107_a_chunk_holds_back_only_what_a_needle_could_still_need() {
    let scrubber = scrubber();
    let longest = scrubber.pairs.first().map_or(0, |(needle, _)| needle.len());
    let chunk = vec![b'.'; 3 * longest];
    let mut stream = scrubber.stream();
    assert_eq!(stream.push(&chunk).len(), chunk.len() - (longest - 1));
    assert_eq!(stream.finish().len(), longest - 1);
}

// TEST: GW-U-107
/// The value inside a longer base64 blob is scrubbed whatever byte stands before it: the fragment learned leaves out the
/// characters it shares with its neighbour.
#[test]
fn gw_u_107_the_value_in_a_blob_is_scrubbed_after_any_neighbouring_byte() {
    let scrubber = scrubber();
    for before in ["token=", "\"", ":", "Bearer "] {
        for offset in 0..3 {
            let echo = format!("{}{before}{VALUE}\"}}", "x".repeat(offset));
            let answer = base64::engine::general_purpose::STANDARD
                .encode(&echo)
                .into_bytes();
            let cleaned = scrubber.header(&answer);
            for decoded in decodings(&cleaned) {
                assert!(
                    !decoded
                        .windows(8)
                        .any(|window| VALUE.as_bytes().windows(8).any(|part| part == window)),
                    "{before:?} at offset {offset}: {}",
                    String::from_utf8_lossy(&cleaned)
                );
            }
        }
    }
}
