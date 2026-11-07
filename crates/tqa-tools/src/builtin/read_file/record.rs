//! A long-line cursor binds the file identity and scan position to one version.
use super::Args;
use crate::{ToolError, builtin::cursor};
use serde::{Deserialize, Serialize};
use std::{fs::Metadata, time::UNIX_EPOCH};

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Stamp {
    pub(super) size: u64,
    modified: (bool, u64, u32),
}
impl Stamp {
    pub(super) fn new(metadata: &Metadata) -> Result<Self, ToolError> {
        let modified = match metadata.modified()?.duration_since(UNIX_EPOCH) {
            Ok(duration) => (false, duration.as_secs(), duration.subsec_nanos()),
            Err(error) => (
                true,
                error.duration().as_secs(),
                error.duration().subsec_nanos(),
            ),
        };
        Ok(Self {
            size: metadata.len(),
            modified,
        })
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    version: u8,
    path: String,
    pub(super) stamp: Stamp,
    pub(super) offset: u64,
    pub(super) line: u64,
    pub(super) line_start: u64,
    pub(super) content_end: u64,
    pub(super) next_line: u64,
    pub(super) end_line: Option<u64>,
}
impl Record {
    /// Whether `token` reads as a cursor this tool issued, before the file is looked at.
    pub(super) fn issued(token: &str) -> bool {
        cursor::decode::<Self>(token).is_ok()
    }
    pub(super) fn for_request(args: &Args, path: &str, stamp: Stamp) -> Result<Self, ToolError> {
        let Some(token) = &args.cursor else {
            return Ok(Self {
                version: 1,
                path: path.into(),
                stamp,
                offset: 0,
                line: args.start_line.unwrap_or(1),
                line_start: 0,
                content_end: 0,
                next_line: 0,
                end_line: args.end_line,
            });
        };
        let record: Self = cursor::decode(token)?;
        if record.stamp != stamp {
            return Err(stale());
        }
        if record.version != 1
            || record.path != path
            || record.line == 0
            || record.line > record.line_start.saturating_add(1)
            || record.line_start >= record.offset
            || record.offset >= record.content_end
            || record.content_end > record.next_line
            || record.next_line > stamp.size
            || record.end_line.is_some_and(|end| end < record.line)
        {
            return Err(ToolError::invalid(
                "/cursor",
                "invalid cursor for this path; read again from start_line",
            ));
        }
        Ok(record)
    }
    pub(super) fn notice(&self, first_byte: u64) -> Result<String, ToolError> {
        Ok(format!(
            "[partial: line {} bytes {first_byte}-{} of {}; continue with cursor={}]",
            self.line,
            self.offset - self.line_start,
            self.content_end - self.line_start,
            cursor::encode(self)?
        ))
    }
    pub(super) fn more(&self) -> bool {
        self.next_line < self.stamp.size && self.end_line.is_none_or(|end| self.line < end)
    }
}
pub(super) fn stale() -> ToolError {
    ToolError::invalid(
        "/cursor",
        "file changed since the cursor was issued; read again from start_line",
    )
}
