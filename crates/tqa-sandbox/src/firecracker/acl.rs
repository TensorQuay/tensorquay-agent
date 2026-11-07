//! One-entry POSIX ACLs set by a file's owner, so the VMM uid can open exactly the per-VM files it needs while the
//! owner keeps them (ADR-0006). The kernel's `system.posix_acl_access` encoding is written directly: a version word,
//! then entries sorted by tag, each a tag, a permission set and an id.

use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

use rustix::fs::{XattrFlags, fgetxattr, fsetxattr};

use crate::SandboxError;

const XATTR: &str = "system.posix_acl_access";
const VERSION: u32 = 2;
const USER_OBJ: u16 = 0x01;
const USER: u16 = 0x02;
const GROUP_OBJ: u16 = 0x04;
const MASK: u16 = 0x10;
const OTHER: u16 = 0x20;
const UNDEFINED_ID: u32 = u32::MAX;
const UNSUPPORTED: &str = "storage.root does not support POSIX ACLs";

/// The access granted to the VMM uid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Access {
    /// Read only, for the configuration device and launch files.
    Read,
    /// Read and write, for the workspace and state images.
    ReadWrite,
}

impl Access {
    const fn bits(self) -> u16 {
        match self {
            Self::Read => 4,
            Self::ReadWrite => 6,
        }
    }
}

/// The encoded ACL for a private file (owner read-write, no group or other access) plus one named user. The mask
/// equals the named entry, so the owning group's entry stays empty and other users get nothing.
pub(crate) fn encode(uid: u32, access: Access) -> Vec<u8> {
    let entries = [
        (USER_OBJ, 6, UNDEFINED_ID),
        (USER, access.bits(), uid),
        (GROUP_OBJ, 0, UNDEFINED_ID),
        (MASK, access.bits(), UNDEFINED_ID),
        (OTHER, 0, UNDEFINED_ID),
    ];
    let mut bytes = VERSION.to_le_bytes().to_vec();
    for (tag, permissions, id) in entries {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&permissions.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes
}

/// Makes `file` private to its owner plus `uid`, replacing any previous ACL. Idempotent.
///
/// # Errors
/// Returns `Preflight` when the filesystem has no ACL support, otherwise a redacted `Backend` error.
pub(crate) fn grant(file: &File, uid: u32, access: Access) -> Result<(), SandboxError> {
    if uid == 0 {
        return Err(SandboxError::invalid(
            "vmm_file_uid",
            "the VMM must not run as root",
        ));
    }
    let wanted = encode(uid, access);
    fsetxattr(file, XATTR, &wanted, XattrFlags::empty()).map_err(set_error)?;
    let mut stored = vec![0; wanted.len() + 64];
    let length = fgetxattr(file, XATTR, &mut stored[..])
        .map_err(|error| SandboxError::Backend(format!("reading a file ACL failed: {error}")))?;
    if stored.get(..length) != Some(&wanted[..]) {
        return Err(SandboxError::Backend(
            "the stored file ACL differs from the one written".into(),
        ));
    }
    Ok(())
}

/// A filesystem without ACL support fails closed with a reason an administrator can act on.
fn set_error(error: rustix::io::Errno) -> SandboxError {
    if error == rustix::io::Errno::OPNOTSUPP {
        SandboxError::Preflight(UNSUPPORTED)
    } else {
        SandboxError::Backend(format!("setting a file ACL failed: {error}"))
    }
}

/// Proves once that `directory` can hold the per-VM ACLs: a probe file is created, granted, read back and removed.
///
/// # Errors
/// Returns `Preflight` with a clear reason when ACLs are unsupported; the probe file never survives.
pub(crate) fn probe(directory: &Path, uid: u32) -> Result<(), SandboxError> {
    let path = directory.join(format!(".tqa-acl-probe-{}", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| {
            SandboxError::Backend(format!("creating the ACL probe failed: {}", error.kind()))
        })?;
    let result = grant(&file, uid, Access::Read);
    drop(file);
    match std::fs::remove_file(&path) {
        Ok(()) => result,
        Err(error) if error.kind() == io::ErrorKind::NotFound => result,
        Err(error) => Err(SandboxError::Backend(format!(
            "removing the ACL probe failed: {}",
            error.kind()
        ))),
    }
}

#[cfg(test)]
#[path = "acl_tests.rs"]
mod tests;
