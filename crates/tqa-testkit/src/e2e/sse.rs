//! Server-sent event decoding for the conversation stream: `data:` lines carry one JSON frame,
//! `id:` repeats its sequence, and comment lines are keep-alives.
use tqa_types::ServerFrame;

/// Largest buffered event; a real frame is at most a tool result plus its envelope.
const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;

/// Why the stream could not be decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SseError {
    /// An event exceeded [`MAX_EVENT_BYTES`] without ending.
    TooLarge,
    /// An event's data was not UTF-8 or not a server frame.
    Malformed,
}

impl std::fmt::Display for SseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::TooLarge => "a stream event exceeded its size bound",
            Self::Malformed => "a stream event was not a server frame",
        })
    }
}
impl std::error::Error for SseError {}

/// Incremental decoder over arbitrary chunk boundaries.
#[derive(Debug, Default)]
pub struct SseDecoder {
    pending: Vec<u8>,
}
impl SseDecoder {
    /// Feeds one chunk and returns every frame it completed, in order.
    ///
    /// # Errors
    /// [`SseError`] for an oversized or malformed event; the stream must then be abandoned.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<ServerFrame>, SseError> {
        self.pending.extend_from_slice(chunk);
        let mut frames = Vec::new();
        while let Some(end) = event_end(&self.pending) {
            let event: Vec<u8> = self.pending.drain(..end.0).collect();
            self.pending.drain(..end.1);
            let text = std::str::from_utf8(&event).map_err(|_| SseError::Malformed)?;
            let data: Vec<&str> = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|value| value.strip_prefix(' ').unwrap_or(value))
                .collect();
            if !data.is_empty() {
                let frame =
                    serde_json::from_str(&data.join("\n")).map_err(|_| SseError::Malformed)?;
                frames.push(frame);
            }
        }
        if self.pending.len() > MAX_EVENT_BYTES {
            return Err(SseError::TooLarge);
        }
        Ok(frames)
    }
}

/// The event's length and the length of its blank-line terminator, `\n\n` or `\r\n\r\n`.
fn event_end(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .map(|at| (at, 2));
    let crlf = bytes
        .windows(4)
        .position(|quad| quad == b"\r\n\r\n")
        .map(|at| (at, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (one, other) => one.or(other),
    }
}

#[cfg(test)]
mod tests;
