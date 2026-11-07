//! Shared by the Firecracker KVM test binaries: the host, its templates, the test network with a gateway stub, and the
//! connection retry for the logind window. Requirements, checked before any VM starts: `TQA_VMM_IMAGE` (a built
//! images/vmm image ID), `TQA_FC_KERNEL_DIR` (vmlinux and its manifest), `TQA_FC_ROOTFS_DIR` (a persistent directory for
//! built rootfs images), optional `TQA_FC_BASE_IMAGE` (default `tqa-runtime:base`) and `TQA_FC_STORAGE` (default
//! `/var/tmp`, which must support ACLs). Run through `tools/rootless-test.sh` on a rootless engine, one run at a time.
#![allow(
    dead_code,
    reason = "each test binary uses a different part of the shared harness"
)]

use std::{
    collections::BTreeMap,
    error::Error,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{
    ByteSize, CpuLimit, EgressMode, EgressPolicy, ImageRef, ReadinessSource, SandboxBackend,
    SandboxError, SandboxHandle, SandboxSpec, StateMount, StateSource, WorkspaceMount,
    WorkspaceSource,
    firecracker::{FirecrackerBackend, FirecrackerBackendConfig},
};
use tqa_types::ConversationId;

pub type TestResult = Result<(), Box<dyn Error>>;

/// An internal network with a documentation-range subnet, so the stub's addresses can be pinned.
pub const NETWORK: &str = "tqa-fc-net";
const SUBNET: &str = "198.51.100.0/24";
pub const GATEWAY: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 10);
pub const BYSTANDER: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 20);
/// The gateway port a guest may reach, and one on the same address it must not.
pub const LINK_PORT: u16 = 8080;
pub const CLOSED_PORT: u16 = 9999;
const STUB: &str = "tqa-fc-gateway-stub";
const STUB_BYSTANDER: &str = "tqa-fc-bystander";
const PROBE_TAG: &str = "tqa-fc-probe:test";
const DERIVED_PROBE_TAG: &str = "tqa-fc-probe:derived";
const SECRET_PROBE_TAG: &str = "tqa-fc-probe:secret-env";

pub fn env(name: &str) -> Result<String, Box<dyn Error>> {
    std::env::var(name)
        .map_err(|_| format!("{name} is required for the Firecracker KVM suites").into())
}

pub fn run(command: &mut Command) -> Result<String, Box<dyn Error>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn base_image() -> String {
    std::env::var("TQA_FC_BASE_IMAGE").unwrap_or_else(|_| "tqa-runtime:base".into())
}

/// A template derived from the base image (SBX-C-014): the Workstation image unless `TQA_FC_DERIVED_IMAGE` names another.
fn derived_image() -> String {
    std::env::var("TQA_FC_DERIVED_IMAGE").unwrap_or_else(|_| "tqa-runtime:workstation".into())
}

/// Builds the rootfs for `image` once per (image, VM host image): the rootfs carries the guest init from the VM host.
fn rootfs_for(image: &str, rootfs_dir: &Path) -> Result<(), Box<dyn Error>> {
    let vmm = env("TQA_VMM_IMAGE")?;
    let target = rootfs_dir.join(image.trim_start_matches("sha256:"));
    let manifest = std::fs::read_to_string(target.join("manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    // Why a rootfs is (re)built, so an unexpected rebuild shows in the suite's output.
    let stale = match &manifest {
        None => Some("no manifest"),
        Some(manifest) if manifest["builder"]["image_id"] != vmm.as_str() => {
            Some("another VM host image")
        }
        Some(manifest) if manifest["schema"] != 2 => Some("an older schema"),
        Some(_) => None,
    };
    if let Some(reason) = stale {
        std::io::Write::write_fmt(
            &mut std::io::stderr(),
            format_args!("building the rootfs for {image}: {reason}\n"),
        )?;
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../images/rootfs/build_rootfs.py");
        run(Command::new("python3")
            .arg(script)
            .args(["--image", image, "--vmm-image", &vmm, "--out"])
            .arg(rootfs_dir))?;
    }
    Ok(())
}

/// Which image the guest runs.
pub enum Template {
    /// The in-guest probe (tests/firecracker/probe) instead of the runtime.
    Probe,
    /// The real runtime image, which dials the gateway.
    Runtime,
    /// The probe on a template derived from the base image (SBX-C-014).
    DerivedProbe,
    /// The probe with a secret baked into the image environment (SBX-C-014: the image policy refuses it).
    SecretEnvProbe,
}

/// The tag of an image built from `inputs`, reused while they don't change. `docker build` isn't reproducible here:
/// two builds of the same context give two IDs. The rootfs cache is keyed by image ID, so without this every run
/// rebuilt every probe rootfs, and each rebuild was another chance for the builder's intermittent failures. The image
/// is tagged `<tag>-<hash of the inputs>` and built only when that tag is missing. (A tag, because `BuildKit` reads
/// `FROM sha256:...` as a repository to pull.)
fn keyed_image(
    tag: &str,
    inputs: &[&[u8]],
    build: impl FnOnce(&str) -> Result<(), Box<dyn Error>>,
) -> Result<String, Box<dyn Error>> {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for input in inputs {
        digest.update((input.len() as u64).to_be_bytes());
        digest.update(input);
    }
    let hash = digest.finalize();
    let key = hash[..8].iter().fold(String::new(), |mut key, byte| {
        let _ = std::fmt::Write::write_fmt(&mut key, format_args!("{byte:02x}"));
        key
    });
    let keyed = format!("{tag}-{key}");
    if !image_id(&keyed).is_ok_and(|id| !id.is_empty()) {
        build(&keyed)?;
        // A new key makes this variant's older keys stale: removed, so at most one image per variant stays
        // (SBX-F-034). One still in use by a container stays until the next build.
        let listed = run(Command::new("docker").args([
            "image",
            "ls",
            "--format",
            "{{.Repository}}:{{.Tag}}",
        ]))?;
        let listed: Vec<&str> = listed.lines().collect();
        for stale in stale_keys(tag, &keyed, &listed) {
            let _ = Command::new("docker")
                .args(["image", "rm", &stale])
                .output();
        }
    }
    Ok(keyed)
}

/// This variant's keyed images in `listed` other than `keyed`: `<tag>-<16 hex digits>`, never another variant's (a
/// longer tag that starts the same) nor the bare tag.
fn stale_keys(tag: &str, keyed: &str, listed: &[&str]) -> Vec<String> {
    let prefix = format!("{tag}-");
    listed
        .iter()
        .filter(|image| **image != keyed)
        .filter(|image| {
            image.strip_prefix(&prefix).is_some_and(|key| {
                key.len() == 16 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        })
        .map(|image| (*image).to_owned())
        .collect()
}

// TEST: SBX-F-034
/// A variant's older keys are the stale ones: its own `<tag>-<key>` images but the current one, never another variant's
/// (`derived` is not `test`), nor a bare tag.
#[test]
fn sbx_f_034_a_new_key_makes_only_its_own_variant_s_older_keys_stale() {
    let listed = [
        "tqa-fc-probe:test-00112233aabbccdd",
        "tqa-fc-probe:test-0011223344556677",
        "tqa-fc-probe:test-ffeeddccbbaa9988",
        "tqa-fc-probe:testing-0011223344556677",
        "tqa-fc-probe:derived-0011223344556677",
        "tqa-fc-probe:test",
        "tqa-runtime:base",
    ];
    assert_eq!(
        stale_keys(PROBE_TAG, "tqa-fc-probe:test-00112233aabbccdd", &listed),
        [
            "tqa-fc-probe:test-0011223344556677",
            "tqa-fc-probe:test-ffeeddccbbaa9988"
        ]
    );
    assert!(
        stale_keys(
            SECRET_PROBE_TAG,
            "tqa-fc-probe:secret-env-0011223344556677",
            &listed
        )
        .is_empty()
    );
}

fn image_id(image: &str) -> Result<String, Box<dyn Error>> {
    run(Command::new("docker").args(["image", "inspect", "-f", "{{.Id}}", image]))
}

/// The keyed tag of a probe variant (see [`keyed_image`]): the probe directory on the base image.
fn probe_tag(template: &Template) -> Result<String, Box<dyn Error>> {
    let (base, tag) = if matches!(template, Template::DerivedProbe) {
        (derived_image(), DERIVED_PROBE_TAG)
    } else {
        (base_image(), PROBE_TAG)
    };
    let base_id = image_id(&base)?;
    let context = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/firecracker/probe");
    let (dockerfile, probe) = (
        std::fs::read(context.join("Dockerfile"))?,
        std::fs::read(context.join("probe.py"))?,
    );
    keyed_image(tag, &[base_id.as_bytes(), &dockerfile, &probe], |keyed| {
        let arg = format!("BASE={base}");
        run(Command::new("docker")
            .args(["build", "-q", "--build-arg", &arg, "-t", keyed])
            .arg(&context))
        .map(|_| ())
    })
}

fn template_id(template: &Template, rootfs_dir: &Path) -> Result<String, Box<dyn Error>> {
    let id = match template {
        Template::Probe | Template::DerivedProbe => image_id(&probe_tag(template)?)?,
        Template::SecretEnvProbe => {
            let probe = probe_tag(&Template::Probe)?;
            let dockerfile = format!("FROM {probe}\nENV API_TOKEN=synthetic-image-secret\n");
            let keyed = keyed_image(SECRET_PROBE_TAG, &[dockerfile.as_bytes()], |keyed| {
                let mut build = Command::new("docker")
                    .args(["build", "-q", "-t", keyed, "-"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .spawn()?;
                {
                    let mut input = build.stdin.take().ok_or("stdin")?;
                    std::io::Write::write_all(&mut input, dockerfile.as_bytes())?;
                }
                if build.wait()?.success() {
                    Ok(())
                } else {
                    Err("the secret-env probe image did not build".into())
                }
            })?;
            image_id(&keyed)?
        }
        Template::Runtime => image_id(&base_image())?,
    };
    rootfs_for(&id, rootfs_dir)?;
    Ok(id)
}

/// The internal test network and, on it, the gateway stub and a bystander, all reused across runs.
pub fn network() -> Result<(), Box<dyn Error>> {
    if run(Command::new("docker").args(["network", "inspect", "-f", "{{.Internal}}", NETWORK]))
        .is_err()
    {
        run(Command::new("docker").args([
            "network",
            "create",
            "--internal",
            "--subnet",
            SUBNET,
            NETWORK,
        ]))?;
    }
    let stub = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/firecracker/stub/stub.py");
    for (name, address) in [(STUB, GATEWAY), (STUB_BYSTANDER, BYSTANDER)] {
        let running =
            run(Command::new("docker").args(["inspect", "-f", "{{.State.Running}}", name]));
        if running.as_deref().ok() == Some("true") {
            continue;
        }
        let _ = Command::new("docker").args(["rm", "-f", name]).output();
        run(Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                name,
                "--network",
                NETWORK,
                "--ip",
                &address.to_string(),
            ])
            .args([
                "--user",
                "1000:1000",
                "--read-only",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges",
            ])
            .args([
                "-v",
                &format!("{}:/stub.py:ro", stub.display()),
                "--entrypoint",
                "/usr/bin/python3",
                &base_image(),
            ])
            .args(["/stub.py", &LINK_PORT.to_string(), &CLOSED_PORT.to_string()]))?;
    }
    Ok(())
}

/// The stub's connection lines since `since` (RFC 3339), from `peer` when given.
pub fn stub_lines(since: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let output = Command::new("docker")
        .args(["logs", "--since", since, STUB])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("stub port="))
        .map(str::to_owned)
        .collect())
}

/// Never ready: a probe does not dial a gateway, so waiting ends only when the VM exits.
pub struct Never;

#[async_trait]
impl ReadinessSource for Never {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        false
    }
}

/// Ready once the stub has seen a request on the link port from `peer` (the VM host's address) since `since`.
pub struct Dialled {
    pub since: String,
    pub peer: std::sync::Mutex<Option<String>>,
}

#[async_trait]
impl ReadinessSource for Dialled {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        let Ok(Some(peer)) = self.peer.lock().map(|peer| peer.clone()) else {
            return false;
        };
        stub_lines(&self.since).is_ok_and(|lines| {
            lines.iter().any(|line| {
                line.contains(&format!("port={LINK_PORT} "))
                    && line.contains(&format!("peer={peer} "))
            })
        })
    }
}

pub struct Host {
    pub backend: FirecrackerBackend,
    pub storage: tempfile::TempDir,
    pub image: ImageRef,
    /// The VM host's memory on top of the guest's, as this host's configuration sets it (the default unless a test
    /// changes it), so a test checks the limit the backend was given rather than a remembered number.
    pub vmm_overhead: u64,
}

/// Connects for a KVM test, retrying once after 35 seconds when the only missing requirement is `/dev/kvm` access.
/// logind rewrites that device's ACL on every seat-session change, and the host's timer restores the VMM grant within
/// 30 seconds (deploy/firecracker/README.md). Real time is used because the test observes real host state; the bound
/// is one retry, and any other refusal, or a second one, fails the test with its reason.
pub async fn connect(config: FirecrackerBackendConfig) -> Result<FirecrackerBackend, SandboxError> {
    // The backend's warnings (which step timed out, for one) on stderr, where --no-capture shows them.
    let _ = tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .finish(),
    );
    match FirecrackerBackend::connect(config.clone()).await {
        Err(SandboxError::Preflight(reason)) if reason.starts_with("/dev/kvm does not open") => {
            tokio::time::sleep(Duration::from_secs(35)).await;
            FirecrackerBackend::connect(config).await
        }
        other => other,
    }
}

pub async fn host(template: &Template) -> Result<Host, Box<dyn Error>> {
    let rootfs_dir = PathBuf::from(env("TQA_FC_ROOTFS_DIR")?);
    std::fs::create_dir_all(&rootfs_dir)?;
    let image = ImageRef::local_id(template_id(template, &rootfs_dir)?)?;
    network()?;
    let storage = tempfile::tempdir_in(
        std::env::var("TQA_FC_STORAGE").unwrap_or_else(|_| "/var/tmp".into()),
    )?;
    let mut config = FirecrackerBackendConfig::new(
        storage.path().into(),
        ImageRef::local_id(env("TQA_VMM_IMAGE")?)?,
        NETWORK.into(),
        vec![LINK_PORT],
    )?;
    config.gateway_address = GATEWAY;
    config.kernel_dir = env("TQA_FC_KERNEL_DIR")?.into();
    config.rootfs_dir = rootfs_dir;
    let vmm_overhead = config.vmm_overhead.bytes();
    Ok(Host {
        backend: connect(config).await?,
        storage,
        image,
        vmm_overhead,
    })
}

/// A VM host image as an upgrade leaves it: the current one with a label added, so a new ID with the same contents.
pub fn upgraded_vmm() -> Result<ImageRef, Box<dyn Error>> {
    const CURRENT: &str = "tqa-fc-vmm:current";
    const TAG: &str = "tqa-fc-vmm:upgraded";
    // A build resolves a bare image ID as a registry name, so the current image is named locally first.
    run(Command::new("docker").args(["tag", &env("TQA_VMM_IMAGE")?, CURRENT]))?;
    let dockerfile = format!("FROM {CURRENT}\nLABEL org.tensorquay.test=upgraded-vmm\n");
    let mut build = Command::new("docker")
        .args(["build", "-q", "-t", TAG, "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    {
        let mut input = build.stdin.take().ok_or("stdin")?;
        std::io::Write::write_all(&mut input, dockerfile.as_bytes())?;
    }
    if !build.wait()?.success() {
        return Err("the upgraded VM host image did not build".into());
    }
    let id = run(Command::new("docker").args(["image", "inspect", "-f", "{{.Id}}", TAG]))?;
    Ok(ImageRef::local_id(id)?)
}

/// A second backend on `host`'s storage and rootfs directory whose VM host image is `vmm`: as after a VM host image
/// upgrade, when the rootfs builds on disk came from the previous one.
pub async fn backend_with_vmm(
    host: &Host,
    vmm: ImageRef,
) -> Result<FirecrackerBackend, Box<dyn Error>> {
    let mut config = FirecrackerBackendConfig::new(
        host.storage.path().into(),
        vmm,
        NETWORK.into(),
        vec![LINK_PORT],
    )?;
    config.gateway_address = GATEWAY;
    config.kernel_dir = env("TQA_FC_KERNEL_DIR")?.into();
    config.rootfs_dir = PathBuf::from(env("TQA_FC_ROOTFS_DIR")?);
    Ok(connect(config).await?)
}

impl Host {
    pub fn spec(&self, env: &[(&str, &str)]) -> Result<SandboxSpec, Box<dyn Error>> {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let root = self.storage.path();
        std::fs::create_dir_all(root.join("workspaces"))?;
        std::fs::create_dir_all(root.join("state"))?;
        Ok(SandboxSpec {
            conversation_id: format!("conversation_{stamp:x}").parse()?,
            workspace: WorkspaceMount {
                id: "workspace_probe".parse()?,
                source: WorkspaceSource::BlockImage(
                    root.join(format!("workspaces/{stamp:x}.ext4")),
                ),
            },
            state: StateMount {
                source: StateSource::BlockImage(root.join(format!("state/{stamp:x}.ext4"))),
            },
            image: self.image.clone(),
            cpu: CpuLimit::new(1000)?,
            memory: ByteSize::new(512 << 20)?,
            disk: Some(ByteSize::new(256 << 20)?),
            pids: 256,
            egress: EgressPolicy {
                mode: EgressMode::Internal,
                allow: vec![],
                proxy_url: None,
            },
            gateway_url: format!("http://tqa-gateway:{LINK_PORT}/").parse()?,
            runtime_token: SecretString::from(format!("probe-token-{stamp:x}")),
            // The image's own environment first, as an operator's `[sandbox.env]` sets it (the image policy requires
            // every key the image sets); the suite's values on top.
            env: image_env(&self.image)?
                .into_iter()
                .chain(
                    env.iter()
                        .map(|&(key, value)| (key.to_owned(), value.to_owned())),
                )
                .collect::<BTreeMap<_, _>>(),
            skills: tqa_sandbox::SkillsMounts::default(),
            desktop: false,
        })
    }

    /// Boots the probe with `suite`, waits for it to finish, and returns its code and its console report.
    pub async fn probe(&self, suite: &str) -> Result<(i32, String), Box<dyn Error>> {
        self.probe_with(suite, &[], |_| ()).await
    }

    /// [`Host::probe`] with extra environment for the suite and a change to the spec (a reused workspace image, a
    /// smaller disk, more vCPUs).
    pub async fn probe_with(
        &self,
        suite: &str,
        extra: &[(&str, &str)],
        adjust: impl FnOnce(&mut SandboxSpec),
    ) -> Result<(i32, String), Box<dyn Error>> {
        let (gateway, bystander) = (GATEWAY.to_string(), BYSTANDER.to_string());
        let mut env = vec![
            ("PROBE_CHECK", suite),
            ("PROBE_GATEWAY", gateway.as_str()),
            ("PROBE_BYSTANDER", bystander.as_str()),
        ];
        env.extend_from_slice(extra);
        let mut spec = self.spec(&env)?;
        adjust(&mut spec);
        let handle = self.backend.create(spec, CancellationToken::new()).await?;
        let waited = self
            .backend
            .wait_ready(
                &handle,
                &Never,
                Duration::from_mins(3),
                CancellationToken::new(),
            )
            .await;
        let report = console(&handle)?;
        self.backend.destroy(&handle).await?;
        match waited {
            Err(SandboxError::RuntimeExited { code }) => Ok((code, report)),
            other => Err(format!("the probe did not finish: {other:?}; console: {report}").into()),
        }
    }
}

/// The template image's environment, as `KEY=value` pairs from the engine.
fn image_env(image: &ImageRef) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let text = run(Command::new("docker").args([
        "image",
        "inspect",
        "-f",
        "{{json .Config.Env}}",
        image.digest(),
    ]))?;
    let entries: Vec<String> = serde_json::from_str(&text)?;
    Ok(entries
        .iter()
        .filter_map(|entry| entry.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect())
}

pub fn console(handle: &SandboxHandle) -> Result<String, Box<dyn Error>> {
    let output = Command::new("docker").args(["logs", &handle.id]).output()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    Ok(text
        .lines()
        .filter(|line| line.contains("tqa-probe:") || line.contains("tqa-vm-init:"))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Container uid `inner` of this user's rootless engine as this process sees it: unchanged inside the engine's user
/// namespace (tools/rootless-test.sh), the subordinate range start + inner - 1 in the host view.
pub fn engine_uid(inner: u32) -> Result<u32, Box<dyn Error>> {
    let identity = std::fs::read_to_string("/proc/self/uid_map")?
        .split_whitespace()
        .collect::<Vec<_>>()
        == ["0", "0", "4294967295"];
    if !identity {
        return Ok(inner);
    }
    let user = run(Command::new("id").arg("-un"))?;
    let start: u32 = std::fs::read_to_string("/etc/subuid")?
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{user}:"))
                .and_then(|rest| rest.split(':').next().map(str::to_owned))
        })
        .ok_or("no subordinate uid range")?
        .parse()?;
    Ok(start + inner - 1)
}

/// The last lines of the VM host's log (the guest console), for failure messages.
pub fn full_console(handle: &SandboxHandle) -> Result<String, Box<dyn Error>> {
    let output = Command::new("docker")
        .args(["logs", "--tail", "25", &handle.id])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr))
}
