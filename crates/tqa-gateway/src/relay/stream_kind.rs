//! Whether an upstream answer is an event stream (GW-U-062). Some endpoints send a proper event stream labelled
//! `text/plain`; such a body is still decoded and metered, and the probe warns `stream_mislabelled`. Only `text/plain`
//! is sniffed: after any blank or `:` comment lines, its first line must be an `event:` or `data:` field. The first
//! [`SNIFF_BYTES`] decide; a body still undecided there, or ending undecided, is not a stream.

/// What an answer's content type and first bytes say it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamKind {
    /// Labelled `text/event-stream`.
    EventStream,
    /// Labelled `text/plain`, but its body starts like an event stream.
    Mislabelled,
    /// Anything else.
    Other,
}

impl StreamKind {
    /// Whether the body is decoded as an event stream.
    pub(super) const fn is_stream(self) -> bool {
        !matches!(self, Self::Other)
    }
}

/// How many leading bytes of a text/plain body may be buffered to decide.
pub(super) const SNIFF_BYTES: usize = 1024;

/// Classifies an answer by its media type and, for `text/plain` only, its first bytes taken as the whole start.
pub(super) fn classify(content_type: Option<&str>, first: &[u8]) -> StreamKind {
    match content_type
        .and_then(|value| value.split(';').next())
        .map(str::trim)
    {
        Some("text/event-stream") => StreamKind::EventStream,
        Some("text/plain") => {
            let mut sniffer = Sniffer::default();
            sniffer.push(first).unwrap_or_else(|| sniffer.finish())
        }
        _ => StreamKind::Other,
    }
}

/// Decides a text/plain body from its first bytes, chunk by chunk; the chunks themselves pass on unchanged.
#[derive(Debug, Default)]
pub(super) struct Sniffer {
    buffered: Vec<u8>,
}

impl Sniffer {
    /// Adds a chunk; `Some` once decided (from then on, pushing more changes nothing).
    pub(super) fn push(&mut self, chunk: &[u8]) -> Option<StreamKind> {
        let room = SNIFF_BYTES.saturating_sub(self.buffered.len());
        self.buffered
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
        decide(&self.buffered)
            .or_else(|| (self.buffered.len() >= SNIFF_BYTES).then_some(StreamKind::Other))
    }

    /// The body ended: what is buffered decides, and an undecided start is not a stream.
    pub(super) fn finish(&self) -> StreamKind {
        decide(&self.buffered).unwrap_or(StreamKind::Other)
    }
}

/// The decision a body's start allows, or `None` while its first field line may still become `event:` or `data:`.
fn decide(mut rest: &[u8]) -> Option<StreamKind> {
    const FIELDS: [&[u8]; 2] = [b"event:", b"data:"];
    loop {
        let (line, complete) = match rest.iter().position(|byte| *byte == b'\n') {
            Some(end) => (&rest[..end], Some(end)),
            None => (rest, None),
        };
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if FIELDS.iter().any(|field| line.starts_with(field)) {
            return Some(StreamKind::Mislabelled);
        }
        match complete {
            // A blank or comment line: look at the next one.
            Some(end) if line.is_empty() || line.starts_with(b":") => rest = &rest[end + 1..],
            // An unfinished line waits if it may still become a field or is a comment.
            None if line.starts_with(b":")
                || FIELDS.iter().any(|field| field.starts_with(line)) =>
            {
                return None;
            }
            _ => return Some(StreamKind::Other),
        }
    }
}

/// Whether a content type is the one kind that is sniffed.
pub(super) fn sniffed(content_type: Option<&str>) -> bool {
    content_type
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        == Some("text/plain")
}
