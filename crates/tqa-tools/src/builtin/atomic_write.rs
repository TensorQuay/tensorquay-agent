//! Shared same-directory staging, permission preservation and atomic replacement.
use super::file_io;
use crate::{ToolError, WorkspaceRoot};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tokio_util::sync::CancellationToken;

pub(super) fn replace(
    workspace: &WorkspaceRoot,
    requested: &str,
    content: &str,
    cancel: &CancellationToken,
    before_rename: impl FnOnce(&Path) -> Result<(), ToolError>,
) -> Result<(), ToolError> {
    file_io::cancelled(cancel)?;
    let path = workspace.resolve(requested)?;
    let permissions = match path.metadata() {
        Ok(metadata) if metadata.is_file() => Some(metadata.permissions()),
        Ok(_) => return Err(ToolError::PathInvalid("expected regular file".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::PathInvalid("missing parent directory".into()))?;
    let (mut file, temporary) = stage(parent)?;
    let result = (|| {
        if let Some(permissions) = &permissions {
            file.set_permissions(permissions.clone())?;
        }
        for chunk in content.as_bytes().chunks(8192) {
            file_io::cancelled(cancel)?;
            file.write_all(chunk)?;
        }
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        file_io::cancelled(cancel)?;
        // Recheck canonical parents immediately before replacement.
        if workspace.resolve(requested)? != path {
            return Err(ToolError::PathOutsideWorkspace);
        }
        before_rename(&path)?;
        fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if result.is_err()
        && let Err(error) = fs::remove_file(&temporary)
    {
        tracing::warn!(kind = ?error.kind(), "staging file cleanup failed");
    }
    result
}

fn stage(parent: &Path) -> Result<(File, PathBuf), ToolError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    const MAX_STAGE_ATTEMPTS: usize = 16;
    for _ in 0..MAX_STAGE_ATTEMPTS {
        let path = parent.join(format!(
            ".tqa-write-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(ToolError::Io("cannot allocate staging file".into()))
}
