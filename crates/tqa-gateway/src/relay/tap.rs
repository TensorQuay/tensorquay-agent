use tqa_llm::{
    Protocol, Usage,
    relay_shaping::{StreamedBytes, UsageReader},
};

/// Frames the relayed SSE bytes into events for the protocol's usage reader.
pub(super) struct UsageTap {
    reader: UsageReader,
    maximum: usize,
    line: Vec<u8>,
    data: Vec<u8>,
    dropping: bool,
    first_line: bool,
    skip_lf: bool,
    line_bytes: usize,
}
impl UsageTap {
    pub fn new(protocol: Protocol, maximum: usize) -> Self {
        Self {
            reader: UsageReader::new(protocol),
            maximum,
            line: Vec::new(),
            data: Vec::new(),
            dropping: false,
            first_line: true,
            skip_lf: false,
            line_bytes: 0,
        }
    }
    /// The usage reported so far; Messages combines several events, the others keep the latest.
    pub fn latest(&self) -> Option<Usage> {
        self.reader.usage()
    }
    /// What the response streamed so far, for an estimate when no usage arrives.
    pub fn streamed(&self) -> StreamedBytes {
        self.reader.streamed()
    }
    pub fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if std::mem::take(&mut self.skip_lf) && byte == b'\n' {
                continue;
            }
            if matches!(byte, b'\n' | b'\r') {
                self.end_line();
                self.skip_lf = byte == b'\r';
                continue;
            }
            self.line_bytes = self.line_bytes.saturating_add(1);
            if self.line.len() + self.data.len() < self.maximum {
                self.line.push(byte);
            } else {
                self.dropping = true;
            }
        }
    }
    fn end_line(&mut self) {
        if std::mem::take(&mut self.first_line) && self.line.starts_with(b"\xef\xbb\xbf") {
            self.line.drain(..3);
            self.line_bytes = self.line_bytes.saturating_sub(3);
        }
        if self.line_bytes == 0 {
            if !self.dropping
                && let Ok(data) = std::str::from_utf8(&self.data)
            {
                self.reader.observe(data);
            }
            self.data.clear();
            self.dropping = false;
        } else if !self.dropping
            && let Some(data) = self.line.strip_prefix(b"data:")
        {
            let data = data.strip_prefix(b" ").unwrap_or(data);
            self.data.extend_from_slice(data);
            self.data.push(b'\n');
        }
        self.line.clear();
        self.line_bytes = 0;
    }
}
