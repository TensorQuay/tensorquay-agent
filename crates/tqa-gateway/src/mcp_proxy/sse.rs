//! A bounded server-sent-events framer for the MCP proxy (ADR-0031 decision 6): the remote's event stream is cut into
//! whole events, none larger than one message, so each one can be checked (and a `tools/list` result filtered) before
//! it is passed on. Pure, so every rule is a unit test (GW-U-039).

/// One event: its fields as the stream carried them. Comments and unknown fields are dropped.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Event {
    /// The `event` field: the event's type.
    pub kind: Option<String>,
    pub id: Option<String>,
    pub retry: Option<String>,
    /// The `data` lines joined with `\n`, as the specification defines.
    pub data: String,
}

impl Event {
    /// The event in wire form; multi-line data becomes one `data:` line per line.
    pub(crate) fn encode(&self) -> String {
        let mut out = String::new();
        if let Some(event) = &self.kind {
            out.push_str("event: ");
            out.push_str(event);
            out.push('\n');
        }
        if let Some(id) = &self.id {
            out.push_str("id: ");
            out.push_str(id);
            out.push('\n');
        }
        if let Some(retry) = &self.retry {
            out.push_str("retry: ");
            out.push_str(retry);
            out.push('\n');
        }
        for line in self.data.split('\n') {
            out.push_str("data: ");
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
        out
    }
}

/// The unfinished event grew past the bound.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct TooLarge;

/// Cuts a byte stream into events of at most `limit` bytes each (counting the unfinished event's buffered bytes).
pub(crate) struct Framer {
    limit: usize,
    /// Bytes of the current line not yet ended.
    line: Vec<u8>,
    /// The event being assembled, and how many bytes it has taken so far.
    current: Event,
    has_data: bool,
    size: usize,
}

impl Framer {
    pub(crate) const fn new(limit: usize) -> Self {
        Self {
            limit,
            line: Vec::new(),
            current: Event {
                kind: None,
                id: None,
                retry: None,
                data: String::new(),
            },
            has_data: false,
            size: 0,
        }
    }

    /// Feeds bytes; returns the events they complete.
    ///
    /// # Errors
    /// [`TooLarge`] once an event (or one line of it) exceeds the bound; the stream should then end.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Event>, TooLarge> {
        let mut events = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.line);
                events.extend(self.line_ended(&line));
                continue;
            }
            self.line.push(byte);
            self.size += 1;
            if self.size > self.limit {
                return Err(TooLarge);
            }
        }
        Ok(events)
    }

    /// One line ended (a trailing CR is dropped); a blank line ends the event.
    fn line_ended(&mut self, line: &[u8]) -> Option<Event> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(b":") {
            return None;
        }
        let text = String::from_utf8_lossy(line);
        let (field, value) = match text.split_once(':') {
            Some((field, value)) => (
                field.to_owned(),
                value.strip_prefix(' ').unwrap_or(value).to_owned(),
            ),
            None => (text.into_owned(), String::new()),
        };
        match field.as_str() {
            "data" => {
                if self.has_data {
                    self.current.data.push('\n');
                }
                self.current.data.push_str(&value);
                self.has_data = true;
            }
            "event" => self.current.kind = Some(value),
            "id" if !value.contains('\0') => self.current.id = Some(value),
            "retry" if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                self.current.retry = Some(value);
            }
            _ => {}
        }
        None
    }

    /// Emits the assembled event when it carried data; an event without data is dropped, as the specification says.
    fn dispatch(&mut self) -> Option<Event> {
        self.size = 0;
        let event = std::mem::take(&mut self.current);
        let had_data = std::mem::replace(&mut self.has_data, false);
        had_data.then_some(event)
    }
}

#[cfg(test)]
#[path = "sse_tests.rs"]
mod tests;
