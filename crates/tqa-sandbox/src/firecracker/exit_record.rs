//! The exit record (ADR-0006): a one-sector raw device the guest init writes the runtime's exit code to, because a
//! guest reset always ends Firecracker with status 0. Only root can open the device inside the guest, so no sandbox
//! process can forge it; a missing or torn record reads as "no code", like a VM that died.

use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

use super::acl::{self, Access};
use crate::SandboxError;

/// The record's file name in the per-sandbox directory.
pub(crate) const FILE: &str = "exit.img";
const MAGIC: &[u8; 8] = b"TQAEXIT1";
const SIZE: usize = 4096;

/// Creates the zeroed record device, private to its owner plus a read-write ACL entry for the VMM uid.
///
/// # Errors
/// Returns `Preflight` without ACL support, otherwise redacted `Backend` errors.
pub(crate) fn create(directory: &Path, uid: u32) -> Result<(), SandboxError> {
    let storage =
        |error: std::io::Error| SandboxError::Backend(format!("exit record: {}", error.kind()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join(FILE))
        .map_err(storage)?;
    file.write_all(&[0; SIZE])
        .and_then(|()| file.sync_all())
        .map_err(storage)?;
    acl::grant(&file, uid, Access::ReadWrite)
}

/// The recorded code, if a complete record exists.
pub(crate) fn read(directory: &Path) -> Option<i32> {
    let mut head = [0; 16];
    let mut file = std::fs::File::open(directory.join(FILE)).ok()?;
    file.read_exact(&mut head).ok()?;
    decode(&head)
}

fn decode(head: &[u8; 16]) -> Option<i32> {
    let code = i32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    let check = i32::from_le_bytes([head[12], head[13], head[14], head[15]]);
    (&head[..8] == MAGIC && check == !code).then_some(code)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::{FILE, create, decode, read};

    #[test]
    fn sbx_f_018_record_decodes_only_when_complete() {
        // The same bytes the guest init's encoder produces for code 7 (tqa-vm-init, exit_record).
        let written: &[u8; 16] = b"TQAEXIT1\x07\x00\x00\x00\xf8\xff\xff\xff";
        assert_eq!(decode(written), Some(7));
        assert_eq!(decode(&[0; 16]), None, "never written");
        let mut torn = *written;
        torn[13] = 0;
        assert_eq!(decode(&torn), None, "code and check disagree");
        let mut foreign = *written;
        foreign[0] = b'X';
        assert_eq!(decode(&foreign), None);
    }

    #[test]
    fn sbx_f_018_record_file_is_private_and_starts_empty() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        create(directory.path(), 4242)?;
        assert_eq!(read(directory.path()), None);
        assert_eq!(std::fs::metadata(directory.path().join(FILE))?.len(), 4096);
        std::fs::write(
            directory.path().join(FILE),
            b"TQAEXIT1\x01\x00\x00\x00\xfe\xff\xff\xff",
        )?;
        assert_eq!(read(directory.path()), Some(1));
        assert!(create(directory.path(), 4242).is_err(), "never overwritten");
        Ok(())
    }
}
