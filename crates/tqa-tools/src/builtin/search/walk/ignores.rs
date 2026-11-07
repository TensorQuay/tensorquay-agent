//! Validate each ignore file before the library loads that directory's rules.
use crate::{ToolError, builtin::file_io};
use ignore::gitignore::GitignoreBuilder;
use std::{fs, path::Path};
use tokio_util::sync::CancellationToken;

pub(super) fn validate(
    directory: &Path,
    max: usize,
    cancel: &CancellationToken,
) -> Result<(), ToolError> {
    file_io::cancelled(cancel)?;
    match fs::symlink_metadata(directory.join(".git")) {
        Ok(metadata) if !metadata.is_dir() => {
            return Err(ToolError::PathInvalid(
                "git ignore metadata must be a workspace directory".into(),
            ));
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    if let Ok(metadata) = fs::symlink_metadata(directory.join(".git/info"))
        && !metadata.is_dir()
    {
        return Err(ToolError::PathInvalid(
            "git exclude directory must not be a symlink".into(),
        ));
    }
    for name in [".gitignore", ".ignore", ".tqaignore", ".git/info/exclude"] {
        let path = directory.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file() {
            return Err(ToolError::PathInvalid(
                "ignore file must be regular and not a symlink".into(),
            ));
        }
        if metadata.len() > max as u64 {
            return Err(ToolError::OutputTooLarge { max });
        }
        let bytes = file_io::read_bounded(
            &mut file_io::open_regular(&path)?.0,
            max.saturating_add(1),
            cancel,
        )?;
        if bytes.len() > max {
            return Err(ToolError::OutputTooLarge { max });
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| ToolError::invalid("/ignore", "ignore file must be UTF-8"))?;
        let mut builder = GitignoreBuilder::new(directory);
        for (index, line) in text.lines().enumerate() {
            builder.add_line(None, line).map_err(|_| {
                ToolError::invalid(
                    "/ignore",
                    &format!("invalid ignore glob at line {}", index + 1),
                )
            })?;
        }
    }
    Ok(())
}
