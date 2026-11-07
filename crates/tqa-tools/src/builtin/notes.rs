//! Run notes: one bounded file per conversation that the agent keeps for itself (ADR-0024).
//! The projection pins the latest written notes after compaction from the journal, not this file.
use super::{arguments, atomic_write, file_io, specification};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec, WorkspaceRoot};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, io::Read};
use tqa_types::ConversationId;

/// Largest notes text in UTF-8 bytes.
pub const MAX_NOTES_BYTES: usize = 16 * 1024;
/// What `read_notes` returns before anything was written.
const EMPTY: &str = "No notes yet.";

/// The workspace-relative notes path of one conversation.
fn notes_path(conversation: &ConversationId) -> String {
    format!(".tqa/{}/notes.md", conversation.as_str())
}

/// Replaces the conversation's notes atomically.
pub struct WriteNotes {
    spec: ToolSpec,
    path: String,
}
impl WriteNotes {
    /// Constructs the tool for one conversation's notes file.
    #[must_use]
    pub fn new(conversation: &ConversationId) -> Self {
        Self {
            spec: specification(
                "write_notes",
                "Replace your run notes: a short record of the goal, progress, decisions and open questions that you want to keep. The latest notes stay in your context after older conversation is summarised. At most 16 KiB.",
                json!({"type":"object", "required":["text"], "additionalProperties":false,
                    "properties":{"text":{"type":"string"}}}),
                false,
            ),
            path: notes_path(conversation),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    text: String,
}
#[async_trait]
impl Tool for WriteNotes {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args: WriteArgs = arguments(args)?;
        if args.text.len() > MAX_NOTES_BYTES {
            return Err(ToolError::invalid(
                "/text",
                "notes must be at most 16384 UTF-8 bytes",
            ));
        }
        let (workspace, path) = (cx.workspace.clone(), self.path.clone());
        file_io::run(cx, move |cancel| {
            let target = workspace.resolve(&path)?;
            let parent = target
                .parent()
                .ok_or_else(|| ToolError::PathInvalid("missing parent directory".into()))?;
            fs::create_dir_all(parent)?;
            atomic_write::replace(&workspace, &path, &args.text, cancel, |_| Ok(()))?;
            Ok(ToolOutput::text(format!(
                "Notes saved ({} bytes).",
                args.text.len()
            )))
        })
        .await
    }
}

/// Returns the conversation's current notes.
pub struct ReadNotes {
    spec: ToolSpec,
    path: String,
}
impl ReadNotes {
    /// Constructs the tool for one conversation's notes file.
    #[must_use]
    pub fn new(conversation: &ConversationId) -> Self {
        Self {
            spec: specification(
                "read_notes",
                "Read your current run notes.",
                json!({"type":"object", "additionalProperties":false, "properties":{}}),
                true,
            ),
            path: notes_path(conversation),
        }
    }
}
#[async_trait]
impl Tool for ReadNotes {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let _: Empty = arguments(args)?;
        let (workspace, path) = (cx.workspace.clone(), self.path.clone());
        file_io::run(cx, move |_| read(&workspace, &path)).await
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn read(workspace: &WorkspaceRoot, path: &str) -> Result<ToolOutput, ToolError> {
    let target = workspace.resolve(path)?;
    let file = match fs::File::open(&target) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ToolOutput::text(EMPTY));
        }
        Err(error) => return Err(error.into()),
    };
    if !file.metadata()?.is_file() {
        return Err(ToolError::PathInvalid("expected regular file".into()));
    }
    let mut text = String::new();
    file.take(MAX_NOTES_BYTES as u64)
        .read_to_string(&mut text)
        .map_err(|_| ToolError::PathInvalid("notes are not UTF-8 text".into()))?;
    Ok(ToolOutput::text(if text.is_empty() {
        EMPTY.into()
    } else {
        text
    }))
}
