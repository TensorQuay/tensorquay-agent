//! Uploads into `/workspace/uploads/` (FR-36, GW-I-045). A name is sanitised to the attachment contract, an existing
//! file is never overwritten (`name-2.ext`), and the per-file and per-workspace limits are checked before anything is
//! kept: the bytes go to a hidden temporary file, which is linked into place only when complete and within bounds.
//! Contents are not scanned; uploads are untrusted data that the gateway serves only as downloads.
use crate::GatewayError;
use rustix::fs::{self as sys, AtFlags, Mode, OFlags};
use std::io::Write;
use std::os::fd::{AsFd, BorrowedFd};
use tqa_tools::workspace_tree::upload::{self as shared, TEMPORARY_PREFIX};
pub(crate) use tqa_tools::workspace_tree::upload::{DIRECTORY, sanitise};

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;

/// Upload limits in bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Limits {
    pub per_file: u64,
    pub per_workspace: u64,
}

/// A stored upload: its workspace-relative path (the attachment reference) and size.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Stored {
    pub path: String,
    pub size: u64,
}

/// Stores one upload under `uploads/` of the workspace at `root`.
///
/// # Errors
/// `413 upload_too_large` over the per-file limit, `409 uploads_full` over the workspace's upload space,
/// `path_invalid` when `uploads` is a link, host I/O failures. Nothing is kept on any error.
pub(crate) fn store(
    root: BorrowedFd<'_>,
    name: &str,
    chunks: &[Vec<u8>],
    limits: Limits,
) -> Result<Stored, GatewayError> {
    let total: u64 = chunks.iter().map(|chunk| chunk.len() as u64).sum();
    if total > limits.per_file {
        return Err(GatewayError::UploadTooLarge);
    }
    let directory = shared::directory(root)?;
    if shared::used_in(directory.as_fd())?.saturating_add(total) > limits.per_workspace {
        return Err(GatewayError::Conflict("uploads_full"));
    }
    let temporary = format!("{TEMPORARY_PREFIX}{}", uuid::Uuid::now_v7().simple());
    let written = write(directory.as_fd(), &temporary, chunks);
    let linked = written.and_then(|()| link(directory.as_fd(), &temporary, &sanitise(name)));
    let _ = sys::unlinkat(&directory, temporary.as_str(), AtFlags::empty());
    let stored = linked?;
    Ok(Stored {
        path: format!("{DIRECTORY}/{stored}"),
        size: total,
    })
}

/// Bytes stored in the workspace's `uploads/` (regular files only; 0 when it does not exist yet).
///
/// # Errors
/// `path_invalid` when `uploads` is a link; host I/O failures.
#[cfg(test)]
pub(crate) fn used(root: BorrowedFd<'_>) -> Result<u64, GatewayError> {
    match tqa_tools::workspace_tree::path::walk(root, &[DIRECTORY.to_owned()], |_| ()) {
        Ok(directory) => Ok(shared::used_in(directory.as_fd())?),
        Err(tqa_tools::workspace_tree::TreeError::NotFound) => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn write(directory: BorrowedFd<'_>, name: &str, chunks: &[Vec<u8>]) -> Result<(), GatewayError> {
    let flags = OFlags::WRONLY
        .union(OFlags::CREATE)
        .union(OFlags::EXCL)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    let fd = sys::openat(directory, name, flags, Mode::from_raw_mode(0o644))
        .map_err(|_| GatewayError::Io)?;
    let mut file = std::fs::File::from(fd);
    for chunk in chunks {
        file.write_all(chunk).map_err(|_| GatewayError::Io)?;
    }
    file.sync_all().map_err(|_| GatewayError::Io)
}

/// Links the finished temporary file to the first free name, never overwriting one.
fn link(directory: BorrowedFd<'_>, temporary: &str, name: &str) -> Result<String, GatewayError> {
    shared::link(directory, temporary, name)?.ok_or(GatewayError::Conflict("uploads_full"))
}
