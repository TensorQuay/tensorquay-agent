//! The exit record: the runtime's exit code, written once by the init to a dedicated raw block device that only root
//! can open in the guest, so no sandbox process can forge it. The host backend reads it after the VM ends (ADR-0006).
//! One 512-byte sector: the magic, the code and its bitwise complement; anything else reads as "no record".

use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, process::ExitStatus};

use nix::fcntl::OFlag;

use crate::error::{InitError, io};

/// The device the host attaches for the record.
pub const DEVICE: &str = "/dev/vde";
const MAGIC: &[u8; 8] = b"TQAEXIT1";
const SECTOR: usize = 512;

/// The code recorded when the sandbox could not be started at all.
pub const INIT_FAILED: i32 = 126;

/// The code the host reports for a finished sandbox: its exit code, or 128 plus the terminating signal.
#[must_use]
pub fn code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(-1)
}

/// The sector holding `code`.
#[must_use]
pub fn encode(code: i32) -> [u8; SECTOR] {
    let mut sector = [0; SECTOR];
    sector[..8].copy_from_slice(MAGIC);
    sector[8..12].copy_from_slice(&code.to_le_bytes());
    sector[12..16].copy_from_slice(&(!code).to_le_bytes());
    sector
}

/// Writes the record synchronously, as one sector.
///
/// # Errors
/// Returns the failing step; the caller powers off regardless.
pub fn write(code: i32) -> Result<(), InitError> {
    let mut device = OpenOptions::new()
        .write(true)
        .custom_flags(OFlag::O_SYNC.bits())
        .open(DEVICE)
        .map_err(io("open exit record"))?;
    device
        .write_all(&encode(code))
        .map_err(io("write exit record"))?;
    device.sync_all().map_err(io("sync exit record"))
}

#[cfg(test)]
mod tests {
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    use super::{code, encode};

    #[test]
    fn sbx_f_018_record_layout_matches_the_host_contract() {
        // The same 16 bytes are asserted by the host backend's decoder (tqa-sandbox, firecracker::exit_record).
        let sector = encode(7);
        assert_eq!(&sector[..16], b"TQAEXIT1\x07\x00\x00\x00\xf8\xff\xff\xff");
        assert!(sector[16..].iter().all(|&byte| byte == 0));
        assert_eq!(encode(-1)[8..16], [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    }

    #[test]
    fn sbx_f_018_codes_follow_the_shell_convention() {
        assert_eq!(code(ExitStatus::from_raw(3 << 8)), 3);
        assert_eq!(code(ExitStatus::from_raw(9)), 128 + 9);
    }
}
