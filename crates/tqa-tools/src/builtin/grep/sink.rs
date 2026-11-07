//! Retains only the current page's rows while finishing binary detection and counts.
use super::{Args, Mode};
use crate::ToolError;
use crate::builtin::search::{budget::Budget, page::Page};
use grep_searcher::{Searcher, Sink, SinkContext, SinkMatch};
use std::{io, sync::Arc, time::Instant};
use tokio_util::sync::CancellationToken;
use tqa_types::Clock;

const CONTENT_LINE_BYTES: usize = 500;
const TRUNCATED: &str = "[truncated]";
#[derive(Clone, Copy)]
pub(super) struct Capture {
    max_rows: usize,
    capacity: usize,
    row_bytes: usize,
}
impl Capture {
    pub(super) fn new(page: &Page) -> Self {
        let skip = usize::try_from(page.skip).unwrap_or(usize::MAX);
        let row_bytes = CONTENT_LINE_BYTES.min(page.content_bytes);
        Self {
            max_rows: skip.saturating_add(page.max_rows - page.rows),
            capacity: skip
                .saturating_mul(row_bytes + 1)
                .saturating_add(page.available()),
            row_bytes,
        }
    }
}
pub(super) struct FileSink {
    mode: Mode,
    path: String,
    count: u64,
    rows: Vec<String>,
    used: usize,
    max_rows: usize,
    capacity: usize,
    row_bytes: usize,
    overflow: bool,
    clock: Arc<dyn Clock>,
    cancel: CancellationToken,
    deadline: Instant,
    pub(super) binary: bool,
    pub(super) timed_out: bool,
}
impl FileSink {
    pub(super) fn new(budget: &Budget<'_>, args: &Args, path: &str, capture: Capture) -> Self {
        Self {
            mode: args.output_mode,
            path: path.into(),
            count: 0,
            rows: vec![],
            used: 0,
            max_rows: capture.max_rows,
            capacity: capture.capacity,
            row_bytes: capture.row_bytes,
            overflow: false,
            clock: Arc::clone(&budget.cx.clock),
            cancel: budget.cancel.clone(),
            deadline: budget.started + budget.cx.limits.search_max_elapsed,
            binary: false,
            timed_out: false,
        }
    }
    fn check(&mut self) -> io::Result<bool> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::other(ToolError::Cancelled));
        }
        self.timed_out = self.clock.now() >= self.deadline;
        Ok(!self.timed_out)
    }
    fn line(&mut self, line: u64, bytes: &[u8], separator: char) -> io::Result<bool> {
        if !self.check()? {
            return Ok(false);
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            self.binary = true;
            return Ok(false);
        };
        if self.mode != Mode::Content {
            return Ok(true);
        }
        let text = text
            .strip_suffix('\n')
            .map_or(text, |line| line.strip_suffix('\r').unwrap_or(line));
        let prefix = format!("{}{separator}{line}{separator}", self.path);
        let mut row = prefix;
        let room = self.row_bytes.saturating_sub(row.len());
        let end = text.floor_char_boundary(room.min(text.len()));
        row.push_str(&text[..end]);
        if end < text.len() || row.len() > self.row_bytes {
            row.truncate(row.floor_char_boundary(self.row_bytes - TRUNCATED.len()));
            row.push_str(TRUNCATED);
        }
        self.push(row);
        Ok(true)
    }
    fn push(&mut self, row: String) {
        if self.overflow {
            return;
        }
        let needed = row.len() + usize::from(!self.rows.is_empty());
        if self.rows.len() == self.max_rows || self.used + needed > self.capacity {
            self.overflow = true;
            return;
        }
        self.used += needed;
        self.rows.push(row);
    }
    pub(super) fn append(self, page: &mut Page) -> Result<bool, ToolError> {
        match self.mode {
            Mode::FilesWithMatches if self.count > 0 => page.push(&self.path),
            Mode::Count if self.count > 0 => page.push(&format!("{}:{}", self.path, self.count)),
            Mode::Content => self.append_rows(page),
            _ => Ok(true),
        }
    }
    fn append_rows(self, page: &mut Page) -> Result<bool, ToolError> {
        for row in self.rows {
            if !page.push(&row)? {
                return Ok(false);
            }
        }
        page.more |= self.overflow;
        Ok(!page.more)
    }
}
impl Sink for FileSink {
    type Error = io::Error;
    fn matched(&mut self, _: &Searcher, matched: &SinkMatch<'_>) -> io::Result<bool> {
        self.count += 1;
        self.line(matched.line_number().unwrap_or(1), matched.bytes(), ':')
    }
    fn context(&mut self, _: &Searcher, context: &SinkContext<'_>) -> io::Result<bool> {
        self.line(context.line_number().unwrap_or(1), context.bytes(), '-')
    }
    fn context_break(&mut self, _: &Searcher) -> io::Result<bool> {
        if !self.check()? {
            return Ok(false);
        }
        self.push("--".into());
        Ok(true)
    }
    fn binary_data(&mut self, _: &Searcher, _: u64) -> io::Result<bool> {
        self.binary = true;
        Ok(false)
    }
}
