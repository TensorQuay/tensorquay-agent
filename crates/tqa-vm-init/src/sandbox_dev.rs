//! The sandbox's own `/dev`: a private tmpfs with the character devices a process needs and nothing else, so no block
//! device (the root, workspace, state, configuration or exit device) exists inside the sandbox, as in Docker mode
//! (SBX-C-016). Built by stage two in its private mount namespace while it is still root.

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

use nix::{
    mount::{MsFlags, mount},
    sys::stat::{Mode, SFlag, makedev, mknod},
};

use crate::error::{InitError, io, sys};

/// Character devices: name, major, minor. Readable and writable by everyone, as in a container's `/dev`.
pub const NODES: [(&str, u64, u64); 6] = [
    ("null", 1, 3),
    ("zero", 1, 5),
    ("full", 1, 7),
    ("random", 1, 8),
    ("urandom", 1, 9),
    ("tty", 5, 0),
];

/// Links into the process's own descriptors, and to the private pseudo-terminal multiplexer.
pub const LINKS: [(&str, &str); 5] = [
    ("fd", "/proc/self/fd"),
    ("stdin", "/proc/self/fd/0"),
    ("stdout", "/proc/self/fd/1"),
    ("stderr", "/proc/self/fd/2"),
    ("ptmx", "pts/ptmx"),
];

/// Replaces `/dev` in the current mount namespace.
///
/// # Errors
/// Returns the failing step; the sandbox is not started without its `/dev`.
pub fn build() -> Result<(), InitError> {
    let flags = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC;
    mount(
        Some("tmpfs"),
        "/dev",
        Some("tmpfs"),
        flags,
        Some("mode=0755,size=64k"),
    )
    .map_err(sys("sandbox /dev"))?;
    let dev = Path::new("/dev");
    for (name, major, minor) in NODES {
        mknod(
            &dev.join(name),
            SFlag::S_IFCHR,
            Mode::from_bits_truncate(0o666),
            makedev(major, minor),
        )
        .map_err(sys("sandbox device node"))?;
        // The mode given to mknod is filtered by the umask; set it explicitly so the sandbox user can write.
        fs::set_permissions(dev.join(name), fs::Permissions::from_mode(0o666))
            .map_err(io("sandbox device mode"))?;
    }
    for (name, target) in LINKS {
        symlink(target, dev.join(name)).map_err(io("sandbox device link"))?;
    }
    for directory in ["pts", "shm"] {
        fs::create_dir(dev.join(directory)).map_err(io("sandbox device directory"))?;
    }
    mount(
        Some("devpts"),
        "/dev/pts",
        Some("devpts"),
        MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC,
        Some("newinstance,ptmxmode=0666,mode=0620"),
    )
    .map_err(sys("sandbox pseudo-terminals"))?;
    mount(
        Some("tmpfs"),
        "/dev/shm",
        Some("tmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
        Some("mode=1777,size=64m"),
    )
    .map_err(sys("sandbox shared memory"))
}

#[cfg(test)]
mod tests {
    use super::{LINKS, NODES};

    #[test]
    fn sbx_f_016_sandbox_dev_has_character_devices_only() {
        // Majors 1 (memory devices) and 5 (tty); disk majors (virtio-blk, loop, device mapper) never appear.
        assert!(NODES.iter().all(|&(_, major, _)| major == 1 || major == 5));
        let names: Vec<_> = NODES.iter().map(|&(name, _, _)| name).collect();
        assert_eq!(names, ["null", "zero", "full", "random", "urandom", "tty"]);
        assert!(
            LINKS
                .iter()
                .all(|&(_, target)| target.starts_with("/proc/self/fd") || target == "pts/ptmx")
        );
    }
}
