//! Ordered exact replacements are validated in memory before one atomic write.
mod diff;
mod miss;
mod regions;
#[cfg(test)]
mod tests;
use super::{arguments, atomic_write, file_io, read_file, specification};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec, WorkspaceRoot};
use async_trait::async_trait;
use miss::Miss;
use regions::Regions;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;

/// Applies exact text replacements atomically, preserving file permissions and line endings.
pub struct EditFile {
    spec: ToolSpec,
}
impl EditFile {
    /// Constructs the bounded edit schema; diffs describe changed regions.
    #[must_use]
    pub fn new() -> Self {
        Self {
            spec: ToolSpec {
                // A success may have changed files: a repeated failure counts afresh after it (CORE-E-041).
                changes_workspace: true,
                ..specification(
                    "edit_file",
                    "Replace exact workspace text atomically; show a diff of changed regions. Each old_text must occur once, including overlapping occurrences. replace_all replaces non-overlapping matches from left to right.",
                    json!({
                        "type":"object","required":["path","edits"],"additionalProperties":false,
                        "properties":{"path":{"type":"string"},"edits":{"type":"array","minItems":1,"maxItems":50,
                            "items":{"type":"object","required":["old_text","new_text","replace_all"],"additionalProperties":false,
                                "properties":{"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"},"replace_all":{"type":["boolean","null"]}}}}}
                    }),
                    false,
                )
            },
        }
    }
}
impl Default for EditFile {
    fn default() -> Self {
        Self::new()
    }
}
/// What an edit that names the same text twice changes: nothing (18.1 item F, proposal E).
const NO_CHANGE: &str =
    "no change: old_text and new_text are the same, so the file was not changed.";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    path: String,
    edits: Vec<Edit>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    old_text: String,
    new_text: String,
    #[serde(default)]
    replace_all: bool,
}
#[async_trait]
impl Tool for EditFile {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args: Args = arguments(args)?;
        if !(1..=50).contains(&args.edits.len()) {
            return Err(ToolError::invalid("/edits", "expected 1 through 50 edits"));
        }
        if args.edits.iter().any(|edit| edit.old_text.is_empty()) {
            return Err(ToolError::invalid("/edits", "old text must be nonempty"));
        }
        // An edit naming the same text twice changes nothing: it is left out and said so, never refused, so a model
        // repeating it isn't stopped as a failure (proposal E, with the refusals of A, B and D).
        let unchanged = args.edits.iter().filter(|edit| same(edit)).count();
        let (workspace, memory) = (cx.workspace.clone(), cx.memory.clone());
        let max = cx.limits.file_read_bytes;
        file_io::run(cx, move |cancel| {
            if unchanged == args.edits.len() {
                read(&workspace, &args.path, max, cancel)?;
                return Ok(ToolOutput::text(NO_CHANGE));
            }
            let mut output = apply((&workspace, memory.as_ref()), &args, max, cancel, || Ok(()))?;
            if unchanged > 0 {
                let said = if unchanged == 1 {
                    "1 edit changed nothing: its old_text and new_text are the same.\n".to_owned()
                } else {
                    format!("{unchanged} edits changed nothing: their old_text and new_text are the same.\n")
                };
                if !output.text.ends_with('\n') {
                    output.text.push('\n');
                }
                output.text.push_str(&said);
            }
            Ok(output)
        })
        .await
    }
    // The second identical refusal shows the file's region around the lines the refusal named (proposal D).
    async fn repeated_refusal(&self, args: &Value, cx: &ToolContext) -> Option<String> {
        let args: Args = arguments(args.clone()).ok()?;
        let workspace = cx.workspace.clone();
        let max = cx.limits.file_read_bytes;
        file_io::run(cx, move |cancel| {
            let (original, _) = read(&workspace, &args.path, max, cancel)?;
            Ok(match edited(&original, &args.edits, max, cancel) {
                Err(Stopped::Miss {
                    text, old, miss, ..
                }) => miss.region(&args.path, &text, &old),
                Ok(_) | Err(Stopped::Tool(_)) => None,
            })
        })
        .await
        .ok()
        .flatten()
    }
}
/// Why the edits stopped: a tool failure, or an `old_text` that didn't match once in the file as it then stood.
enum Stopped {
    Tool(ToolError),
    Miss {
        /// The edit, by its place in the call.
        index: usize,
        text: String,
        old: String,
        miss: Miss,
    },
}
impl From<ToolError> for Stopped {
    fn from(error: ToolError) -> Self {
        Self::Tool(error)
    }
}
/// The edits made in memory, in order: the new text and the changed regions.
fn edited(
    original: &str,
    edits: &[Edit],
    max: usize,
    cancel: &CancellationToken,
) -> Result<(String, Regions), Stopped> {
    let crlf = original.matches("\r\n").count()
        > original.bytes().filter(|&byte| byte == b'\n').count() / 2;
    let mut text = original.to_owned();
    let mut regions = Regions::default();
    for (index, edit) in edits.iter().enumerate() {
        file_io::cancelled(cancel)?;
        // The same text twice changes nothing (proposal E): skipped, its place kept.
        if same(edit) {
            continue;
        }
        let old = line_endings(&edit.old_text, crlf);
        let new = line_endings(&edit.new_text, crlf);
        let matches = match matches(&text, &old, edit.replace_all) {
            Ok(matches) => matches,
            Err(miss) => {
                return Err(Stopped::Miss {
                    index,
                    text,
                    old,
                    miss,
                });
            }
        };
        let length = text.len() - matches.len() * old.len();
        let length = new
            .len()
            .checked_mul(matches.len())
            .and_then(|added| length.checked_add(added))
            .filter(|&length| length <= max)
            .ok_or(ToolError::OutputTooLarge { max })?;
        regions.replace(&matches, old.len(), new.len());
        let mut replaced = String::with_capacity(length);
        let mut cursor = 0;
        for start in matches {
            replaced.push_str(&text[cursor..start]);
            replaced.push_str(&new);
            cursor = start + old.len();
        }
        replaced.push_str(&text[cursor..]);
        text = replaced;
    }
    Ok((text, regions))
}
fn apply(
    (workspace, memory): (&WorkspaceRoot, Option<&crate::NoteWriter>),
    args: &Args,
    max: usize,
    cancel: &CancellationToken,
    before_rename: impl FnOnce() -> Result<(), ToolError>,
) -> Result<ToolOutput, ToolError> {
    let (original, stamp) = read(workspace, &args.path, max, cancel)?;
    let (text, regions) = match edited(&original, &args.edits, max, cancel) {
        Ok(done) => done,
        Err(Stopped::Tool(error)) => return Err(error),
        Err(Stopped::Miss {
            index, text, miss, ..
        }) => {
            return Err(ToolError::invalid(
                &format!("/edits/{index}/old_text"),
                &miss.refusal(&text),
            ));
        }
    };
    let changing = args.edits.iter().filter(|edit| !same(edit)).count();
    let output = diff::render(&args.path, changing, &original, &text, regions);
    atomic_write::replace((workspace, memory), &args.path, &text, cancel, |path| {
        before_rename()?;
        let current = path.metadata()?;
        if (current.len(), current.modified()?) != stamp {
            return Err(ToolError::invalid(
                "/path",
                "file changed while editing; read it again",
            ));
        }
        Ok(())
    })?;
    Ok(ToolOutput::text(output))
}
fn read(
    workspace: &WorkspaceRoot,
    requested: &str,
    max: usize,
    cancel: &CancellationToken,
) -> Result<(String, (u64, SystemTime)), ToolError> {
    let path = workspace.resolve(requested)?;
    let (mut file, metadata) = file_io::open_regular(&path)?;
    if metadata.len() > max as u64 {
        return Err(ToolError::OutputTooLarge { max });
    }
    let stamp = (metadata.len(), metadata.modified()?);
    let bytes = file_io::read_bounded(&mut file, max.saturating_add(1), cancel)?;
    if bytes.len() > max {
        return Err(ToolError::OutputTooLarge { max });
    }
    if read_file::binary(&bytes, false) {
        return Err(ToolError::Unsupported("cannot edit a binary file".into()));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| ToolError::Unsupported("cannot edit a binary file".into()))?;
    Ok((text, stamp))
}
fn same(edit: &Edit) -> bool {
    edit.old_text == edit.new_text
}
fn line_endings(text: &str, crlf: bool) -> String {
    let text = text.replace("\r\n", "\n");
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}
fn matches(text: &str, old: &str, all: bool) -> Result<Vec<usize>, Miss> {
    let (matches, total) = if all {
        let matches: Vec<_> = text.match_indices(old).map(|(offset, _)| offset).collect();
        let total = matches.len();
        (matches, total)
    } else {
        overlapping(text, old)
    };
    if matches.is_empty() {
        return Err(Miss::not_found(text, old));
    }
    if !all && total > 1 {
        return Err(Miss::Ambiguous {
            lines: matches.iter().map(|&offset| line(text, offset)).collect(),
            total,
        });
    }
    Ok(matches)
}
/// The first ten overlapping matches, and how many there are, counted up to [`miss::COUNTED`]: an `old_text` of one
/// repeated character in a file of it matches at every position, so the count stops there.
fn overlapping(text: &str, old: &str) -> (Vec<usize>, usize) {
    let (mut matches, mut total, mut offset) = (Vec::new(), 0, 0);
    while total < miss::COUNTED
        && let Some(relative) = text[offset..].find(old)
    {
        let start = offset + relative;
        if matches.len() < miss::LISTED {
            matches.push(start);
        }
        total += 1;
        offset = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    (matches, total)
}
fn line(text: &str, offset: usize) -> usize {
    miss::line_of(text, offset)
}
