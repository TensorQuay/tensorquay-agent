use std::{
    collections::HashMap,
    path::{Component, Path},
    time::SystemTime,
};

use bollard::models::{
    ContainerCreateBody, HostConfig, Mount, MountType, MountVolumeOptions, RestartPolicy,
    RestartPolicyNameEnum,
};

use crate::{BackendKind, EgressMode, SandboxError, SandboxSpec, WorkspaceSource};

/// Builds the immutable hardening posture without I/O or clock reads.
///
/// The returned transport body contains only the credential file's path, never its contents.
///
/// # Errors
/// Returns `InvalidSpec` before producing a configuration for invalid input.
pub fn container_config(
    spec: &SandboxSpec,
    network_name: &str,
    created_at: SystemTime,
    token_file: &Path,
) -> Result<ContainerCreateBody, SandboxError> {
    let mount = workspace_mount(&spec.workspace.source)?;
    spec.validate(BackendKind::Docker)?;
    validate_network_name(network_name)?;
    let token_mount = credential_mount(token_file)?;
    let state_mount = Mount {
        typ: Some(MountType::BIND),
        source: Some(
            spec.state
                .host_directory()
                .ok_or_else(|| {
                    SandboxError::invalid("state", "block images require the microVM backend")
                })?
                .to_str()
                .ok_or_else(|| SandboxError::invalid("state", "state path is not UTF-8"))?
                .into(),
        ),
        target: Some("/run/tqa/state".into()),
        read_only: Some(false),
        ..Default::default()
    };
    let mut mounts = vec![mount, token_mount, state_mount];
    for (path, target) in spec.skills.tiers() {
        mounts.push(Mount {
            typ: Some(MountType::BIND),
            source: Some(
                path.to_str()
                    .ok_or_else(|| SandboxError::invalid("skills", "skills path is not UTF-8"))?
                    .into(),
            ),
            target: Some(target.into()),
            read_only: Some(true),
            ..Default::default()
        });
    }
    let timestamp = created_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| {
            SandboxError::invalid("created_at", "creation time precedes the Unix epoch")
        })?;
    let labels = HashMap::from([
        ("tqa.conversation".into(), spec.conversation_id.to_string()),
        ("tqa.workspace".into(), spec.workspace.id.to_string()),
        ("tqa.image_digest".into(), spec.image.digest().to_owned()),
        ("tqa.created_at".into(), timestamp.as_secs().to_string()),
    ]);
    Ok(ContainerCreateBody {
        image: Some(spec.image.to_string()),
        user: Some("1000:1000".into()),
        working_dir: Some("/workspace".into()),
        attach_stdin: Some(false),
        open_stdin: Some(false),
        tty: Some(false),
        labels: Some(labels),
        env: Some(environment(spec)),
        stop_timeout: Some(10),
        host_config: Some(host_config(spec, network_name, mounts)),
        ..Default::default()
    })
}

/// A desktop sandbox's shared memory, for the browser's and the display's buffers (ADR-0039 section 10).
const DESKTOP_SHM_BYTES: i64 = 1 << 30;
/// The browser's profile and cache: its own 512 MiB tmpfs, the sandbox user's (RT-I-046's probe gives the same).
const BROWSER_TMPFS: (&str, &str) = (
    "/run/tqa-browser",
    "rw,nosuid,nodev,noexec,size=536870912,mode=0700,uid=1000,gid=1000",
);
/// The display's framebuffer as a file (Xvfb `-fbdir`), which the runtime reads to see whether the screen changed and
/// for screenshots: 8 MiB holds 1280x800 at 32 bits, the sandbox user's.
const DISPLAY_TMPFS: (&str, &str) = (
    "/run/tqa-display",
    "rw,nosuid,nodev,noexec,size=8388608,mode=0700,uid=1000,gid=1000",
);

fn host_config(spec: &SandboxSpec, network_name: &str, mounts: Vec<Mount>) -> HostConfig {
    let mut tmpfs = HashMap::from([(
        "/tmp".into(),
        "rw,nosuid,nodev,noexec,size=16777216,mode=1777".into(),
    )]);
    if spec.desktop {
        tmpfs.insert(BROWSER_TMPFS.0.into(), BROWSER_TMPFS.1.into());
        tmpfs.insert(DISPLAY_TMPFS.0.into(), DISPLAY_TMPFS.1.into());
    }
    HostConfig {
        readonly_rootfs: Some(true),
        privileged: Some(false),
        cap_drop: Some(vec!["ALL".into()]),
        security_opt: Some(vec!["no-new-privileges".into(), "seccomp=builtin".into()]),
        tmpfs: Some(tmpfs),
        shm_size: spec.desktop.then_some(DESKTOP_SHM_BYTES),
        mounts: Some(mounts),
        pids_limit: Some(i64::from(spec.pids)),
        memory: Some(spec.memory.signed()),
        memory_swap: Some(spec.memory.signed()),
        cpu_period: Some(1_000_000),
        cpu_quota: Some(i64::from(spec.cpu.millicores()) * 1000),
        network_mode: Some(network_name.into()),
        ipc_mode: Some("private".into()),
        publish_all_ports: Some(false),
        restart_policy: Some(RestartPolicy {
            name: Some(RestartPolicyNameEnum::NO),
            maximum_retry_count: Some(0),
        }),
        ..Default::default()
    }
}

fn workspace_mount(source: &WorkspaceSource) -> Result<Mount, SandboxError> {
    let (typ, source) = match source {
        WorkspaceSource::NamedVolume(name) => (MountType::VOLUME, name.clone()),
        WorkspaceSource::HostDirectory(path) => (
            MountType::BIND,
            path.to_str()
                .ok_or_else(|| SandboxError::invalid("workspace", "workspace path is not UTF-8"))?
                .to_owned(),
        ),
        WorkspaceSource::BlockImage(_) => {
            return Err(SandboxError::invalid(
                "workspace",
                "block images require the microVM backend",
            ));
        }
    };
    Ok(Mount {
        typ: Some(typ),
        source: Some(source),
        target: Some("/workspace".into()),
        read_only: Some(false),
        volume_options: (typ == MountType::VOLUME).then_some(MountVolumeOptions {
            no_copy: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// The loopback names and addresses no proxy variable may send through the egress proxy (SBX-U-018); the runtime's
/// shell tool lists the same (`tqa-tools` `shell.rs`, TOOL-U-060).
const LOOPBACK_BYPASS: &str = "localhost,127.0.0.1,::1";

/// The runtime environment; the Firecracker backend passes the same variables through its configuration device.
pub(crate) fn environment(spec: &SandboxSpec) -> Vec<String> {
    let mut env: Vec<_> = spec
        .env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    env.push(format!("TQA_GATEWAY_URL={}", spec.gateway_url));
    env.push("TQA_RUNTIME_TOKEN_FILE=/run/tqa/token".into());
    env.push("TQA_STATE_DIR=/run/tqa/state".into());
    if let (EgressMode::Proxy, Some(proxy)) = (spec.egress.mode, &spec.egress.proxy_url) {
        // The origin only (scheme, host, port), never a path: `Url` writes "/" for http, and Chromium refuses a proxy
        // with any path (net::ERR_NO_SUPPORTED_PROXIES) where curl and pip take either (SBX-U-016).
        let proxy = proxy.origin().ascii_serialization();
        env.push(format!("TQA_EGRESS_PROXY_URL={proxy}"));
        env.push(format!("HTTP_PROXY={proxy}"));
        env.push(format!("HTTPS_PROXY={proxy}"));
        // Loopback never goes through the proxy, in both spellings (curl reads the lower case first, wget only it): a task
        // reaches its own local servers (SBX-U-018, real-world use before 17.0: "Blocked network access to 127.0.0.1"),
        // the rest still goes through the proxy.
        let bypass = format!(
            "{},{LOOPBACK_BYPASS}",
            spec.gateway_url.host_str().unwrap_or_default()
        );
        env.push(format!("NO_PROXY={bypass}"));
        env.push(format!("no_proxy={bypass}"));
    }
    env
}

fn credential_mount(path: &Path) -> Result<Mount, SandboxError> {
    if !path.is_absolute()
        || path.file_name().is_none_or(|name| name != "token")
        || path.components().any(|part| part == Component::ParentDir)
    {
        return Err(SandboxError::invalid(
            "token_file",
            "expected an absolute credential file path without traversal",
        ));
    }
    let source = path
        .to_str()
        .ok_or_else(|| SandboxError::invalid("token_file", "credential path must be UTF-8"))?;
    Ok(Mount {
        typ: Some(MountType::BIND),
        source: Some(source.into()),
        target: Some("/run/tqa/token".into()),
        read_only: Some(true),
        ..Default::default()
    })
}

pub(super) fn validate_network_name(name: &str) -> Result<(), SandboxError> {
    if name.is_empty()
        || name.len() > 63
        || ["host", "none", "bridge", "default"].contains(&name)
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(SandboxError::invalid(
            "network",
            "expected a dedicated network name of 1 through 63 characters",
        ));
    }
    Ok(())
}
