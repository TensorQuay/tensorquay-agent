//! Ordered exact replacements are validated in memory before one atomic write.
mod diff;
mod regions;
#[cfg(test)]
mod tests;
use super::{arguments, atomic_write, file_io, read_file, specification};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec, WorkspaceRoot};
use async_trait::async_trait;
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
                // A success may have changed files: a repeated failure counts afresh after it (backlog #18).
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
        for edit in &args.edits {
            if edit.old_text.is_empty() || edit.old_text == edit.new_text {
                return Err(ToolError::invalid(
                    "/edits",
                    "old text must be nonempty and differ from new text",
                ));
            }
        }
        let workspace = cx.workspace.clone();
        let max = cx.limits.file_read_bytes;
        file_io::run(cx, move |cancel| {
            apply(&workspace, &args, max, cancel, || Ok(()))
        })
        .await
    }
}
fn apply(
    workspace: &WorkspaceRoot,
    args: &Args,
    max: usize,
    cancel: &CancellationToken,
    before_rename: impl FnOnce() -> Result<(), ToolError>,
) -> Result<ToolOutput, ToolError> {
    let (original, stamp) = read(workspace, &args.path, max, cancel)?;
    let crlf = original.matches("\r\n").count()
        > original.bytes().filter(|&byte| byte == b'\n').count() / 2;
    let mut text = original.clone();
    let mut regions = Regions::default();
    for (index, edit) in args.edits.iter().enumerate() {
        file_io::cancelled(cancel)?;
        let old = line_endings(&edit.old_text, crlf);
        let new = line_endings(&edit.new_text, crlf);
        let matches = matches(&text, &old, edit.replace_all, index)?;
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
    let output = diff::render(&args.path, args.edits.len(), &original, &text, regions);
    atomic_write::replace(workspace, &args.path, &text, cancel, |path| {
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
fn line_endings(text: &str, crlf: bool) -> String {
    let text = text.replace("\r\n", "\n");
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}
fn matches(text: &str, old: &str, all: bool, index: usize) -> Result<Vec<usize>, ToolError> {
    let matches: Vec<_> = if all {
        text.match_indices(old).map(|(offset, _)| offset).collect()
    } else {
        overlapping(text, old)
    };
    let path = format!("/edits/{index}/old_text");
    if matches.is_empty() {
        let normalize = |text: &str| {
            text.split('\n')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("\n")
        };
        let normalized = normalize(text);
        let hint = normalized.find(&normalize(old)).map_or_else(
            || "not found".to_owned(),
            |offset| {
                format!(
                    "not found; a match differing only in whitespace starts at line {}",
                    line(&normalized, offset)
                )
            },
        );
        return Err(ToolError::invalid(&path, &hint));
    }
    if !all && matches.len() > 1 {
        let lines = matches
            .iter()
            .take(10)
            .map(|&offset| line(text, offset).to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ToolError::invalid(
            &path,
            &format!("ambiguous; matches at lines {lines}"),
        ));
    }
    Ok(matches)
}
fn overlapping(text: &str, old: &str) -> Vec<usize> {
    let (mut matches, mut offset) = (Vec::new(), 0);
    while let Some(relative) = text[offset..].find(old) {
        let start = offset + relative;
        matches.push(start);
        if matches.len() == 10 {
            break;
        }
        offset = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    matches
}
fn line(text: &str, offset: usize) -> usize {
    text[..offset].bytes().filter(|&byte| byte == b'\n').count() + 1
}
