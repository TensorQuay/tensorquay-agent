//! A bounded page and a cursor bound to normalized search options.
use super::budget::Budget;
use crate::{SearchStatus, SearchStopReason, ToolError, ToolOutput, builtin::cursor};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// A digest/position cursor plus its notice and a binary count fit within 256 bytes.
const FOOTER_BYTES: usize = 256;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    digest: String,
    position: u64,
}
pub(crate) struct Page {
    text: String,
    cursor: Cursor,
    pub(crate) skip: u64,
    pub(crate) rows: usize,
    pub(crate) max_rows: usize,
    pub(crate) content_bytes: usize,
    pub(crate) more: bool,
}
impl Page {
    pub(crate) fn new(
        options: &impl Serialize,
        token: Option<&str>,
        bytes: usize,
        rows: usize,
    ) -> Result<Self, ToolError> {
        let encoded = serde_json::to_vec(options)
            .map_err(|_| ToolError::Io("search options encoding failed".into()))?;
        let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(encoded));
        // A cursor this search didn't issue is refused, deliberately, unlike read_file's guess beside a line range
        // (TOOL-U-051): a search cursor carries the query it continues, so a guess has nothing to fall back to that
        // the model asked for; the refusal says to search again, and the schema's description says it's opaque.
        let cursor = if let Some(token) = token {
            let cursor: Cursor = cursor::decode(token)?;
            if cursor.version != 1 || cursor.digest != digest {
                return Err(ToolError::invalid(
                    "/cursor",
                    "search options changed; search again",
                ));
            }
            cursor
        } else {
            Cursor {
                version: 1,
                digest,
                position: 0,
            }
        };
        Ok(Self {
            text: String::new(),
            skip: cursor.position,
            cursor,
            rows: 0,
            max_rows: rows,
            content_bytes: bytes - FOOTER_BYTES,
            more: false,
        })
    }
    pub(crate) fn push(&mut self, row: &str) -> Result<bool, ToolError> {
        if self.skip > 0 {
            self.skip -= 1;
            return Ok(true);
        }
        if row.len() > self.content_bytes {
            return Err(ToolError::OutputTooLarge {
                max: self.content_bytes + FOOTER_BYTES,
            });
        }
        let separator = usize::from(!self.text.is_empty());
        if self.rows == self.max_rows
            || self.text.len() + separator + row.len() > self.content_bytes
        {
            self.more = true;
            return Ok(false);
        }
        if separator != 0 {
            self.text.push('\n');
        }
        self.text.push_str(row);
        self.rows += 1;
        self.cursor.position += 1;
        Ok(true)
    }
    pub(crate) fn available(&self) -> usize {
        self.content_bytes
            .saturating_sub(self.text.len() + usize::from(!self.text.is_empty()))
    }
    pub(crate) fn finish(
        mut self,
        budget: &Budget<'_>,
        stopped: Option<SearchStopReason>,
        binaries: u64,
    ) -> Result<ToolOutput, ToolError> {
        let reason = stopped.or(budget.reason);
        if self.skip > 0 && reason.is_none() {
            return Err(ToolError::invalid(
                "/cursor",
                "cursor position is no longer available; search again",
            ));
        }
        let next_cursor = if self.more && reason.is_none() {
            Some(cursor::encode(&self.cursor)?)
        } else {
            None
        };
        if binaries != 0 {
            self.footer(&format!("[binary files skipped: {binaries}]"));
        }
        if let Some(reason) = reason {
            self.footer(&format!(
                "[incomplete: scanned {} files, reason={}; narrow the path or glob]",
                budget.files,
                reason.as_str()
            ));
        } else if let Some(cursor) = &next_cursor {
            self.footer(&format!("[partial: continue with cursor={cursor}]"));
        }
        let mut output = ToolOutput::text(self.text);
        output.search = Some(SearchStatus {
            complete: reason.is_none() && next_cursor.is_none(),
            scanned_files: budget.files,
            scanned_bytes: budget.bytes,
            reason,
            next_cursor,
        });
        Ok(output)
    }
    fn footer(&mut self, text: &str) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(text);
    }
}
