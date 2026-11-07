//! An agent team's handed-back files (ADR-0034 point 6, slice 5), as files: a reported file is read from the member's
//! workspace into the gateway's store, and placed in the lead's `uploads/` exactly as a person's attachment is, with
//! its next free name. On Docker the workspace's directory is read and written through no-follow opens; on the microVM
//! its runtime's workspace service does both (ADR-0030), so neither needs a new operation.
use std::{
    os::fd::AsFd as _,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::StreamExt as _;
use sha2::Digest as _;
use tokio::io::AsyncWriteExt as _;
use tqa_sandbox::ReadinessSource as _;
use tqa_types::ConversationId;
use uuid::Uuid;

use super::{
    endpoints::blocking,
    path::{self, WorkspacePath},
    upload::{self, Limits},
};
use crate::{GatewayError, app::State};

/// Where a task's workspace is read and written.
pub(crate) enum Workspace {
    /// Its directory on the host (Docker).
    Directory(PathBuf),
    /// The task's own live runtime (microVM installs): the task is running, since it is the one calling.
    Runtime(ConversationId),
}

/// The workspace of `conversation`, a task that is running now.
///
/// # Errors
/// `409 files_unavailable` on the microVM when its runtime isn't linked; `404` for a deleted task; store failures.
pub(crate) async fn workspace(
    state: &State,
    conversation: Uuid,
) -> Result<Workspace, GatewayError> {
    if state.config.uses_block_images() {
        let id: ConversationId = conversation
            .to_string()
            .parse()
            .map_err(|_| GatewayError::Io)?;
        return if state.links.is_ready(&id).await {
            Ok(Workspace::Runtime(id))
        } else {
            Err(GatewayError::FilesUnavailable)
        };
    }
    let binding = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        crate::workspaces::of_conversation(&state.pool, conversation).await
    })
    .await?;
    Ok(Workspace::Directory(
        state.storage.workspace(binding.id).await?,
    ))
}

/// The limits a handed-back file is held to: an attachment's (`files.upload_bytes` each, `files.uploads_bytes` for a
/// workspace's uploads).
pub(crate) fn limits(state: &State) -> Limits {
    Limits {
        per_file: state.config.files.upload_bytes,
        per_workspace: state.config.files.uploads_bytes,
    }
}

/// Copies the regular file `path` of `from` to the new file `to`, within `limit` bytes. Answers its size and SHA-256
/// (lowercase hex).
///
/// # Errors
/// `400 path_invalid` (a link anywhere in the path, a directory, anything but a regular file), `404`, `413
/// upload_too_large` over `limit`, `409 file_changed` when it changes while read, `409 files_unavailable`, I/O.
pub(crate) async fn copy_out(
    state: &Arc<State>,
    from: &Workspace,
    path: &str,
    to: &Path,
    limit: u64,
) -> Result<(u64, String), GatewayError> {
    let target = WorkspacePath::parse(path)?;
    match from {
        Workspace::Directory(root) => {
            let (root, to) = (root.clone(), to.to_owned());
            blocking(move || copy_from_directory(&root, &target, &to, limit)).await
        }
        Workspace::Runtime(link) => {
            let (size, chunks) =
                super::linked::download(Arc::clone(state), link.clone(), &target, limit)
                    .await
                    .map_err(|error| match error {
                        GatewayError::BodyTooLarge => GatewayError::UploadTooLarge,
                        other => other,
                    })?;
            let mut chunks = std::pin::pin!(chunks);
            let mut file = new_file(to).await?;
            let mut hash = sha2::Sha256::new();
            let mut written: u64 = 0;
            while let Some(chunk) = chunks.next().await {
                let chunk = chunk?;
                written = written.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
                if written > size {
                    return Err(GatewayError::FileChanged);
                }
                hash.update(&chunk);
                file.write_all(&chunk).await?;
            }
            file.sync_all().await?;
            Ok((written, crate::hex::hex(&hash.finalize()[..])))
        }
    }
}

/// [`copy_out`] from a workspace directory (Docker), for callers that hold no shared state: the gateway's hand-back of
/// a stopped helper's files (GW-I-311).
///
/// # Errors
/// As [`copy_out`].
pub(crate) async fn copy_out_directory(
    root: &Path,
    path: &str,
    to: &Path,
    limit: u64,
) -> Result<(u64, String), GatewayError> {
    let target = WorkspacePath::parse(path)?;
    let (root, to) = (root.to_owned(), to.to_owned());
    blocking(move || copy_from_directory(&root, &target, &to, limit)).await
}

/// [`copy_out`] on Docker: the file opened without following a link, read through its descriptor.
fn copy_from_directory(
    root: &Path,
    target: &WorkspacePath,
    to: &Path,
    limit: u64,
) -> Result<(u64, String), GatewayError> {
    use std::io::{Read as _, Write as _};
    let root = path::open_root(root)?;
    let (file, stat) = path::open_file(root.as_fd(), target)?;
    if u64::try_from(stat.st_size).unwrap_or(u64::MAX) > limit {
        return Err(GatewayError::UploadTooLarge);
    }
    let mut source = std::fs::File::from(file);
    let mut copy = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    let mut hash = sha2::Sha256::new();
    let mut written: u64 = 0;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        written = written.saturating_add(read as u64);
        // Grown since it was opened: the limit holds for the bytes kept, not only the size first seen.
        if written > limit {
            return Err(GatewayError::UploadTooLarge);
        }
        hash.update(&buffer[..read]);
        copy.write_all(&buffer[..read])?;
    }
    copy.sync_all()?;
    Ok((written, crate::hex::hex(&hash.finalize()[..])))
}

async fn new_file(path: &Path) -> Result<tokio::fs::File, GatewayError> {
    Ok(tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?)
}

/// Places the stored file `from` in the `uploads/` of `into` as an attachment named `name` (sanitised; a name already
/// taken gets the next free one, so nothing is overwritten). Answers its workspace path and size.
///
/// # Errors
/// `413 upload_too_large`, `409 uploads_full`, `409 files_unavailable`, `path_invalid` when `uploads` is a link, I/O.
pub(crate) async fn place(
    state: &State,
    into: &Workspace,
    name: &str,
    from: &Path,
) -> Result<(String, u64), GatewayError> {
    let name = upload::sanitise(name);
    let limits = limits(state);
    let bytes = tokio::fs::read(from).await?;
    match into {
        Workspace::Directory(root) => {
            let root = root.clone();
            let stored = blocking(move || {
                let root = path::open_root(&root)?;
                upload::store(root.as_fd(), &name, &[bytes], limits)
            })
            .await?;
            Ok((stored.path, stored.size))
        }
        Workspace::Runtime(link) => {
            let body = futures::stream::once(async move {
                Ok::<_, std::convert::Infallible>(bytes::Bytes::from(bytes))
            });
            let stored = super::linked::upload(state, link, &name, Box::pin(body), limits).await?;
            Ok((stored.path, stored.size))
        }
    }
}
