//! Workspace filesystem tools and a sandbox-local shell runner.
mod atomic_write;
pub mod channel;
pub mod computer;
mod cursor;
mod edit_file;
mod fetch_url;
mod file_io;
mod glob;
mod grep;
mod notes;
mod read_file;
mod search;
pub(crate) mod shell;
#[cfg(test)]
mod strict_tests;
pub mod team;
mod write_file;

use crate::{ToolError, ToolName, ToolSpec};
pub use computer::Computer;
pub use edit_file::EditFile;
pub use fetch_url::FetchUrl;
pub use glob::Glob;
pub use grep::Grep;
pub use notes::{MAX_NOTES_BYTES, ReadNotes, WriteNotes};
pub use read_file::PICTURE_SUMMARY;
pub use read_file::ReadFile;
use serde::de::DeserializeOwned;
use serde_json::Value;
pub use shell::{CA_BUNDLE_VARIABLES, Shell};
pub use write_file::WriteFile;

/// Every built-in tool a runtime may register, by name: the ones every run has, then `skill` (when a skill exists) and
/// `fetch_url` (behind the egress proxy), and `computer` (an environment declaring `desktop`, ADR-0039). `git_push`
/// is gone (ADR-0037 §7: a task pushes with plain git and the person's key). A profile's tool allow-list
/// names only these (ADR-0012); the runtime's advertised tools are checked against this list (RT environments test).
pub const BUILTIN_TOOL_NAMES: [&str; 11] = [
    "read_file",
    "write_file",
    "edit_file",
    "glob",
    "grep",
    "read_notes",
    "write_notes",
    "shell",
    "skill",
    "fetch_url",
    "computer",
];

/// The channel tools (ADR-0014 point 6), registered for a run whose conversation belongs to a channel. A profile's
/// tool allow-list may name them too (point 4), so this one list is what both the gateway's profile check and the
/// runtime read.
pub const CHANNEL_TOOL_NAMES: [&str; 4] =
    ["channel_post", "channel_read", "channel_wait", "task_claim"];

/// Whether a profile's tool allow-list may name `name`: a built-in tool or a channel tool (ADR-0012, ADR-0014).
#[must_use]
pub fn profile_tool(name: &str) -> bool {
    BUILTIN_TOOL_NAMES.contains(&name) || CHANNEL_TOOL_NAMES.contains(&name)
}

/// What the model is told about a paging tool's `cursor`, sent with its schema (TOOL-U-051): it's opaque and never a
/// line number; a model guessed `"1"` for `read_file`'s next to a line range (the c14b-rc demo).
const CURSOR: &str =
    "null, unless continuing: the exact cursor an earlier result gave. Never a line number.";

fn specification(
    name: &'static str,
    description: &str,
    parameters: Value,
    parallel_safe: bool,
) -> ToolSpec {
    ToolSpec {
        name: ToolName::builtin(name),
        description: description.into(),
        parameters,
        parallel_safe,
        changes_workspace: false,
    }
}
/// Whether `text` is a null-word a model writes for "nothing": `None`, `null` or `undefined`, in any case.
pub(crate) fn null_word(text: &str) -> bool {
    ["none", "null", "undefined"]
        .iter()
        .any(|word| text.trim().eq_ignore_ascii_case(word))
}

/// An optional closed or opaque field (a cursor, a closed list), with blank and null-word values taken as absent: a
/// model fills an optional string with `""` or `"None"` (the Gate 5 demo before 17.0, 2 Oct 2026; TOOL-U-001,
/// TOOL-U-034).
pub(crate) fn absent(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty() && !null_word(text))
}

/// A tool's arguments, with null members dropped at any depth first: every optional property is required and nullable
/// in the schemas (strict endpoints need both), and a null means the same as leaving it out.
fn arguments<T: DeserializeOwned>(mut args: Value) -> Result<T, ToolError> {
    drop_nulls(&mut args);
    serde_json::from_value(args).map_err(|_| ToolError::invalid("", "invalid tool arguments"))
}

fn drop_nulls(value: &mut Value) {
    match value {
        Value::Object(members) => {
            members.retain(|_, member| !member.is_null());
            members.values_mut().for_each(drop_nulls);
        }
        Value::Array(items) => items.iter_mut().for_each(drop_nulls),
        _ => {}
    }
}
