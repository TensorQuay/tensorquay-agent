//! Cooperative file, byte and elapsed-time accounting.
use crate::{SearchStopReason, ToolContext, ToolError, builtin::file_io};
use std::{
    fs::File,
    io::{self, Read},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

pub(crate) struct Budget<'a> {
    pub(crate) cx: &'a ToolContext,
    pub(crate) cancel: &'a CancellationToken,
    pub(crate) started: Instant,
    pub(crate) files: u64,
    pub(crate) bytes: u64,
    pub(crate) reason: Option<SearchStopReason>,
}
impl<'a> Budget<'a> {
    pub(crate) fn new(
        cx: &'a ToolContext,
        cancel: &'a CancellationToken,
        started: Instant,
    ) -> Self {
        Self {
            cx,
            cancel,
            started,
            files: 0,
            bytes: 0,
            reason: None,
        }
    }
    pub(crate) fn check(&mut self) -> Result<bool, ToolError> {
        file_io::cancelled(self.cancel)?;
        if self.cx.clock.now().saturating_duration_since(self.started)
            >= self.cx.limits.search_max_elapsed
        {
            self.reason.get_or_insert(SearchStopReason::Time);
        }
        Ok(self.reason.is_none())
    }
    pub(crate) fn file(&mut self) -> Result<bool, ToolError> {
        if !self.check()? {
            return Ok(false);
        }
        if self.files == self.cx.limits.search_max_files {
            self.reason = Some(SearchStopReason::Files);
            return Ok(false);
        }
        if self.bytes == self.cx.limits.search_max_bytes {
            self.reason = Some(SearchStopReason::Bytes);
            return Ok(false);
        }
        self.files += 1;
        Ok(true)
    }
    pub(crate) fn account(&mut self, bytes: u64) -> bool {
        let remaining = self.cx.limits.search_max_bytes - self.bytes;
        self.bytes += remaining.min(bytes);
        if bytes > remaining {
            self.reason = Some(SearchStopReason::Bytes);
            return false;
        }
        true
    }
}
pub(crate) struct Reader<'a, 'b> {
    pub(crate) file: File,
    pub(crate) remaining: u64,
    pub(crate) budget: &'a mut Budget<'b>,
}
impl Read for Reader<'_, '_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        if !self.budget.check().map_err(io::Error::other)? {
            return Err(io::Error::other("search budget stopped"));
        }
        let remaining = self.budget.cx.limits.search_max_bytes - self.budget.bytes;
        if remaining == 0 {
            self.budget.reason = Some(SearchStopReason::Bytes);
            // An error, never artificial EOF: truncation must not produce a false end-of-line match.
            return Err(io::Error::other("search byte budget stopped"));
        }
        let count = usize::try_from(remaining.min(self.remaining))
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let read = self.file.read(&mut bytes[..count])?;
        self.budget.bytes += read as u64;
        self.remaining -= read as u64;
        Ok(read)
    }
}
