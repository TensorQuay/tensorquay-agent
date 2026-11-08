//! Response scrubbing on keyed connections (ADR-0037 §6): a key's value, its URL encoding and, for a basic key, the
//! base64 of `user:value` are replaced with the placeholder (base64 of `user:placeholder`) in answers' headers and
//! bodies. Bodies stream: only the bytes that could still begin a match are held back. Defence in depth, not a
//! guarantee (the ADR's T1).
use base64::Engine as _;
use secrecy::zeroize::Zeroizing;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};
use uuid::Uuid;

/// A person and the host and port they reach.
type Address = (Uuid, String, u16);
/// A scrubber the connections of one person to one address share.
type Shared = Arc<Mutex<Scrubber>>;

/// The most people and addresses with learned values held at once; past it, those no connection holds go first.
const MAX_SCRUBBERS: usize = 4096;

/// The fewest base64 characters (96 bits) a value's fragment inside a longer blob must have to be learned: a shorter
/// one could match unrelated text.
const MIN_FRAGMENT: usize = 16;

/// The values a person's swaps taught, by person, host and port: every keyed connection of that person to that address
/// scrubs them from its first byte, whichever of their runs or tasks swapped them. Only in
/// memory, bounded; a key's change or deletion forgets its address. What it can't cover: a host sending back a value it
/// stored before that person's first swap since the gateway started (ADR-0037 §6).
#[derive(Default)]
pub(crate) struct Scrubbers(Mutex<HashMap<Address, Shared>>);

impl Scrubbers {
    /// The scrubber `person`'s keyed connections to `host:port` share. Past the bound, with every entry in use, a
    /// connection gets one of its own (it still learns its own swaps).
    pub(super) fn of(&self, person: Uuid, host: &str, port: u16) -> Shared {
        let mut shared = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let key = (person, host.to_owned(), port);
        if shared.len() >= MAX_SCRUBBERS && !shared.contains_key(&key) {
            shared.retain(|_, scrubber| Arc::strong_count(scrubber) > 1);
            if shared.len() >= MAX_SCRUBBERS {
                return Arc::default();
            }
        }
        shared.entry(key).or_default().clone()
    }

    /// How many people and addresses have a shared scrubber (tests).
    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// Forgets the values learned for `host:port`, everyone's: a key for it changed or went. Connections open now keep
    /// what they hold until they close.
    pub(crate) fn forget(&self, host: &str, port: u16) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(_, known, at), _| !(known == host && *at == port));
    }
}

/// What to find and what to put in its place, the longest needle first.
#[derive(Clone, Default)]
pub(super) struct Scrubber {
    pairs: Vec<(Zeroizing<Vec<u8>>, Vec<u8>)>,
}

impl Scrubber {
    /// Learns a key's `value`, used as `placeholder`, and for a basic key its `user`.
    pub(super) fn learn(&mut self, value: &[u8], placeholder: &str, user: Option<&str>) {
        if value.is_empty() {
            return;
        }
        let mut add = |needle: Vec<u8>, replacement: Vec<u8>| {
            if !self
                .pairs
                .iter()
                .any(|(known, _)| known.as_slice() == needle.as_slice())
            {
                self.pairs.push((Zeroizing::new(needle), replacement));
            }
        };
        add(value.to_vec(), placeholder.as_bytes().to_vec());
        add(url_encoded(value), placeholder.as_bytes().to_vec());
        // Its other common spellings: lowercase percent-encoding, JSON's escaped slash, and
        // its own base64, each replaced with the placeholder in the same spelling. Inside a longer base64 blob (an echo
        // of the request, encoded whole) the value sits at one of three byte alignments: the characters only it decides
        // at each, when there are enough of them, become the placeholder's at the same alignment.
        add(
            lowercase_hex(&url_encoded(value)),
            placeholder.as_bytes().to_vec(),
        );
        add(json_slashes(value), placeholder.as_bytes().to_vec());
        for engine in [
            &base64::engine::general_purpose::STANDARD,
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        ] {
            add(
                engine.encode(value).into_bytes(),
                engine.encode(placeholder).into_bytes(),
            );
            for (needle, replacement) in fragments(engine, value, placeholder.as_bytes()) {
                add(needle, replacement);
            }
        }
        if let Some(user) = user {
            let encode = |secret: &[u8]| {
                let mut pair = Zeroizing::new(format!("{user}:").into_bytes());
                pair.extend_from_slice(secret);
                base64::engine::general_purpose::STANDARD.encode(pair.as_slice())
            };
            add(
                encode(value).into_bytes(),
                encode(placeholder.as_bytes()).into_bytes(),
            );
        }
        self.pairs
            .sort_by_key(|(needle, _)| std::cmp::Reverse(needle.len()));
    }

    /// Whether there is nothing to scrub.
    pub(super) fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// An answer's headers, each value scrubbed; a header whose scrubbed value isn't a valid one is dropped, every
    /// value of its name, never sent on as it came (fail closed).
    pub(super) fn headers(&self, headers: &mut hyper::HeaderMap) {
        let mut unscrubbable = Vec::new();
        for (name, value) in headers.iter_mut() {
            match hyper::header::HeaderValue::from_bytes(&self.header(value.as_bytes())) {
                Ok(scrubbed) => *value = scrubbed,
                Err(_) => unscrubbable.push(name.clone()),
            }
        }
        for name in unscrubbable {
            headers.remove(&name);
        }
    }

    /// A whole value, such as a header's, scrubbed.
    pub(super) fn header(&self, bytes: &[u8]) -> Vec<u8> {
        self.replace(bytes, true).0
    }

    /// A streaming scrubber for one body.
    pub(super) fn stream(&self) -> Stream {
        Stream {
            scrubber: self.clone(),
            carry: Zeroizing::new(Vec::new()),
        }
    }

    /// Replaces every needle in `bytes`; unless `last`, stops where a needle could still begin past the end. Returns
    /// the output and how much of `bytes` it covers.
    fn replace(&self, bytes: &[u8], last: bool) -> (Vec<u8>, usize) {
        let hold = self.pairs.first().map_or(0, |(needle, _)| needle.len() - 1);
        let limit = if last {
            bytes.len()
        } else {
            bytes.len().saturating_sub(hold)
        };
        let mut out = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if let Some((needle, replacement)) = self
                .pairs
                .iter()
                .find(|(needle, _)| bytes[index..].starts_with(needle))
            {
                out.extend_from_slice(replacement);
                index += needle.len();
                continue;
            }
            if index >= limit {
                break;
            }
            out.push(bytes[index]);
            index += 1;
        }
        (out, index)
    }
}

/// One body's scrubbing, chunk by chunk.
pub(super) struct Stream {
    scrubber: Scrubber,
    carry: Zeroizing<Vec<u8>>,
}

impl Stream {
    /// The scrubbed bytes this chunk lets out; the rest waits for the next.
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.carry.extend_from_slice(chunk);
        let (out, used) = self.scrubber.replace(&self.carry, false);
        self.carry.drain(..used);
        out
    }

    /// What was held back, scrubbed, at the body's end.
    pub(super) fn finish(&mut self) -> Vec<u8> {
        let (out, _) = self.scrubber.replace(&self.carry, true);
        self.carry.clear();
        out
    }
}

/// `value` percent-encoded as a URL component (everything but the unreserved characters).
fn url_encoded(value: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(value.len() * 3);
    for &byte in value {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte);
        } else {
            encoded.extend_from_slice(format!("%{byte:02X}").as_bytes());
        }
    }
    encoded
}

/// Learns a swapped value into a connection's shared scrubber; whether it could. A poisoned lock can't learn, and the
/// swap is then refused, never sent unscrubbable.
pub(super) fn learn_shared(
    shared: &Mutex<Scrubber>,
    value: &[u8],
    placeholder: &str,
    user: Option<&str>,
) -> bool {
    let Ok(mut scrubber) = shared.lock() else {
        return false;
    };
    scrubber.learn(value, placeholder, user);
    true
}

/// A percent-encoding with its hex digits in lowercase, as some servers write them.
fn lowercase_hex(encoded: &[u8]) -> Vec<u8> {
    let mut out = encoded.to_vec();
    let mut index = 0;
    while index < out.len() {
        if out[index] == b'%' {
            for digit in out.iter_mut().skip(index + 1).take(2) {
                digit.make_ascii_lowercase();
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    out
}

/// A value as JSON writes it with its slashes escaped (`\/`).
fn json_slashes(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    for &byte in value {
        if byte == b'/' {
            out.push(b'\\');
        }
        out.push(byte);
    }
    out
}

/// For each of the three byte alignments, `value`'s fragment inside a longer blob and `placeholder`'s in its place;
/// none shorter than [`MIN_FRAGMENT`].
fn fragments(
    engine: &base64::engine::GeneralPurpose,
    value: &[u8],
    placeholder: &[u8],
) -> Vec<(Vec<u8>, Vec<u8>)> {
    (0..3)
        .map(|offset| {
            (
                fragment(engine, value, offset),
                fragment(engine, placeholder, offset),
            )
        })
        .filter(|(needle, _)| needle.len() >= MIN_FRAGMENT)
        .collect()
}

/// The base64 characters only `value` decides when it starts at byte `offset` (0 to 2) of a longer blob; those at its
/// edges, which mix with the neighbouring bytes, are left out.
fn fragment(engine: &base64::engine::GeneralPurpose, value: &[u8], offset: usize) -> Vec<u8> {
    let mut blob = Zeroizing::new(vec![0; offset]);
    blob.extend_from_slice(value);
    let encoded = Zeroizing::new(engine.encode(blob.as_slice()));
    let first = (offset * 8).div_ceil(6);
    let end = (offset + value.len()) * 8 / 6;
    encoded
        .as_bytes()
        .get(first..end)
        .unwrap_or_default()
        .to_vec()
}

#[cfg(test)]
#[path = "scrub_tests.rs"]
mod tests;
