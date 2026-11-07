use std::{
    collections::HashMap,
    error::Error,
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use bollard::models::ContainerCreateBody;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::FirecrackerBackend;
use super::image_tests::image_helpers;
use crate::{
    ReadinessSource, SandboxBackend, SandboxError, SandboxSpec, StateMount, StateSource,
    WorkspaceSource,
    firecracker::{
        FirecrackerBackendConfig,
        engine::{Container, Created, Engine, NetworkState, Usage},
        manifest::hex,
        test_support::{TOKEN, config, spec},
    },
};

/// How a busy engine's scripted timeout behaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Busy {
    /// It returns at once.
    #[default]
    Quick,
    /// It takes its whole bound first, as a busy engine's call does.
    SpendsBound,
    /// As `SpendsBound`, and the create still lands: the engine finished it after its caller gave up (SBX-F-033).
    LandsLate,
    /// As `SpendsBound`, and the create stays in flight, holding its name, for two more look-ups (SBX-F-035).
    InFlight,
}

/// An in-memory engine: probes succeed, `mkfs` helpers create the image file, containers can be made to fail.
#[derive(Default)]
pub(super) struct FakeEngine {
    pub(super) containers: Mutex<HashMap<String, Container>>,
    pub(super) helpers: Mutex<Vec<ContainerCreateBody>>,
    pub(super) fail_start: bool,
    /// The engine maps the storage owner to another uid, so the helper's file never appears as ours.
    pub(super) foreign_owner: bool,
    /// Scripted outcomes for the next inspections, in order: `true` times out (a busy engine).
    pub(super) inspect_timeouts: Mutex<std::collections::VecDeque<bool>>,
    /// Scripted outcomes for the next creates, in order: `true` times out (a busy engine, SBX-F-028).
    pub(super) create_timeouts: Mutex<std::collections::VecDeque<bool>>,
    /// How many creates were attempted.
    pub(super) create_attempts: std::sync::atomic::AtomicUsize,
    /// Scripted outcomes for the next image helpers, in order: `true` times out after writing part of the image, as a
    /// helper the engine removed mid-mkfs leaves it (SBX-F-030).
    pub(super) mkfs_timeouts: Mutex<std::collections::VecDeque<bool>>,
    /// How many image helpers ran.
    pub(super) mkfs_attempts: std::sync::atomic::AtomicUsize,
    /// Each create's and helper's bound, in order ("create" or "run", bound), SBX-F-031.
    pub(super) bounds: Mutex<Vec<(&'static str, Duration)>>,
    /// How a scripted create or helper timeout behaves (tests on paused time).
    pub(super) busy: Busy,
    /// How long this engine takes to create a container or run a helper (a loaded host: 44 s, SBX-F-032); a call
    /// bounded below it times out after its bound (tests on paused time).
    pub(super) create_latency: Mutex<Option<Duration>>,
    /// The image helper exits non-zero without writing anything (SBX-F-029).
    pub(super) fail_mkfs: bool,
    /// Each removal's container name, in order (SBX-F-033).
    pub(super) removed: Mutex<Vec<String>>,
    /// Creates still in flight in the engine, holding their names: the look-ups left before each lands (SBX-F-035).
    pub(super) in_flight: Mutex<HashMap<String, (u32, Container)>>,
}

#[async_trait]
impl Engine for FakeEngine {
    async fn image_present(&self, _: &str) -> Result<bool, SandboxError> {
        Ok(true)
    }
    async fn network(&self, _: &str) -> Result<NetworkState, SandboxError> {
        Ok(NetworkState::Internal)
    }
    async fn create_and_start(
        &self,
        name: &str,
        body: ContainerCreateBody,
        bound: Duration,
    ) -> Result<Created, SandboxError> {
        self.create_attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.latency(bound).await?;
        if self.name_held(name)? {
            return Ok(Created::NameHeld);
        }
        let times_out = self
            .create_timeouts
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .pop_front()
            .unwrap_or(false);
        self.bounded("create", bound, times_out).await?;
        if self.fail_start && !times_out {
            return Err(SandboxError::Backend("start failed".into()));
        }
        let container = Container {
            name: name.into(),
            labels: body.labels.unwrap_or_default(),
            running: true,
            exit_code: None,
            address: Some("192.0.2.20".parse().map_err(|_| SandboxError::NotFound)?),
        };
        if times_out && self.busy == Busy::InFlight {
            self.hold(name, container)?;
        } else if !times_out || self.busy == Busy::LandsLate {
            self.containers
                .lock()
                .map_err(|_| SandboxError::NotFound)?
                .insert(name.into(), container);
        }
        if times_out {
            return Err(SandboxError::Timeout);
        }
        Ok(Created::Started)
    }
    async fn inspect(&self, name: &str) -> Result<Container, SandboxError> {
        let times_out = self
            .inspect_timeouts
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .pop_front()
            .unwrap_or(false);
        if times_out {
            return Err(SandboxError::Timeout);
        }
        self.land(name)?;
        self.containers
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .get(name)
            .cloned()
            .ok_or(SandboxError::NotFound)
    }
    async fn remove(&self, name: &str) -> Result<(), SandboxError> {
        self.removed
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .push(name.into());
        self.containers
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .remove(name);
        Ok(())
    }
    async fn list(&self, instance: &str) -> Result<Vec<Container>, SandboxError> {
        let containers = self.containers.lock().map_err(|_| SandboxError::NotFound)?;
        Ok(containers
            .values()
            .filter(|c| c.labels.get("tqa.instance").map(String::as_str) == Some(instance))
            .cloned()
            .collect())
    }
    async fn helpers(&self, instance: &str) -> Result<Vec<String>, SandboxError> {
        let containers = self.containers.lock().map_err(|_| SandboxError::NotFound)?;
        Ok(containers
            .values()
            .filter(|c| {
                c.labels.get("tqa.instance").map(String::as_str) == Some(instance)
                    && c.labels.get("tqa.role").map(String::as_str) == Some("helper")
            })
            .map(|c| c.name.clone())
            .collect())
    }
    async fn usage(&self, _: &str) -> Result<Usage, SandboxError> {
        Ok(Usage {
            cpu_nanos: 5_000_000,
            memory_bytes: 1 << 20,
            pids: 3,
        })
    }
    async fn run(&self, body: ContainerCreateBody, bound: Duration) -> Result<i64, SandboxError> {
        self.latency(bound).await?;
        let cmd = body.cmd.clone().unwrap_or_default();
        if cmd.first().map(String::as_str) == Some("/usr/bin/touch") && !self.foreign_owner {
            let mount = body
                .host_config
                .as_ref()
                .and_then(|h| h.mounts.as_ref())
                .and_then(|m| m.first().cloned());
            let directory = mount.and_then(|m| m.source).ok_or(SandboxError::NotFound)?;
            let file = cmd
                .get(1)
                .and_then(|arg| arg.strip_prefix("/img/"))
                .ok_or(SandboxError::NotFound)?;
            fs::write(Path::new(&directory).join(file), b"").map_err(|_| SandboxError::NotFound)?;
        }
        // The image helper, told from the probes (also `/bin/sh -c`) by its script running mkfs.
        if is_image_helper(&cmd) {
            if self.fail_mkfs {
                self.helpers
                    .lock()
                    .map_err(|_| SandboxError::NotFound)?
                    .push(body);
                return Ok(1);
            }
            // The image helper: (file, size) pairs after the script and its $0, each file under a mount's target.
            let mounts = body
                .host_config
                .as_ref()
                .and_then(|h| h.mounts.clone())
                .unwrap_or_default();
            self.mkfs_attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let times_out = self
                .mkfs_timeouts
                .lock()
                .map_err(|_| SandboxError::NotFound)?
                .pop_front()
                .unwrap_or(false);
            self.bounded("run", bound, times_out).await?;
            for (index, pair) in cmd.get(4..).unwrap_or_default().chunks(2).enumerate() {
                let path = Path::new(&pair[0]);
                let target = path
                    .parent()
                    .and_then(Path::to_str)
                    .ok_or(SandboxError::NotFound)?;
                let source = mounts
                    .iter()
                    .find(|m| m.target.as_deref() == Some(target))
                    .and_then(|m| m.source.clone())
                    .ok_or(SandboxError::NotFound)?;
                let file = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .ok_or(SandboxError::NotFound)?;
                // A helper removed mid-run leaves its first image's file and the next one's `.tmp` half written.
                let (name, content) = match (times_out, index) {
                    (false, _) => (file.to_owned(), &b"ext4"[..]),
                    (true, 0) => (file.to_owned(), &b"part"[..]),
                    (true, _) => (format!("{file}.tmp"), &b"part"[..]),
                };
                fs::write(Path::new(&source).join(name), content)
                    .map_err(|_| SandboxError::NotFound)?;
            }
            if times_out {
                return Err(SandboxError::Timeout);
            }
        }
        self.helpers
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .push(body);
        Ok(0)
    }
}

struct Ready;

#[async_trait]
impl ReadinessSource for Ready {
    async fn is_ready(&self, _: &tqa_types::ConversationId) -> bool {
        true
    }
}

/// A storage root with a verified kernel and a rootfs for the test spec's image, plus that spec's images.
pub(super) fn host(root: &Path) -> Result<(FirecrackerBackendConfig, SandboxSpec), Box<dyn Error>> {
    host_with(root, (&[], &[]))
}

/// The image policy recorded in the rootfs manifest: environment key names, then declared volumes.
type Policy<'a> = (&'a [&'a str], &'a [&'a str]);

fn host_with(
    root: &Path,
    policy: Policy<'_>,
) -> Result<(FirecrackerBackendConfig, SandboxSpec), Box<dyn Error>> {
    let zero = "0".repeat(64);
    fs::create_dir_all(root.join("kernel"))?;
    fs::write(root.join("kernel/vmlinux"), b"kernel")?;
    let kernel = json!({ "schema": 1, "release": "6.18.54", "licence": "GPL-2.0-only", "source_sha256": zero,
        "base_config": "b", "base_config_sha256": zero, "fragment_sha256": zero, "config_sha256": zero,
        "vmlinux_sha256": hex(&Sha256::digest(b"kernel")) });
    fs::write(
        root.join("kernel/manifest.json"),
        serde_json::to_vec(&kernel)?,
    )?;
    let mut spec = spec()?;
    let image = root
        .join("images")
        .join(spec.image.digest().trim_start_matches("sha256:"));
    fs::create_dir_all(&image)?;
    fs::write(image.join("rootfs.ext4"), b"ext4")?;
    let revision = tqa_types::PROTOCOL_REVISION;
    let rootfs = json!({ "schema": 2, "image": { "env_keys": policy.0, "volumes": policy.1 },
        "source_image": { "id": spec.image.digest(), "repo_digests": [], "protocol_revision": revision },
        "rootfs": { "file": "rootfs.ext4", "sha256": hex(&Sha256::digest(b"ext4")), "bytes": 4, "tree_sha256": zero },
        "init": { "path": "/usr/sbin/tqa-vm-init", "sha256": zero }, "runtime": { "path": "/r", "sha256": zero },
        // Built by the configured VM host image, as a real build is.
        "builder": { "image_id": format!("sha256:{}", "a".repeat(64)), "e2fsprogs": "1.47.2" } });
    fs::write(image.join("manifest.json"), serde_json::to_vec(&rootfs)?)?;
    fs::create_dir_all(root.join("workspaces"))?;
    fs::create_dir_all(root.join("state"))?;
    spec.workspace.source = WorkspaceSource::BlockImage(root.join("workspaces/workspace_1.ext4"));
    spec.state = StateMount {
        source: StateSource::BlockImage(root.join("state/conversation_1.ext4")),
    };
    let mut config = config()?;
    config.storage_root = root.to_path_buf();
    config.kernel_dir = root.join("kernel");
    config.rootfs_dir = root.join("images");
    config.vmm_file_uid = 4242;
    Ok((config, spec))
}

pub(super) async fn backend(
    root: &Path,
    engine: Arc<FakeEngine>,
) -> Result<(FirecrackerBackend, SandboxSpec), Box<dyn Error>> {
    let (config, spec) = host(root)?;
    Ok((FirecrackerBackend::with_engine(config, engine).await?, spec))
}

#[tokio::test]
async fn sbx_f_017_create_provisions_the_vm_and_destroy_removes_everything()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let handle = backend
        .create(spec.clone(), CancellationToken::new())
        .await?;
    let directory = root.path().join("sandboxes").join(&handle.id);
    for file in ["config.img", "exit.img", "vm.json", "rules.nft"] {
        assert!(directory.join(file).exists(), "{file}");
    }
    assert!(fs::read_to_string(directory.join("config.img"))?.contains(TOKEN));
    assert!(!fs::read_to_string(directory.join("vm.json"))?.contains(TOKEN));
    // Both images were created by helpers running as the storage root's owner, then shared with the VMM uid only.
    assert_eq!(
        engine.helpers.lock().map_err(|_| "lock")?.len(),
        2 + 1 + 1,
        "two probes, the storage-owner check, one image helper making both images (SBX-F-029)"
    );
    // The runtime (uid 1000 in the guest) makes its state directory private, which only the owner of the mounted
    // filesystem's root may do; both new images are created with that root owner.
    let helpers = engine.helpers.lock().map_err(|_| "lock")?.clone();
    let mkfs = image_helpers(&helpers);
    assert_eq!(mkfs.len(), 1);
    assert!(mkfs[0][2].contains("root_owner=1000:1000"));
    // No root reserve: the guest (uid 1000) could never use it (SBX-F-025).
    assert!(mkfs[0][2].contains("-m 0"));
    for image in ["workspaces/workspace_1.ext4", "state/conversation_1.ext4"] {
        let acl = std::process::Command::new("getfacl")
            .args(["-n", "--omit-header", "-p"])
            .arg(root.path().join(image))
            .output()?;
        assert!(
            String::from_utf8(acl.stdout)?.contains("user:4242:rw-"),
            "{image}"
        );
    }
    backend
        .wait_ready(
            &handle,
            &Ready,
            Duration::from_secs(5),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(backend.address(&handle).await?.to_string(), "192.0.2.20");
    assert_eq!(backend.stats(&handle).await?.cpu_millis, 5);
    assert_eq!(backend.list().await?, std::slice::from_ref(&handle));
    backend.destroy(&handle).await?;
    backend.destroy(&handle).await?;
    assert!(!directory.exists(), "the token is gone after destroy");
    assert!(backend.list().await?.is_empty());
    assert!(
        root.path().join("workspaces/workspace_1.ext4").exists(),
        "the workspace outlives the sandbox"
    );
    Ok(())
}

#[tokio::test]
async fn sbx_f_017_failed_start_leaves_no_container_and_no_token() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine {
        fail_start: true,
        ..FakeEngine::default()
    });
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    assert!(
        backend
            .create(spec, CancellationToken::new())
            .await
            .is_err()
    );
    assert!(engine.containers.lock().map_err(|_| "lock")?.is_empty());
    assert_eq!(fs::read_dir(root.path().join("sandboxes"))?.count(), 0);
    Ok(())
}

#[tokio::test]
async fn sbx_f_017_exited_vm_and_foreign_containers() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let handle = backend.create(spec, CancellationToken::new()).await?;
    if let Some(container) = engine
        .containers
        .lock()
        .map_err(|_| "lock")?
        .get_mut(&handle.id)
    {
        // A guest reset ends the VMM with 0 whatever the runtime's code was.
        container.running = false;
        container.exit_code = Some(0);
    }
    let wait = || {
        backend.wait_ready(
            &handle,
            &Ready,
            Duration::from_secs(5),
            CancellationToken::new(),
        )
    };
    assert_eq!(
        wait().await,
        Err(SandboxError::HostDied),
        "no record: the VM host died (started once more by the gateway, SBX-F-038)"
    );
    let record = root
        .path()
        .join("sandboxes")
        .join(&handle.id)
        .join("exit.img");
    fs::write(&record, b"TQAEXIT1\x03\x00\x00\x00\xfc\xff\xff\xff")?;
    assert_eq!(
        wait().await,
        Err(SandboxError::RuntimeExited { code: 3 }),
        "the runtime's own code"
    );
    // A same-named container of another owner is neither reported nor removed.
    if let Some(container) = engine
        .containers
        .lock()
        .map_err(|_| "lock")?
        .get_mut(&handle.id)
    {
        container
            .labels
            .insert("tqa.instance".into(), "someone-else".into());
    }
    assert_eq!(backend.address(&handle).await, Err(SandboxError::NotFound));
    backend.destroy(&handle).await?;
    assert!(
        engine
            .containers
            .lock()
            .map_err(|_| "lock")?
            .contains_key(&handle.id)
    );
    Ok(())
}

#[tokio::test]
async fn sbx_f_017_directory_storage_is_refused_before_any_side_effect()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, mut spec) = backend(root.path(), Arc::clone(&engine)).await?;
    spec.state = StateMount::directory(root.path().join("state/conversation_1"));
    let error = backend
        .create(spec, CancellationToken::new())
        .await
        .err()
        .ok_or("accepted directory state")?;
    assert!(matches!(
        error,
        SandboxError::InvalidSpec { field: "state", .. }
    ));
    assert_eq!(
        engine.helpers.lock().map_err(|_| "lock")?.len(),
        3,
        "only the preflight probes and the storage-owner check ran"
    );
    Ok(())
}

#[tokio::test]
async fn sbx_f_017_template_readiness_checks_the_verified_rootfs() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let (backend, spec) = backend(root.path(), Arc::new(FakeEngine::default())).await?;
    backend.template_ready(&spec.image).await?;
    let absent = crate::ImageRef::local_id(format!("sha256:{}", "c".repeat(64)))?;
    assert_eq!(
        backend.template_ready(&absent).await,
        Err(SandboxError::Preflight(
            "no rootfs was built for this image"
        ))
    );
    let image = root
        .path()
        .join("images")
        .join(spec.image.digest().trim_start_matches("sha256:"));
    fs::write(image.join("rootfs.ext4"), b"ext5")?;
    assert_eq!(
        backend.template_ready(&spec.image).await,
        Err(SandboxError::Preflight(
            "the rootfs does not match its manifest"
        ))
    );
    Ok(())
}

#[tokio::test]
async fn sbx_f_001_a_foreign_storage_owner_mapping_is_refused_at_connect()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let (config, _) = host(root.path())?;
    let engine = Arc::new(FakeEngine {
        foreign_owner: true,
        ..FakeEngine::default()
    });
    let error = FirecrackerBackend::with_engine(config, engine)
        .await
        .err()
        .ok_or("accepted a foreign owner mapping")?;
    assert!(
        error
            .to_string()
            .contains("storage helpers cannot create files this process owns"),
        "{error}"
    );
    assert_eq!(
        fs::read_dir(root.path())?
            .filter_map(Result::ok)
            .filter(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with(".tqa-owner-probe"))
            .count(),
        0
    );
    Ok(())
}

#[tokio::test]
async fn sbx_f_007_after_a_reboot_exited_vms_are_absent_and_stale_directories_are_swept()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let handle = backend.create(spec, CancellationToken::new()).await?;
    // A host reboot: the VM host container exits (no restart policy) and keeps no address.
    if let Some(container) = engine
        .containers
        .lock()
        .map_err(|_| "lock")?
        .get_mut(&handle.id)
    {
        container.running = false;
        container.address = None;
    }
    assert_eq!(
        backend.address(&handle).await,
        Err(SandboxError::NotFound),
        "an exited VM host is not live"
    );
    assert_eq!(
        backend.list().await?,
        std::slice::from_ref(&handle),
        "still listed, so reconciliation destroys it"
    );
    // A per-sandbox directory whose container is gone (an engine prune) still holds a token: list sweeps it.
    let sandboxes = root.path().join("sandboxes");
    let stale = sandboxes.join("tqa-sandbox-conversation_old-1");
    fs::create_dir_all(&stale)?;
    fs::write(stale.join("config.img"), TOKEN)?;
    let foreign = sandboxes.join("unrelated");
    fs::create_dir_all(&foreign)?;
    backend.list().await?;
    assert!(!stale.exists(), "a stale token directory survived");
    assert!(
        foreign.exists(),
        "only this instance's directories are swept"
    );
    assert!(
        sandboxes.join(&handle.id).exists(),
        "a listed container keeps its directory"
    );
    backend.destroy(&handle).await?;
    assert!(!sandboxes.join(&handle.id).exists());
    Ok(())
}

#[tokio::test]
async fn sbx_f_017_a_busy_engine_never_makes_destroy_leave_a_vm_behind()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let handle = backend.create(spec, CancellationToken::new()).await?;
    // A timeout on the first inspection is an error, never a silent success, and the VM host stays.
    engine
        .inspect_timeouts
        .lock()
        .map_err(|_| "lock")?
        .push_back(true);
    assert_eq!(backend.destroy(&handle).await, Err(SandboxError::Timeout));
    assert!(
        engine
            .containers
            .lock()
            .map_err(|_| "lock")?
            .contains_key(&handle.id)
    );
    // Inspected fine, then a later inspection times out: destroy must still remove it (or fail), never succeed silently.
    engine
        .inspect_timeouts
        .lock()
        .map_err(|_| "lock")?
        .extend([false, true]);
    let destroyed = backend.destroy(&handle).await;
    let left = engine
        .containers
        .lock()
        .map_err(|_| "lock")?
        .contains_key(&handle.id);
    assert!(
        !(destroyed.is_ok() && left),
        "destroy reported success but left the VM host"
    );
    assert!(
        !left,
        "the VM host was removed on the inspection that succeeded: {destroyed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn sbx_c_014_the_microvm_backend_applies_dockers_image_policy() -> Result<(), Box<dyn Error>>
{
    for (policy, named) in [
        ((&["LANG", "API_TOKEN"][..], &[][..]), Some("API_TOKEN")),
        ((&["LANG"][..], &["/data"][..]), Some("volumes")),
        ((&["LANG"][..], &[][..]), None),
    ] {
        let root = tempfile::tempdir()?;
        let engine = Arc::new(FakeEngine::default());
        let (config, spec) = host_with(root.path(), policy)?;
        let backend = FirecrackerBackend::with_engine(config, engine.clone()).await?;
        let created = backend.create(spec, CancellationToken::new()).await;
        let reason = match &created {
            Err(SandboxError::InvalidSpec {
                field: "image",
                reason,
            }) => Some(reason.to_string()),
            _ => None,
        };
        assert_eq!(reason.is_some(), named.is_some(), "{policy:?}: {created:?}");
        assert!(
            reason
                .as_deref()
                .zip(named)
                .is_none_or(|(reason, named)| reason.contains(named)),
            "{reason:?}"
        );
        let refused = named.is_some();
        let containers = engine.containers.lock().map_err(|_| "lock")?.len();
        assert_eq!(
            containers,
            usize::from(!refused),
            "refused before any VM host"
        );
        let image = root.path().join("workspaces/workspace_1.ext4");
        assert_eq!(image.exists(), !refused, "and before any image");
    }
    Ok(())
}

#[tokio::test]
async fn sbx_f_022_a_template_pinned_by_registry_digest_starts_from_its_image_ids_rootfs()
-> Result<(), Box<dyn Error>> {
    const PINNED: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (config, mut spec) = host_with(root.path(), (&["LANG"][..], &[][..]))?;
    // The builder saw the image under a registry digest; the build stays keyed by the image ID.
    let manifest = root
        .path()
        .join("images")
        .join(spec.image.digest().trim_start_matches("sha256:"))
        .join("manifest.json");
    let mut document: serde_json::Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    document["source_image"]["repo_digests"] =
        json!([format!("registry.example/team/template@{PINNED}")]);
    fs::write(&manifest, serde_json::to_vec(&document)?)?;
    let backend = FirecrackerBackend::with_engine(config, engine.clone()).await?;
    // Not listed by any build (and unlike the fixture's image ID).
    let unknown = crate::ImageRef::new(
        "registry.example/team/template",
        format!("sha256:{}", "c".repeat(64)),
    )?;
    let answer = backend.template_ready(&unknown).await;
    assert!(
        matches!(
            answer,
            Err(SandboxError::Preflight(
                "no rootfs was built for this image"
            ))
        ),
        "{answer:?}"
    );
    let mut refused = spec.clone();
    refused.image = unknown;
    assert!(
        backend
            .create(refused, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(engine.containers.lock().map_err(|_| "lock")?.len(), 0);
    spec.image = crate::ImageRef::new("registry.example/team/template", PINNED)?;
    backend.template_ready(&spec.image).await?;
    backend.create(spec, CancellationToken::new()).await?;
    Ok(())
}

#[tokio::test]
async fn sbx_f_023_the_backend_reports_a_template_status_without_side_effects()
-> Result<(), Box<dyn Error>> {
    use super::super::manifest::RootfsStatus;
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (mut config, spec) = host_with(root.path(), (&["LANG"][..], &[][..]))?;
    let backend = FirecrackerBackend::with_engine(config.clone(), engine.clone()).await?;
    assert_eq!(
        backend.template_status(&spec.image).await?,
        RootfsStatus::Ready
    );
    let unknown = crate::ImageRef::local_id(format!("sha256:{}", "c".repeat(64)))?;
    assert_eq!(
        backend.template_status(&unknown).await?,
        RootfsStatus::Missing
    );
    // A registry pin no build lists: missing, found without hashing anything.
    let unlisted = crate::ImageRef::new(
        "registry.example/template",
        format!("sha256:{}", "c".repeat(64)),
    )?;
    assert_eq!(
        backend.template_status(&unlisted).await?,
        RootfsStatus::Missing
    );
    // Another VM host image built it.
    config.vmm_image = crate::ImageRef::local_id(format!("sha256:{}", "9".repeat(64)))?;
    let upgraded = FirecrackerBackend::with_engine(config.clone(), engine.clone()).await?;
    assert_eq!(
        upgraded.template_status(&spec.image).await?,
        RootfsStatus::Outdated
    );
    // A VM host image pinned by registry digest is not compared (its digest need not be its ID).
    config.vmm_image = crate::ImageRef::new(
        "registry.example/tqa-vmm",
        format!("sha256:{}", "9".repeat(64)),
    )?;
    let pinned = FirecrackerBackend::with_engine(config, engine.clone()).await?;
    assert_eq!(
        pinned.template_status(&spec.image).await?,
        RootfsStatus::Ready
    );
    // A registry pin the build lists is located and verified like its image ID.
    let manifest = root
        .path()
        .join("images")
        .join(spec.image.digest().trim_start_matches("sha256:"))
        .join("manifest.json");
    let mut document: serde_json::Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    let listed = format!("sha256:{}", "a".repeat(64));
    document["source_image"]["repo_digests"] =
        json!([format!("registry.example/template@{listed}")]);
    fs::write(&manifest, serde_json::to_vec(&document)?)?;
    let registry = crate::ImageRef::new("registry.example/template", listed)?;
    assert_eq!(
        pinned.template_status(&registry).await?,
        RootfsStatus::Ready
    );
    assert_eq!(
        engine.containers.lock().map_err(|_| "lock")?.len(),
        0,
        "no side effects"
    );
    Ok(())
}

#[tokio::test]
async fn sbx_f_023_create_refuses_a_rootfs_built_by_another_vm_host_image()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (mut config, spec) = host_with(root.path(), (&["LANG"][..], &[][..]))?;
    // An upgraded VM host image: its guest init is not the one inside this rootfs.
    config.vmm_image = crate::ImageRef::local_id(format!("sha256:{}", "9".repeat(64)))?;
    let upgraded = FirecrackerBackend::with_engine(config.clone(), engine.clone()).await?;
    let reason = Some(
        "the rootfs was built by another VM host image; rebuild it with images/rootfs/build_rootfs.py",
    );
    let refused = upgraded
        .create(spec.clone(), CancellationToken::new())
        .await;
    assert_eq!(
        match &refused {
            Err(SandboxError::Preflight(reason)) => Some(*reason),
            _ => None,
        },
        reason,
        "{refused:?}"
    );
    let ready = upgraded.template_ready(&spec.image).await;
    assert!(
        matches!(ready, Err(SandboxError::Preflight(_))),
        "{ready:?}"
    );
    assert_eq!(engine.containers.lock().map_err(|_| "lock")?.len(), 0);
    // A VM host image pinned by registry digest is not compared, as in the status.
    config.vmm_image = crate::ImageRef::new(
        "registry.example/tqa-vmm",
        format!("sha256:{}", "9".repeat(64)),
    )?;
    let pinned = FirecrackerBackend::with_engine(config, engine.clone()).await?;
    pinned.create(spec, CancellationToken::new()).await?;
    Ok(())
}

#[tokio::test]
async fn sbx_u_007_the_microvm_backend_is_the_rootfs_source_of_the_template_checks()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let (config, spec) = host_with(root.path(), (&["LANG"][..], &[][..]))?;
    let backend = FirecrackerBackend::with_engine(config, Arc::new(FakeEngine::default())).await?;
    let source: &dyn crate::template::RootfsSource = &backend;
    assert_eq!(
        source.status(&spec.image).await?,
        backend.template_status(&spec.image).await?
    );
    Ok(())
}

/// A container as a stopped gateway may leave it: `labels` as given, running or not.
fn left(name: &str, labels: &[(&str, &str)], running: bool) -> (String, Container) {
    let labels = labels
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    (
        name.to_owned(),
        Container {
            name: name.to_owned(),
            labels,
            running,
            ..Container::default()
        },
    )
}

// TEST: SBX-F-024
#[tokio::test]
async fn sbx_f_024_the_backend_removes_its_own_leftover_helpers_at_start()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let (config, _) = host(root.path())?;
    let ours = config.network.clone();
    let engine = Arc::new(FakeEngine::default());
    engine.containers.lock().map_err(|_| "lock")?.extend([
        // Created and never started, and one still running: a gateway stopped between create and remove.
        left(
            "ours-created",
            &[("tqa.instance", &ours), ("tqa.role", "helper")],
            false,
        ),
        left(
            "ours-running",
            &[("tqa.instance", &ours), ("tqa.role", "helper")],
            true,
        ),
        // Another instance's helper, and this instance's VM host, are not leftovers of this backend.
        left(
            "theirs",
            &[("tqa.instance", "someone-else"), ("tqa.role", "helper")],
            false,
        ),
        left(
            "vm-host",
            &[("tqa.instance", &ours), ("tqa.backend", "firecracker")],
            true,
        ),
    ]);
    FirecrackerBackend::with_engine(config, Arc::clone(&engine) as Arc<dyn Engine>).await?;
    let mut remaining: Vec<String> = engine
        .containers
        .lock()
        .map_err(|_| "lock")?
        .keys()
        .cloned()
        .collect();
    remaining.sort();
    assert_eq!(remaining, ["theirs", "vm-host"]);
    Ok(())
}

// TEST: SBX-F-024
#[tokio::test]
async fn sbx_f_024_every_helper_carries_the_instance_and_its_role_never_the_backend_label()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    // The storage-owner probe ran at start; a new sandbox's block images run mkfs.
    let handle = backend.create(spec, CancellationToken::new()).await?;
    let helpers = engine.helpers.lock().map_err(|_| "lock")?.clone();
    assert!(helpers.len() >= 2, "the probe and at least one mkfs");
    for body in &helpers {
        let labels = body.labels.clone().unwrap_or_default();
        assert_eq!(
            (
                labels.get("tqa.instance").map(String::as_str),
                labels.get("tqa.role").map(String::as_str),
                labels.get("tqa.backend"),
            ),
            (Some(backend.config.network.as_str()), Some("helper"), None),
            "{:?}",
            body.cmd
        );
    }
    // So the backend's own listing, which reads every container as a VM host, never sees one.
    assert_eq!(backend.list().await?.len(), 1);
    backend.destroy(&handle).await?;
    Ok(())
}

// TEST: SBX-F-025
#[test]
fn sbx_f_025_a_workspace_image_is_its_quota_plus_ext4_metadata() -> Result<(), Box<dyn Error>> {
    use crate::ByteSize;
    let mib = |value: u64| value << 20;
    for (quota, image) in [
        // At least 128 MiB: at 1 GiB, ext4 takes about 53 MiB before any file is written.
        (mib(1024), mib(1152)),
        (mib(512), mib(640)),
        // 5% from 2.5 GiB up.
        (mib(10240), mib(10752)),
        (mib(2560), mib(2688)),
    ] {
        assert_eq!(
            super::workspace_image_size(ByteSize::new(quota)?)?.bytes(),
            image,
            "{quota}"
        );
    }
    Ok(())
}

/// Whether `cmd` is the image helper: `/bin/sh -c` with a script running mkfs (the probes are `/bin/sh -c` too).
pub(super) fn is_image_helper(cmd: &[String]) -> bool {
    cmd.first().map(String::as_str) == Some("/bin/sh")
        && cmd
            .get(2)
            .is_some_and(|script| script.contains("mkfs.ext4"))
}
