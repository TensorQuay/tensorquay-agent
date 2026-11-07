//! Discover bounded repository instructions from the root to the current directory.
use crate::PromptError;
use std::{
    fmt::Write as _,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use tqa_tools::WorkspaceRoot;

/// Maximum UTF-8 content bytes loaded from each instruction file, including its marker.
pub const MAX_INSTRUCTION_FILE_BYTES: usize = 32 * 1024;
/// Maximum combined content bytes, including truncation markers; rendered headers are additional.
pub const MAX_INSTRUCTION_TOTAL_BYTES: usize = 96 * 1024;
const FILE_MARKER: &str = "\n[instructions truncated: file byte limit]";
const TOTAL_MARKER: &str = "\n[instructions truncated: total byte limit]";

/// A loaded instruction file with a workspace-relative display path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionFile {
    /// The location at which these instructions apply.
    pub path: PathBuf,
    /// UTF-8 text, with a marker when capped.
    pub content: String,
    /// Whether file or total limits omitted content.
    pub truncated: bool,
}

/// Reads `AGENTS.md` from the workspace root to `cwd`, root first and nearest last.
/// Performs bounded synchronous I/O; async callers should use a blocking worker.
///
/// # Errors
/// Rejects escaping symlinks, invalid directories, nonregular files, invalid UTF-8 and I/O failures.
pub fn discover(
    workspace: &WorkspaceRoot,
    cwd: &Path,
) -> Result<Vec<InstructionFile>, PromptError> {
    let cwd = workspace.resolve(cwd.to_str().ok_or(PromptError::InvalidPath)?)?;
    if !cwd.is_dir() {
        return Err(PromptError::InvalidPath);
    }
    let relative = cwd
        .strip_prefix(workspace.path())
        .map_err(|_| PromptError::InvalidPath)?;
    let mut directories: Vec<_> = relative.ancestors().collect();
    directories.reverse();
    let mut files: Vec<InstructionFile> = Vec::new();
    let mut remaining = MAX_INSTRUCTION_TOTAL_BYTES;
    for directory in directories {
        let relative = directory.join("AGENTS.md");
        let path = workspace.resolve(relative.to_str().ok_or(PromptError::InvalidPath)?)?;
        match fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
            Ok(metadata) if !metadata.is_file() => return Err(PromptError::InvalidPath),
            Ok(_) => {}
        }
        if remaining < TOTAL_MARKER.len() {
            if let Some(last) = files.last_mut() {
                let limit = last.content.len();
                truncate(&mut last.content, limit, TOTAL_MARKER);
                last.truncated = true;
            }
            break;
        }
        let limit = remaining.min(MAX_INSTRUCTION_FILE_BYTES);
        let (content, truncated) = read(&path, limit)?;
        remaining -= content.len();
        files.push(InstructionFile {
            path: relative,
            content,
            truncated,
        });
    }
    Ok(files)
}
fn read(path: &Path, limit: usize) -> Result<(String, bool), PromptError> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    let valid = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(error) if truncated && error.error_len().is_none() => error.valid_up_to(),
        Err(_) => return Err(PromptError::InvalidEncoding),
    };
    bytes.truncate(valid);
    let mut text = String::from_utf8(bytes).map_err(|_| PromptError::InvalidEncoding)?;
    if truncated {
        truncate(&mut text, limit, FILE_MARKER);
    }
    Ok((text, truncated))
}
fn truncate(text: &mut String, limit: usize, marker: &str) {
    let mut end = text.len().min(limit.saturating_sub(marker.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(marker);
}

/// A profile's instructions as their own section of the instruction slot, placed before the repository's files: the
/// administrator's or owner's text for every run under the profile (ADR-0012 decision 6). Empty without instructions.
#[must_use]
pub fn render_profile_instructions(profile: &str, instructions: Option<&str>) -> String {
    instructions.map_or_else(String::new, |text| {
        format!("--- Profile instructions ({profile}) ---\n{text}\n\n")
    })
}

/// Renders files in discovery order with a workspace-relative header for each.
#[must_use]
pub fn render(files: &[InstructionFile]) -> String {
    let mut rendered = String::new();
    for file in files {
        // Writing to an owned String cannot fail.
        let _ = writeln!(
            rendered,
            "--- {} ---\n{}\n",
            file.path.display(),
            file.content
        );
    }
    rendered
}
