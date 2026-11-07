//! Incremental, bounded SSE framing shared by the protocol adapters; UTF-8 is decoded only after
//! a complete line arrives. Each protocol maps data records to chunks through its [`Wire`].

mod stream;

pub(crate) use stream::response_stream;

use crate::{LlmError, StreamChunk, StreamLimitKind};
use bytes::{Buf as _, Bytes};

/// A protocol's mapping from SSE data records to provider-independent chunks.
pub(crate) trait Wire: Send {
    /// Maps one complete data record.
    fn data(&mut self, data: &str) -> Result<Vec<StreamChunk>, LlmError>;
    /// Whether the terminal record arrived; nothing after it is read.
    fn done(&self) -> bool;
}

pub(crate) struct Decoder<W> {
    line: Vec<u8>,
    data: String,
    event_bytes: usize,
    maximum: usize,
    wire: W,
    pub done: bool,
}

#[cfg(test)]
impl<W: Wire + Default> Decoder<W> {
    pub fn new(maximum: usize) -> Self {
        Self::with_wire(maximum, W::default())
    }
}

impl<W: Wire> Decoder<W> {
    pub fn with_wire(maximum: usize, wire: W) -> Self {
        Self {
            line: Vec::new(),
            data: String::new(),
            event_bytes: 0,
            maximum,
            wire,
            done: false,
        }
    }

    pub fn decode(&mut self, input: &mut Bytes) -> Result<Vec<StreamChunk>, LlmError> {
        while input.has_remaining() && !self.done {
            if self.event_bytes >= self.maximum {
                return Err(LlmError::StreamLimit {
                    which: StreamLimitKind::SseEvent,
                    max: self.maximum,
                });
            }
            self.event_bytes += 1;
            let byte = input.get_u8();
            if byte != b'\n' {
                self.line.push(byte);
                continue;
            }
            if let Some(chunks) = self.complete_line()? {
                return Ok(chunks);
            }
        }
        Ok(Vec::new())
    }

    fn complete_line(&mut self) -> Result<Option<Vec<StreamChunk>>, LlmError> {
        let line = std::mem::take(&mut self.line);
        let line = std::str::from_utf8(&line)
            .map_err(|_| LlmError::Malformed("SSE line contains invalid UTF-8".into()))?
            .trim_end_matches('\r');
        if line.is_empty() {
            self.event_bytes = 0;
            if self.data.is_empty() {
                return Ok(None);
            }
            let data = std::mem::take(&mut self.data);
            let chunks = self.wire.data(&data)?;
            self.done = self.wire.done();
            return Ok(Some(chunks));
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        if field == "data" {
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
        Ok(None)
    }
}
