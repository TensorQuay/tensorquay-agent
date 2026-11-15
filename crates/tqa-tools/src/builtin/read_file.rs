//! Line-numbered bounded pages without loading the source file into memory.
mod page;
mod record;
mod scan;
use super::{arguments, file_io, specification};
use crate::{MAX_READ_FILE_BYTES, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use async_trait::async_trait;
use record::{Record, Stamp};
use scan::Input;
use serde::Deserialize;
use serde_json::{Value, json};

/// The note on a page read with a guessed cursor beside a line range (TOOL-U-051): its own line, before the page's
/// closing notice (which stays the last line a model follows) or last when there is none, within the page's bytes.
const IGNORED_CURSOR: &str =
    "[cursor ignored: it wasn't one an earlier read_file gave; these are the lines asked for]";
/// The same note on a binary file's summary, which holds no lines.
const IGNORED_CURSOR_BINARY: &str = "[cursor ignored: it wasn't one an earlier read_file gave]";

/// Reads workspace text in line-numbered pages and summarizes binary files.
pub struct ReadFile {
    spec: ToolSpec,
}
impl ReadFile {
    /// Constructs the line-range and stateless cursor schema.
    #[must_use]
    pub fn new() -> Self {
        Self {
            spec: specification(
                "read_file",
                "Read line-numbered workspace text. Line numbers are not part of the file; never copy them into edit_file. A default read returns the first 200 lines, within 8 KiB. Prefer grep to find the right range first, then set start_line and optional inclusive end_line. Follow the partial notice with start_line or cursor; cursor excludes both line arguments.",
                json!({"type":"object","required":["path","start_line","end_line","cursor"],"additionalProperties":false,
                "properties":{"path":{"type":"string"},"start_line":{"type":["integer","null"],"minimum":1},"end_line":{"type":["integer","null"],"minimum":1},"cursor":{"type":["string","null"],"description":super::CURSOR}}}),
                true,
            ),
        }
    }
}
impl Default for ReadFile {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    path: String,
    start_line: Option<u64>,
    end_line: Option<u64>,
    cursor: Option<String>,
}
#[async_trait]
impl Tool for ReadFile {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let mut args: Args = arguments(args)?;
        // An empty or null-word cursor is no cursor: small models fill an optional string with "" or "None" (the Gate 5
        // demo before 17.0).
        args.cursor = super::absent(args.cursor);
        // A cursor this tool didn't issue: next to a line range it's a model's guess (cursor "1" with start_line 1, the
        // c14b-rc demo), so the lines are read and the result says the cursor was ignored. Alone it's refused first,
        // saying how to read by lines instead.
        let guessed = args
            .cursor
            .as_deref()
            .is_some_and(|token| !Record::issued(token));
        let ignored = guessed && (args.start_line.is_some() || args.end_line.is_some());
        if ignored {
            args.cursor = None;
        } else if guessed {
            return Err(ToolError::invalid(
                "/cursor",
                "not a cursor from an earlier read_file; to read by lines, leave cursor out",
            ));
        }
        if args.cursor.is_some() && (args.start_line.is_some() || args.end_line.is_some()) {
            return Err(ToolError::invalid(
                "/cursor",
                "cursor excludes start_line and end_line; to continue from the cursor, leave both out; to read by \
                 lines, leave cursor out",
            ));
        }
        if args.start_line == Some(0)
            || args
                .end_line
                .is_some_and(|end| end < args.start_line.unwrap_or(1))
        {
            return Err(ToolError::invalid(
                "/start_line",
                "expected a positive, ordered line range",
            ));
        }
        let (workspace, mut limits) = (cx.workspace.clone(), cx.limits);
        if ignored {
            // The note's line is reserved, so the page and the note keep the page's bound together.
            limits.read_page_bytes = limits
                .read_page_bytes
                .saturating_sub(IGNORED_CURSOR.len() + 1);
        }
        file_io::run(cx, move |cancel| {
            let path = workspace.resolve(&args.path)?;
            let (file, metadata) = file_io::open_regular(&path)?;
            let stamp = Stamp::new(&metadata)?;
            let relative = path
                .strip_prefix(workspace.path())
                .ok()
                .and_then(|path| path.to_str())
                .ok_or_else(|| ToolError::PathInvalid("expected a workspace UTF-8 path".into()))?;
            let record = Record::for_request(&args, relative, stamp)?;
            if metadata.len() > MAX_READ_FILE_BYTES {
                return Err(ToolError::invalid(
                    "/path",
                    &format!(
                        "file is {} bytes; maximum readable size is {MAX_READ_FILE_BYTES} bytes",
                        metadata.len()
                    ),
                ));
            }
            let mut input = Input::new(file, cancel);
            if input.probe_binary()? {
                return Ok(summary(input.picture(), stamp.size, ignored));
            }
            let text = page::render(&mut input, record, limits)?;
            if Stamp::new(&input.metadata()?)? != stamp {
                return Err(record::stale());
            }
            let picture = if text.is_none() {
                input.picture()
            } else {
                None
            };
            Ok(text.map_or_else(
                || summary(picture, stamp.size, ignored),
                |text| ToolOutput::text(if ignored { noted(&text) } else { text }),
            ))
        })
        .await
    }
}

/// `page` with the ignored-cursor note on its own line before its closing notice (every notice is a last line in
/// brackets; a content line starts with its number), or after its last line when it has none.
fn noted(page: &str) -> String {
    let start = page
        .trim_end_matches('\n')
        .rfind('\n')
        .map_or(0, |at| at + 1);
    if page[start..].starts_with('[') {
        format!("{}{IGNORED_CURSOR}\n{}", &page[..start], &page[start..])
    } else {
        format!("{page}{IGNORED_CURSOR}")
    }
}
pub(super) fn binary(bytes: &[u8], continues: bool) -> bool {
    bytes.contains(&0)
        || std::str::from_utf8(bytes)
            .err()
            .is_some_and(|error| !continues || error.error_len().is_some())
}
fn binary_summary(size: u64, ignored: bool) -> ToolOutput {
    let note = if ignored {
        format!("\n{IGNORED_CURSOR_BINARY}")
    } else {
        String::new()
    };
    ToolOutput::text(format!("[binary file, {size} bytes]{note}"))
}

/// The first words of a picture's summary: the runner finds the result by them and, on a route that reads pictures,
/// shows the file to the model beside it (TOOL-U-143, CORE-U-090).
pub const PICTURE_SUMMARY: &str = "[picture file, ";

/// A binary file's summary: a picture's kind and size (TOOL-U-143), else its size.
fn summary(picture: Option<tqa_types::ImageMediaType>, size: u64, ignored: bool) -> ToolOutput {
    picture.map_or_else(
        || binary_summary(size, ignored),
        |kind| picture_summary(kind, size),
    )
}

/// A picture the model is shown on a route that reads pictures; its size and kind otherwise.
fn picture_summary(kind: tqa_types::ImageMediaType, size: u64) -> ToolOutput {
    ToolOutput::text(format!(
        "{PICTURE_SUMMARY}{}, {size} bytes: shown to you as a picture when this model reads pictures]",
        kind.as_str()
    ))
}
