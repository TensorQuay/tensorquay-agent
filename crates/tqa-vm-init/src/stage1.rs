//! Stage one runs as the guest's PID 1 with full privileges: it builds the sandbox's filesystem view, delivers the
//! token, applies the limits, starts stage two in a new PID namespace and powers the VM off when it ends.

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt, fchown},
    path::Path,
    process::{Command, ExitStatus},
};

use nix::{
    mount::{MsFlags, mount},
    sched::{CloneFlags, unshare},
    sys::reboot::{RebootMode, reboot, set_cad_enabled},
    unistd::{Gid, Uid, chown, sync},
};
use secrecy::ExposeSecret;

use crate::{
    config::{GuestConfig, ImageConfig, MAX_DOCUMENT_BYTES, parse_guest, parse_image},
    error::{InitError, io, sys},
    plan,
};

const NOSUID_NODEV: MsFlags = MsFlags::MS_NOSUID.union(MsFlags::MS_NODEV);
/// Sysctls written before any sandbox process exists; user namespaces off lets the filter leave `clone` alone.
const SYSCTLS: [(&str, &str); 5] = [
    ("user/max_user_namespaces", "0"),
    ("kernel/kptr_restrict", "2"),
    ("kernel/dmesg_restrict", "1"),
    // Dirty data is written back within a second (ARCHITECTURE §4.5): a VM crash loses the guest's page cache. This is
    // defence in depth for background writes; tool results rely on the runtime's flush (`sync_before_result`).
    ("vm/dirty_expire_centisecs", WRITE_BACK_CENTISECONDS),
    ("vm/dirty_writeback_centisecs", WRITE_BACK_CENTISECONDS),
];
/// One second, in the kernel's unit for the write-back sysctls.
const WRITE_BACK_CENTISECONDS: &str = "100";
/// The workspace's ext4 journal commits every second (the default is five), for the same reason.
const WORKSPACE_MOUNT_OPTIONS: &str = "commit=1";

/// Sets the guest up, runs the sandbox and reports how it ended.
///
/// # Errors
/// Returns the first failing step; the caller powers the VM off either way.
pub fn run() -> Result<ExitStatus, InitError> {
    set_cad_enabled(false).map_err(sys("disable ctrl-alt-del"))?;
    mount_base()?;
    let image = parse_image(&read_bounded(Path::new(plan::IMAGE_CONFIG))?)?;
    let guest = parse_guest(&read_bounded(Path::new(plan::CONFIG_DEVICE))?)?;
    mount_storage(&image)?;
    write_token(&guest, &image)?;
    bind_name_files(&guest)?;
    for (name, value) in SYSCTLS {
        fs::write(Path::new("/proc/sys").join(name), value).map_err(io("sysctl"))?;
    }
    sandbox_cgroup(guest.pids)?;
    fs::write("/proc/self/oom_score_adj", "-1000").map_err(io("oom score"))?;
    // Children created from here on are in the sandbox's own PID namespace; stage two becomes its PID 1.
    unshare(CloneFlags::CLONE_NEWPID).map_err(sys("pid namespace"))?;
    Command::new("/proc/self/exe")
        .args(plan::stage2_args(&image))
        .env_clear()
        .envs(plan::runtime_env(&image, &guest))
        .current_dir(&image.working_dir)
        .status()
        .map_err(io("start sandbox"))
}

/// Flushes the storage and ends the VM; the VMM exits on the guest's reset.
pub fn power_off() -> ! {
    sync();
    for target in [plan::WORKSPACE_DIR, plan::STATE_DIR] {
        let _ = nix::mount::umount2(target, nix::mount::MntFlags::MNT_DETACH);
    }
    sync();
    loop {
        let _ = reboot(RebootMode::RB_AUTOBOOT);
    }
}

fn mount_base() -> Result<(), InitError> {
    let base: [(&str, &str, MsFlags, Option<&str>); 6] = [
        ("proc", "/proc", NOSUID_NODEV | MsFlags::MS_NOEXEC, None),
        (
            "sysfs",
            "/sys",
            NOSUID_NODEV | MsFlags::MS_NOEXEC | MsFlags::MS_RDONLY,
            None,
        ),
        (
            "devtmpfs",
            "/dev",
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC,
            Some("mode=0755"),
        ),
        (
            "cgroup2",
            "/sys/fs/cgroup",
            NOSUID_NODEV | MsFlags::MS_NOEXEC,
            None,
        ),
        ("tmpfs", "/tmp", NOSUID_NODEV, Some("mode=1777")),
        (
            "tmpfs",
            "/run",
            NOSUID_NODEV | MsFlags::MS_NOEXEC,
            Some("mode=0755"),
        ),
    ];
    for (kind, target, flags, data) in base {
        mount(Some(kind), target, Some(kind), flags, data).map_err(sys("mount base filesystem"))?;
    }
    Ok(())
}

fn mount_storage(image: &ImageConfig) -> Result<(), InitError> {
    fs::create_dir_all(plan::STATE_DIR).map_err(io("state directory"))?;
    let owner = (
        Some(Uid::from_raw(image.uid)),
        Some(Gid::from_raw(image.gid)),
    );
    for (device, target, options) in [
        (
            plan::WORKSPACE_DEVICE,
            plan::WORKSPACE_DIR,
            Some(WORKSPACE_MOUNT_OPTIONS),
        ),
        (plan::STATE_DEVICE, plan::STATE_DIR, None),
    ] {
        mount(Some(device), target, Some("ext4"), NOSUID_NODEV, options)
            .map_err(sys("mount storage"))?;
        chown(target, owner.0, owner.1).map_err(sys("own storage root"))?;
    }
    Ok(())
}

fn write_token(guest: &GuestConfig, image: &ImageConfig) -> Result<(), InitError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(plan::TOKEN_FILE)
        .map_err(io("token file"))?;
    fchown(&file, Some(image.uid), Some(image.gid)).map_err(io("token owner"))?;
    file.write_all(guest.token.expose_secret().as_bytes())
        .map_err(io("token write"))?;
    file.sync_all().map_err(io("token sync"))
}

/// The root filesystem is read-only, so the hosts and resolver files are bound over it from a private tmpfs.
fn bind_name_files(guest: &GuestConfig) -> Result<(), InitError> {
    let directory = Path::new("/run/tqa-vm");
    fs::create_dir(directory).map_err(io("name files"))?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).map_err(io("name files"))?;
    for (name, text) in [
        ("hosts", plan::hosts_file(&guest.hosts)),
        ("resolv.conf", String::new()),
    ] {
        let source = directory.join(name);
        fs::write(&source, text).map_err(io("name file"))?;
        let target = Path::new("/etc").join(name);
        mount(
            Some(&source),
            &target,
            None::<&str>,
            MsFlags::MS_BIND,
            None::<&str>,
        )
        .map_err(sys("bind name file"))?;
        let read_only = MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY | NOSUID_NODEV;
        mount(None::<&str>, &target, None::<&str>, read_only, None::<&str>)
            .map_err(sys("seal name file"))?;
    }
    Ok(())
}

fn sandbox_cgroup(pids: u32) -> Result<(), InitError> {
    fs::write("/sys/fs/cgroup/cgroup.subtree_control", "+pids +memory")
        .map_err(io("cgroup controllers"))?;
    fs::create_dir(plan::SANDBOX_CGROUP).map_err(io("sandbox cgroup"))?;
    fs::write(
        Path::new(plan::SANDBOX_CGROUP).join("pids.max"),
        pids.to_string(),
    )
    .map_err(io("pids limit"))?;
    // The guest's OOM killer acts inside the sandbox before the VM runs out of memory (SBX-C-003).
    let meminfo = fs::read_to_string("/proc/meminfo").map_err(io("memory size"))?;
    let limit = plan::mem_total(&meminfo)
        .and_then(plan::sandbox_memory_max)
        .ok_or(InitError::Config(
            "guest memory too small for the sandbox reserve",
        ))?;
    fs::write(
        Path::new(plan::SANDBOX_CGROUP).join("memory.max"),
        limit.to_string(),
    )
    .map_err(io("memory limit"))?;
    // Out-of-memory kills stay per process, so the runtime survives a tool that exceeds the limit (SBX-C-003).
    fs::write(
        Path::new(plan::SANDBOX_CGROUP).join("memory.oom.group"),
        "0",
    )
    .map_err(io("oom policy"))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, InitError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| {
            file.take(MAX_DOCUMENT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(io("read configuration"))?;
    Ok(bytes)
}
