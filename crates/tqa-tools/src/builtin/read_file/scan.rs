//! One fixed read buffer; skipping and line sizing do not retain source text.
use crate::{ToolError, builtin::file_io};
use std::{
    fs::{File, Metadata},
    io::{BufRead, BufReader, Read},
};
use tokio_util::sync::CancellationToken;

const READ_BUFFER_BYTES: usize = 8192;
pub(super) struct Input<'a> {
    reader: BufReader<File>,
    position: u64,
    cancel: &'a CancellationToken,
}
impl<'a> Input<'a> {
    pub(super) fn new(file: File, cancel: &'a CancellationToken) -> Self {
        Self {
            reader: BufReader::with_capacity(READ_BUFFER_BYTES, file),
            position: 0,
            cancel,
        }
    }
    pub(super) fn metadata(&self) -> Result<Metadata, ToolError> {
        Ok(self.reader.get_ref().metadata()?)
    }
    pub(super) fn probe_binary(&mut self) -> Result<bool, ToolError> {
        file_io::cancelled(self.cancel)?;
        Ok(self.reader.fill_buf()?.contains(&0))
    }
    /// The picture format the file starts with (TOOL-U-143), read from the start without moving past the buffer.
    pub(super) fn picture(&mut self) -> Option<tqa_types::ImageMediaType> {
        self.seek(0).ok()?;
        tqa_types::ImageMediaType::from_header(self.reader.fill_buf().ok()?)
    }
    pub(super) fn seek(&mut self, offset: u64) -> Result<(), ToolError> {
        let difference = i64::try_from(i128::from(offset) - i128::from(self.position))
            .map_err(|_| ToolError::PathInvalid("read offset exceeds bounds".into()))?;
        self.reader.seek_relative(difference)?;
        self.position = offset;
        Ok(())
    }
    pub(super) fn skip(&mut self, start_line: u64) -> Result<Option<u64>, ToolError> {
        self.seek(0)?;
        let (mut line, mut last) = (1, None);
        while line < start_line {
            file_io::cancelled(self.cancel)?;
            let buffer = self.reader.fill_buf()?;
            if buffer.is_empty() {
                break;
            }
            let consumed = consume_lines(buffer, &mut line, start_line);
            last = buffer.get(consumed - 1).copied();
            self.reader.consume(consumed);
            self.position += consumed as u64;
        }
        let at_end = self.reader.fill_buf()?.is_empty();
        let total = line - 1 + u64::from(last.is_some_and(|byte| byte != b'\n'));
        Ok(at_end.then_some(total))
    }
    pub(super) fn span(&mut self) -> Result<Option<(u64, u64, u64)>, ToolError> {
        let (start, mut last) = (self.position, None);
        loop {
            file_io::cancelled(self.cancel)?;
            let buffer = self.reader.fill_buf()?;
            if buffer.is_empty() {
                return Ok((self.position > start).then_some((
                    start,
                    self.position,
                    self.position,
                )));
            }
            if let Some(index) = memchr::memchr(b'\n', buffer) {
                let carriage = index
                    .checked_sub(1)
                    .and_then(|index| buffer.get(index))
                    .copied()
                    .or(last)
                    == Some(b'\r');
                let end = self.position + index as u64 - u64::from(carriage);
                self.reader.consume(index + 1);
                self.position += index as u64 + 1;
                return Ok(Some((start, end, self.position)));
            }
            let count = buffer.len();
            last = buffer.last().copied();
            self.reader.consume(count);
            self.position += count as u64;
        }
    }
    pub(super) fn append(
        &mut self,
        output: &mut Vec<u8>,
        offset: u64,
        count: usize,
        partial: bool,
    ) -> Result<Option<usize>, ToolError> {
        file_io::cancelled(self.cancel)?;
        self.seek(offset)?;
        let start = output.len();
        output.resize(start + count, 0);
        self.reader.read_exact(&mut output[start..])?;
        self.position += count as u64;
        match std::str::from_utf8(&output[start..]) {
            Ok(_) => Ok(Some(count)),
            Err(error) if partial && error.error_len().is_none() => {
                let valid = error.valid_up_to();
                output.truncate(start + valid);
                Ok(Some(valid))
            }
            Err(_) => Ok(None),
        }
    }
}
fn consume_lines(buffer: &[u8], line: &mut u64, target: u64) -> usize {
    for index in memchr::memchr_iter(b'\n', buffer) {
        *line += 1;
        if *line == target {
            return index + 1;
        }
    }
    buffer.len()
}
