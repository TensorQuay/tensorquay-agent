//! Same-directory staging and atomic replacement, with cancellation before commit.
use super::{arguments, atomic_write, file_io, specification};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec, WorkspaceRoot};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};
use tokio_util::sync::CancellationToken;

/// Atomically creates or replaces regular workspace files.
pub struct WriteFile {
    spec: ToolSpec,
}
impl WriteFile {
    /// Constructs the builtin write schema.
    #[must_use]
    pub fn new() -> Self {
        Self {
            spec: ToolSpec {
                // A success may have changed files: a repeated failure counts afresh after it (backlog #18).
                changes_workspace: true,
                ..specification(
                    "write_file",
                    "Write workspace text atomically",
                    json!({
                        "type":"object", "required":["path","content","create_parents"], "additionalProperties":false,
                        "properties":{"path":{"type":"string"},"content":{"type":"string"},"create_parents":{"type":"boolean"}}
                    }),
                    false,
                )
            },
        }
    }
}
impl Default for WriteFile {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    path: String,
    content: String,
    create_parents: bool,
}
#[async_trait]
impl Tool for WriteFile {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args: Args = arguments(args)?;
        let workspace = cx.workspace.clone();
        file_io::run(cx, move |cancel| {
            write(&workspace, &args, cancel, || Ok(()))
        })
        .await
    }
}
fn write(
    workspace: &WorkspaceRoot,
    args: &Args,
    cancel: &CancellationToken,
    before_rename: impl FnOnce() -> Result<(), ToolError>,
) -> Result<ToolOutput, ToolError> {
    file_io::cancelled(cancel)?;
    let path = workspace.resolve(&args.path)?;
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::PathInvalid("missing parent directory".into()))?;
    let previous = previous_lines(&path, cancel)?;
    if args.create_parents {
        fs::create_dir_all(parent)?;
    }
    atomic_write::replace(workspace, &args.path, &args.content, cancel, |_| {
        before_rename()
    })?;
    Ok(ToolOutput::text(format!(
        "{}, {} bytes\nadded: {} lines\nremoved: {} lines",
        if previous.is_some() {
            "replaced"
        } else {
            "created"
        },
        args.content.len(),
        args.content.lines().count(),
        previous.unwrap_or(0)
    )))
}
fn previous_lines(path: &Path, cancel: &CancellationToken) -> Result<Option<usize>, ToolError> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Err(ToolError::PathInvalid("expected regular file".into()));
    }
    let mut file = File::open(path)?;
    let mut buffer = [0; 8192];
    let (mut count, mut last) = (0, None);
    loop {
        file_io::cancelled(cancel)?;
        let bytes = file.read(&mut buffer)?;
        if bytes == 0 {
            break;
        }
        count += buffer[..bytes]
            .iter()
            .map(|&byte| usize::from(byte == b'\n'))
            .sum::<usize>();
        last = buffer.get(bytes - 1).copied();
    }
    Ok(Some(
        count + usize::from(last.is_some_and(|byte| byte != b'\n')),
    ))
}

#[cfg(test)]
mod tests;
