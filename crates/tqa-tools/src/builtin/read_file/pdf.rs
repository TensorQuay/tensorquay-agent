//! `read_file` on a PDF (PRD item 30; TOOL-U-145): the document's text by page, from the reader in `tqa-pdf`. The line
//! arguments are page numbers here (a PDF has pages, not lines), and the cursor continues from the next page. The
//! result starts with [`PDF_SUMMARY`] and the page count, so the runner knows the result holds pages and, on a route
//! that reads pictures, can show them (CORE-U-092).
use super::Args;
use crate::{ToolError, ToolLimits, ToolOutput, builtin::cursor};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use tqa_pdf::{Document, OpenError};

/// The first words of a PDF result: `[PDF, 12 pages]`.
pub const PDF_SUMMARY: &str = "[PDF, ";
/// The line before each page's text, with its number from 1; the runner reads the pages a result holds from it.
pub const PAGE_MARK: &str = "--- page ";
/// What an empty page reads.
const NO_TEXT: &str = "[no text on this page]";

/// A cursor into a PDF: the page to continue from, bound to one version of one file.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PdfCursor {
    version: u8,
    path: String,
    stamp: super::record::Stamp,
    next_page: u32,
    end_page: Option<u32>,
}

impl PdfCursor {
    /// Whether `token` is a PDF cursor this tool issued.
    pub(super) fn issued(token: &str) -> bool {
        cursor::decode::<Self>(token).is_ok()
    }
}

/// The pages asked for: `start_line`/`end_line` as pages, or the cursor's place.
struct Range {
    first: u32,
    last: Option<u32>,
}

fn range(args: &Args, path: &str, stamp: super::record::Stamp) -> Result<Range, ToolError> {
    let Some(token) = &args.cursor else {
        let first = u32::try_from(args.start_line.unwrap_or(1)).unwrap_or(u32::MAX);
        let last = args
            .end_line
            .map(|end| u32::try_from(end).unwrap_or(u32::MAX));
        return Ok(Range { first, last });
    };
    let cursor: PdfCursor = cursor::decode(token)?;
    if cursor.version != 1 || cursor.path != path {
        return Err(ToolError::invalid("/cursor", "not a cursor for this file"));
    }
    if cursor.stamp != stamp {
        return Err(super::record::stale());
    }
    Ok(Range {
        first: cursor.next_page,
        last: cursor.end_page,
    })
}

/// How an unopenable PDF reads: a tool result, never an error, so the model learns what the file is.
fn unopenable(error: &OpenError, size: u64) -> ToolOutput {
    let words = match error {
        OpenError::NotPdf => return ToolOutput::text(format!("[binary file, {size} bytes]")),
        OpenError::Password => "a password is needed to read it".to_owned(),
        OpenError::TooManyPages => format!("more than {} pages; not read", tqa_pdf::MAX_PAGES),
        OpenError::Damaged(why) => format!("damaged, nothing could be read: {why}"),
    };
    ToolOutput::text(format!("{PDF_SUMMARY}{size} bytes: {words}]"))
}

/// The bytes kept for the notice line, so the pages and the notice fit the bound together.
const RESERVE: usize = 96;

/// One page's block: its mark, then its text, a note of a cut, or why it couldn't be read.
fn block(document: &Document, page: u32) -> String {
    let text = match document.text(page) {
        Ok(text) if text.is_empty() => NO_TEXT.to_owned(),
        Ok(text) if text.cut => format!(
            "{}\n[this page's text was cut at {} bytes]",
            text.text,
            tqa_pdf::MAX_PAGE_TEXT_BYTES
        ),
        Ok(text) => text.text,
        Err(error) => format!("[this page couldn't be read: {}]", error.message),
    };
    format!("{PAGE_MARK}{page} ---\n{text}\n")
}

/// The one page alone is too long for the read: as much of `block` as fits `budget`, cut on a character boundary,
/// then a note that the next read starts at the next page.
fn push_cut(out: &mut String, block: &str, budget: usize) {
    let room = budget.saturating_sub(out.len() + RESERVE + PAGE_MARK.len() + 16);
    let mut end = room.min(block.len());
    while !block.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&block[..end]);
    out.push_str("\n[page cut: too long for one read; the next read starts at the next page]\n");
}

/// Reads `bytes` (a file starting `%PDF-`) as `args` ask, within `limits.read_page_bytes`: whole pages, as many as fit
/// (at least one, cut when alone it is too long), then a notice with the cursor for the next page.
pub(super) fn read(
    bytes: Vec<u8>,
    args: &Args,
    path: &str,
    stamp: super::record::Stamp,
    limits: ToolLimits,
) -> Result<ToolOutput, ToolError> {
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let document = match Document::open(bytes) {
        Ok(document) => document,
        Err(error) => return Ok(unopenable(&error, size)),
    };
    let count = document.page_count();
    let Range { first, last } = range(args, path, stamp)?;
    if first == 0 || last.is_some_and(|last| last < first) {
        return Err(ToolError::invalid(
            "/start_line",
            "for a PDF, start_line and end_line are page numbers: a positive, ordered range",
        ));
    }
    let mut out = format!("{PDF_SUMMARY}{count} pages]\n");
    if first > count {
        let _ = write!(out, "[end of document: page {first} is past the last page]");
        return Ok(ToolOutput::text(out));
    }
    let last = last.map_or(count, |last| last.min(count));
    let budget = limits.read_page_bytes;
    let mut page = first;
    while page <= last {
        let block = block(&document, page);
        // A notice line is kept in reserve, so the pages and the notice fit the bound together.
        if out.len() + block.len() + RESERVE > budget {
            if page == first {
                push_cut(&mut out, &block, budget);
                page += 1;
            }
            break;
        }
        out.push_str(&block);
        page += 1;
    }
    if page <= last {
        let token = cursor::encode(&PdfCursor {
            version: 1,
            path: path.to_owned(),
            stamp,
            next_page: page,
            end_page: args
                .end_line
                .map(|end| u32::try_from(end).unwrap_or(u32::MAX)),
        })?;
        let _ = write!(
            out,
            "[partial: pages {first}-{} of {count}; continue with cursor {token} or start_line={page}]",
            page - 1
        );
    } else {
        let _ = write!(out, "[end of document: {count} pages]");
    }
    Ok(ToolOutput::text(out))
}
