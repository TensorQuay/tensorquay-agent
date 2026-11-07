//! Fixed guest layout and the pure derivations the two stages act on.

use crate::config::{GuestConfig, HostEntry, ImageConfig};

/// Workspace image, mounted read-write.
pub const WORKSPACE_DEVICE: &str = "/dev/vdb";
/// Per-conversation state image, mounted read-write.
pub const STATE_DEVICE: &str = "/dev/vdc";
/// Configuration device, read once and never mounted.
pub const CONFIG_DEVICE: &str = "/dev/vdd";
/// Image configuration written by the rootfs pipeline.
pub const IMAGE_CONFIG: &str = "/etc/tqa-vm/image.json";
/// Workspace mount point, the same as in Docker mode.
pub const WORKSPACE_DIR: &str = "/workspace";
/// Runtime state mount point, the same as in Docker mode.
pub const STATE_DIR: &str = "/run/tqa/state";
/// Token file, the same path as in Docker mode.
pub const TOKEN_FILE: &str = "/run/tqa/token";
/// The sandbox's cgroup; the init itself stays outside it.
pub const SANDBOX_CGROUP: &str = "/sys/fs/cgroup/sandbox";

/// The runtime's environment: the image's, then the host's (which wins), then the init-owned paths.
#[must_use]
pub fn runtime_env(image: &ImageConfig, guest: &GuestConfig) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = image
        .env
        .iter()
        .filter(|(key, _)| !guest.env.iter().any(|(other, _)| other == key))
        .cloned()
        .collect();
    env.extend(guest.env.iter().cloned());
    env.retain(|(key, _)| !crate::config::INIT_OWNED.contains(&key.as_str()));
    env.push(("TQA_RUNTIME_TOKEN_FILE".into(), TOKEN_FILE.into()));
    env.push(("TQA_STATE_DIR".into(), STATE_DIR.into()));
    env
}

/// Hosts file contents: loopback plus the static entries.
#[must_use]
pub fn hosts_file(hosts: &[HostEntry]) -> String {
    let mut text = String::from("127.0.0.1\tlocalhost\n::1\tlocalhost\n");
    text.extend(
        hosts
            .iter()
            .map(|host| format!("{}\t{}\n", host.address, host.name)),
    );
    text
}

/// Stage-two arguments: the numeric identity, then the program and its arguments.
#[must_use]
pub fn stage2_args(image: &ImageConfig) -> Vec<String> {
    let mut args = vec![
        "--stage2".into(),
        image.uid.to_string(),
        image.gid.to_string(),
        "--".into(),
    ];
    args.extend(image.argv.iter().cloned());
    args
}

/// Guest memory kept outside the sandbox's cgroup, for the guest kernel, the init and the page cache it must keep
/// working, so the guest's OOM killer acts inside the sandbox before the VM runs out and its host kills it (SBX-C-003).
/// Sized from the guest's baseline measured by the limits probe; see deploy/firecracker/README.md, "Memory".
pub const SANDBOX_MEMORY_RESERVE: u64 = 64 << 20;
/// The smallest sandbox cgroup limit that still leaves the runtime room to work.
const MIN_SANDBOX_MEMORY: u64 = 128 << 20;

/// `MemTotal` from `/proc/meminfo`, in bytes.
pub fn mem_total(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let mut fields = line.split_whitespace().skip(1);
    let value: u64 = fields.next()?.parse().ok()?;
    (fields.next()? == "kB").then(|| value.checked_mul(1024))?
}

/// The sandbox cgroup's `memory.max`: the guest's memory less the reserve, or `None` when too little would be left.
pub fn sandbox_memory_max(total: u64) -> Option<u64> {
    total
        .checked_sub(SANDBOX_MEMORY_RESERVE)
        .filter(|limit| *limit >= MIN_SANDBOX_MEMORY)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use serde_json::json;

    use super::{
        SANDBOX_MEMORY_RESERVE, hosts_file, mem_total, runtime_env, sandbox_memory_max, stage2_args,
    };
    use crate::config::{parse_guest, parse_image};

    #[test]
    fn sbx_f_016_runtime_environment_and_arguments() -> Result<(), Box<dyn Error>> {
        let image = parse_image(&serde_json::to_vec(&json!({
            "user": "1000:1000", "env": ["PATH=/bin", "HOME=/tmp", "TQA_STATE_DIR=/image"],
            "working_dir": "/workspace", "entrypoint": ["/usr/bin/tini", "--"], "cmd": ["/usr/local/bin/tqa-runtime"]
        }))?)?;
        let guest = parse_guest(&serde_json::to_vec(&json!({
            "version": 1, "token": "t", "env": ["HOME=/workspace/.tqa/home", "TQA_GATEWAY_URL=http://gw:8080"],
            "hosts": [{ "name": "gw", "address": "192.0.2.10" }], "pids": 64
        }))?)?;
        let env = runtime_env(&image, &guest);
        let get = |name: &str| {
            env.iter()
                .filter(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            get("HOME"),
            ["/workspace/.tqa/home"],
            "the host's value wins, once"
        );
        assert_eq!(get("PATH"), ["/bin"]);
        assert_eq!(
            get("TQA_STATE_DIR"),
            ["/run/tqa/state"],
            "the image cannot move init-owned paths"
        );
        assert_eq!(get("TQA_RUNTIME_TOKEN_FILE"), ["/run/tqa/token"]);
        assert!(
            env.iter().all(|(_, value)| value != "t"),
            "the token never enters the environment"
        );
        assert_eq!(
            hosts_file(&guest.hosts),
            "127.0.0.1\tlocalhost\n::1\tlocalhost\n192.0.2.10\tgw\n"
        );
        assert_eq!(
            stage2_args(&image),
            [
                "--stage2",
                "1000",
                "1000",
                "--",
                "/usr/bin/tini",
                "--",
                "/usr/local/bin/tqa-runtime"
            ]
        );
        Ok(())
    }

    #[test]
    fn sbx_c_003_the_sandbox_cgroup_keeps_a_reserve_for_the_guest() {
        let meminfo = "MemTotal:         503980 kB\nMemFree:          480000 kB\n";
        assert_eq!(mem_total(meminfo), Some(503_980 * 1024));
        for broken in [
            "",
            "MemFree: 1 kB\n",
            "MemTotal: x kB\n",
            "MemTotal: 5\n",
            "MemTotal: 5 MB\n",
        ] {
            assert_eq!(mem_total(broken), None, "{broken:?}");
        }
        let total = 512 << 20;
        assert_eq!(
            sandbox_memory_max(total),
            Some(total - SANDBOX_MEMORY_RESERVE)
        );
        assert_eq!(
            sandbox_memory_max((128 << 20) + SANDBOX_MEMORY_RESERVE),
            Some(128 << 20),
            "the smallest allowed"
        );
        assert_eq!(
            sandbox_memory_max((128 << 20) + SANDBOX_MEMORY_RESERVE - 1),
            None
        );
        assert_eq!(sandbox_memory_max(SANDBOX_MEMORY_RESERVE - 1), None);
    }
}
