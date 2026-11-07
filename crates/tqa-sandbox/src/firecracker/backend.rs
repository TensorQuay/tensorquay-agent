//! The Firecracker `SandboxBackend` (ADR-0006). A sandbox is one VM host container named after the sandbox; its
//! configuration device, VM description and rules live in a per-sandbox directory of the same name. Every operation
//! acts only on containers carrying this instance's labels, and a failed create leaves no container and no token.

use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType};
use futures::FutureExt;
use tokio_util::sync::CancellationToken;

use super::{
    FirecrackerBackendConfig, VMM_CONTAINER_UID,
    acl::{self, Access},
    device,
    docker_engine::DockerEngine,
    engine::{BACKEND_LABEL, Container, Created, Engine, helper_labels},
    exit_record,
    launch::{self, Files},
    manifest::{self, Kernel, Verified},
    preflight,
    probe::EngineProbe,
    rules,
};
use crate::retry::{Budget, Retry};

/// How often a retry looks for an earlier attempt's create that still holds the VM host's name (SBX-F-035).
const LATE_CREATE_POLL: Duration = Duration::from_millis(250);
use crate::{
    BackendKind, ByteSize, ReadinessSource, SandboxBackend, SandboxError, SandboxHandle,
    SandboxSpec, SandboxStats, StateSource, WorkspaceSource, readiness,
};

/// The images that don't exist yet, each with its size; a missing image without a size is refused.
fn missing_images<'a>(
    images: &[(&'a Path, Option<ByteSize>)],
) -> Result<Vec<(&'a Path, ByteSize)>, SandboxError> {
    images
        .iter()
        .filter(|(path, _)| !path.exists())
        .map(|(path, size)| {
            size.map(|size| (*path, size))
                .ok_or_else(|| SandboxError::invalid("disk", "a new block image needs a size"))
        })
        .collect()
}

/// Makes each `(file, size)` argument pair an ext4 image: under a temporary name, moved into place only once mkfs has
/// succeeded, so a failed or interrupted helper never leaves an image a later start would take as made. No root
/// reserve (the guest writes as uid 1000, SBX-F-025) and the root owned by that uid. Names and sizes are arguments only.
const IMAGE_SCRIPT: &str = r#"set -eu
while [ "$#" -ge 2 ]; do
  rm -f "$1.tmp"
  /usr/sbin/mkfs.ext4 -q -m 0 -E root_owner=1000:1000 "$1.tmp" "$2"
  mv "$1.tmp" "$1"
  shift 2
done"#;

/// The microVM backend over a container engine.
pub struct FirecrackerBackend {
    config: FirecrackerBackendConfig,
    engine: Arc<dyn Engine>,
    kernel: Kernel,
    verified: Arc<Verified>,
    /// The storage root's owner; helper containers create images as this identity, so the backend owns them.
    owner: (u32, u32),
    /// Sandboxes being created: their directories exist before their containers, so a listing never sweeps them.
    creating: std::sync::Mutex<std::collections::HashSet<String>>,
    /// How a start's engine calls are retried while the engine is too busy to answer (SBX-F-028, SBX-F-030), inside
    /// the start's create budget (SBX-F-031).
    retry: Retry,
}

impl FirecrackerBackend {
    /// The same backend with another retry policy (tests: a short back-off instead of sleeping).
    #[cfg(test)]
    pub(crate) fn with_retry(mut self, retry: Retry) -> Self {
        self.retry = retry;
        self
    }

    /// Connects to the engine and refuses to start unless the host preflight passes (SBX-F-001).
    ///
    /// # Errors
    /// Returns `InvalidSpec` for an invalid configuration or `Preflight` with the missing requirement.
    pub async fn connect(config: FirecrackerBackendConfig) -> Result<Self, SandboxError> {
        config.validate()?;
        let unavailable = |_| SandboxError::Backend("the Docker Engine is unavailable".into());
        let docker = bollard::Docker::connect_with_defaults().map_err(unavailable)?;
        let docker = tokio::time::timeout(config.operation_timeout, docker.negotiate_version())
            .await
            .map_err(|_| SandboxError::Timeout)?
            .map_err(unavailable)?;
        let engine = DockerEngine::new(docker, config.network.clone(), config.operation_timeout);
        Self::with_engine(config, Arc::new(engine)).await
    }

    pub(crate) async fn with_engine(
        config: FirecrackerBackendConfig,
        engine: Arc<dyn Engine>,
    ) -> Result<Self, SandboxError> {
        preflight::run(
            &config,
            &EngineProbe {
                engine: &*engine,
                image: &config.vmm_image,
                network: &config.network,
                bound: config.create_timeout,
            },
        )
        .await?;
        let verified = Arc::new(Verified::default());
        let (directory, cache) = (config.kernel_dir.clone(), Arc::clone(&verified));
        let kernel = blocking(config.operation_timeout, move || {
            manifest::kernel(&directory, &cache)
        })
        .await?;
        let root = fs::metadata(&config.storage_root)
            .map_err(|_| SandboxError::Preflight("storage.root is missing"))?;
        // No backend can still be using a helper of this instance: one stopped between create and remove left it. This
        // assumes one gateway per instance (tqa.instance), as installs run today; a rolling restart that overlaps two
        // gateways on one instance would need the sweep to spare the other's live helpers.
        for name in engine.helpers(&config.network).await? {
            engine.remove(&name).await?;
        }
        let backend = Self {
            owner: (root.uid(), root.gid()),
            config,
            engine,
            kernel,
            verified,
            creating: std::sync::Mutex::default(),
            retry: Retry::default(),
        };
        backend.check_storage_helper().await?;
        Ok(backend)
    }

    /// Whether a template can start: its rootfs exists, matches its manifest and speaks at least the protocol floor.
    /// Verified files are cached by identity, so a repeated check does not re-hash an unchanged rootfs.
    ///
    /// # Errors
    /// Returns `Preflight` naming what is missing or different, or `RuntimeTooOld`.
    pub async fn template_ready(&self, image: &crate::ImageRef) -> Result<(), SandboxError> {
        self.verified_rootfs(image).await.map(|_| ())
    }

    /// The status of a template's rootfs for the admin API (ADR-0032): create's checks plus whether the configured VM
    /// host image built it, compared only when that image is pinned by its local ID (a registry digest need not be the
    /// ID the builder recorded). No side effects; the storage is read off the async threads within the rootfs's bound.
    ///
    /// # Errors
    /// A timeout or a failure to read the storage; every finding about the rootfs is a status.
    pub async fn template_status(
        &self,
        image: &crate::ImageRef,
    ) -> Result<manifest::RootfsStatus, SandboxError> {
        let directory = self.config.rootfs_dir.clone();
        let builder = self.builder();
        let cache = Arc::clone(&self.verified);
        let located = if image.is_local() {
            Some(image.clone())
        } else {
            self.located(image).await?
        };
        let Some(located) = located else {
            // Not located: the status says why (missing or ambiguous) without hashing anything.
            let image = image.clone();
            return blocking(self.config.operation_timeout, move || {
                Ok(manifest::status(&directory, &image, None, &cache))
            })
            .await;
        };
        let bound = manifest::verify_bound(&directory, &located, self.config.operation_timeout);
        blocking(bound, move || {
            Ok(manifest::status(
                &directory,
                &located,
                builder.as_deref(),
                &cache,
            ))
        })
        .await
    }

    /// A registry pin's local image ID, or `None` when not exactly one build lists it.
    async fn located(
        &self,
        image: &crate::ImageRef,
    ) -> Result<Option<crate::ImageRef>, SandboxError> {
        let (directory, image) = (self.config.rootfs_dir.clone(), image.clone());
        match blocking(self.config.operation_timeout, move || {
            manifest::locate(&directory, &image)
        })
        .await
        {
            Ok(located) => Ok(Some(located)),
            Err(SandboxError::Preflight(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The verified rootfs for `image`, found by its image ID (a registry-digest pin is located first, SBX-F-022).
    /// Both steps read the storage off the async threads, each within its bound.
    async fn verified_rootfs(
        &self,
        image: &crate::ImageRef,
    ) -> Result<manifest::Rootfs, SandboxError> {
        let directory = self.config.rootfs_dir.clone();
        let image = if image.is_local() {
            image.clone()
        } else {
            let (directory, image) = (directory.clone(), image.clone());
            blocking(self.config.operation_timeout, move || {
                manifest::locate(&directory, &image)
            })
            .await?
        };
        let cache = Arc::clone(&self.verified);
        let builder = self.builder();
        let bound = manifest::verify_bound(&directory, &image, self.config.operation_timeout);
        blocking(bound, move || {
            manifest::rootfs(&directory, &image, builder.as_deref(), &cache)
        })
        .await
    }

    /// The configured VM host image's ID when it is pinned by that ID. The guest init comes from the rootfs builder, so a
    /// rootfs from another VM host image is refused; a registry digest need not be the ID the builder recorded, so then
    /// the builder is not compared.
    fn builder(&self) -> Option<String> {
        self.config
            .vmm_image
            .is_local()
            .then(|| self.config.vmm_image.digest().to_owned())
    }

    fn in_flight(&self, name: &str, creating: bool) -> Result<(), SandboxError> {
        let mut set = self
            .creating
            .lock()
            .map_err(|_| SandboxError::Backend("creation registry poisoned".into()))?;
        if creating {
            set.insert(name.to_owned());
        } else {
            set.remove(name);
        }
        Ok(())
    }

    fn sandbox_dir(&self, name: &str) -> PathBuf {
        self.config.storage_root.join("sandboxes").join(name)
    }

    /// The container for `handle`, only when it carries this instance's labels.
    async fn owned(&self, handle: &SandboxHandle) -> Result<Container, SandboxError> {
        if handle.kind != BackendKind::Firecracker {
            return Err(SandboxError::NotFound);
        }
        let container = self.engine.inspect(&handle.id).await?;
        if self.is_ours(&container, handle) {
            Ok(container)
        } else {
            Err(SandboxError::NotFound)
        }
    }

    /// Whether an inspected container is this instance's VM host for `handle`'s conversation (by its labels).
    fn is_ours(&self, container: &Container, handle: &SandboxHandle) -> bool {
        let label = |key: &str| container.labels.get(key).map(String::as_str);
        label("tqa.instance") == Some(&self.config.network)
            && label(BACKEND_LABEL.0) == Some(BACKEND_LABEL.1)
            && label("tqa.conversation") == Some(&handle.conversation_id.to_string())
    }

    async fn provision(
        &self,
        spec: &SandboxSpec,
        name: &str,
        created_at: SystemTime,
    ) -> Result<(), SandboxError> {
        let (workspace, state) = block_images(spec)?;
        // One budget for this start's retried engine calls: a person waits at most this long (SBX-F-031).
        let budget = Budget::new(self.config.create_budget);
        let rootfs = self.verified_rootfs(&spec.image).await?;
        // Docker's image policy, before any side effect (SBX-C-014).
        rootfs.policy.check(spec)?;
        self.ensure_images(
            &[
                (workspace, spec.disk.map(workspace_image_size).transpose()?),
                (state, Some(self.config.state_image)),
            ],
            budget,
        )
        .await?;
        let directory = self.sandbox_dir(name);
        let contents = device::document(spec, self.config.gateway_address.into())?;
        let vm = serde_json::to_vec_pretty(&launch::vm_config(spec)?)
            .map_err(|_| SandboxError::Backend("encoding the VM description failed".into()))?;
        let rules = rules::ruleset(self.config.gateway_address, &self.config.gateway_ports);
        let (target, uid) = (directory.clone(), self.config.vmm_file_uid);
        blocking(self.config.operation_timeout, move || {
            device::write(&target, &contents, uid)?;
            exit_record::create(&target, uid)?;
            write_public(&target.join("vm.json"), &vm)?;
            write_public(&target.join("rules.nft"), rules.as_bytes())
        })
        .await?;
        let files = Files {
            kernel: &self.kernel.path,
            rootfs: &rootfs.path,
            workspace,
            state,
            sandbox_dir: &directory,
        };
        let body = launch::container(spec, &self.config, &files, name, created_at)?;
        self.retry
            .run(
                "starting a VM host",
                budget,
                self.config.create_timeout,
                // Each attempt's bound is create_timeout cut to the budget; the start recomputes it after a wait.
                |number, _| self.start_vm_host(name, body.clone(), number, budget),
            )
            .await
    }

    /// One attempt at the VM host's start. From the second, the attempt before may have timed out while the engine
    /// still created its container, which then holds the name (SBX-F-033): that container, this sandbox's own name and
    /// no other, is removed first. A create that finds the name held by one still in flight waits for it within the
    /// budget, removes it, and creates again; that spends no attempt (SBX-F-035).
    async fn start_vm_host(
        &self,
        name: &str,
        body: ContainerCreateBody,
        number: u32,
        budget: Budget,
    ) -> Result<(), SandboxError> {
        if number > 1 {
            self.engine.remove(name).await?;
        }
        loop {
            let bound = self.config.create_timeout.min(budget.left());
            match self
                .engine
                .create_and_start(name, body.clone(), bound)
                .await?
            {
                Created::Started => return Ok(()),
                Created::NameHeld => self.await_late_vm_host(name, budget).await?,
            }
        }
    }

    /// Waits, within `budget`, for the create in flight under this sandbox's `name` to land, then removes it. Once the
    /// budget is spent it is a timeout, as the start's own create would be.
    async fn await_late_vm_host(&self, name: &str, budget: Budget) -> Result<(), SandboxError> {
        loop {
            if budget.left().is_zero() {
                return Err(SandboxError::Timeout);
            }
            match self.engine.inspect(name).await {
                Ok(_) => return self.engine.remove(name).await,
                Err(SandboxError::NotFound) => {}
                Err(error) => return Err(error),
            }
            tokio::time::sleep(LATE_CREATE_POLL.min(budget.left())).await;
        }
    }

    /// Creates the missing ext4 images in one helper container (SBX-F-029: one container run per start, not one per
    /// image), then makes each private to its owner plus the VMM uid.
    async fn ensure_images(
        &self,
        images: &[(&Path, Option<ByteSize>)],
        budget: Budget,
    ) -> Result<(), SandboxError> {
        let missing = missing_images(images)?;
        if !missing.is_empty() {
            self.make_whole(&missing, budget).await?;
        }
        for (path, _) in images {
            let (path, uid) = (path.to_path_buf(), self.config.vmm_file_uid);
            blocking(self.config.operation_timeout, move || {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(|error| storage(&error))?;
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|error| storage(&error))?;
                acl::grant(&file, uid, Access::ReadWrite)
            })
            .await?;
        }
        Ok(())
    }

    /// Makes the missing images, retrying a busy engine as the VM host's start is (SBX-F-030, SBX-F-028); on failure
    /// nothing a helper wrote is left for a later start to take as made.
    async fn make_whole(
        &self,
        missing: &[(&Path, ByteSize)],
        budget: Budget,
    ) -> Result<(), SandboxError> {
        let body = self.image_helper(missing)?;
        let made = self
            .retry
            .run(
                "making a block image",
                budget,
                self.config.create_timeout,
                |_, bound| self.make_images(missing, body.clone(), bound),
            )
            .await;
        if made.is_err() {
            for (path, _) in missing {
                self.remove_partial(path).await?;
            }
        }
        made
    }

    /// One attempt at the missing images: from no file (a helper the engine removed mid-mkfs may have left a temporary
    /// one, which the script also removes), then the one helper.
    async fn make_images(
        &self,
        missing: &[(&Path, ByteSize)],
        body: ContainerCreateBody,
        bound: Duration,
    ) -> Result<(), SandboxError> {
        for (path, _) in missing {
            self.remove_partial(path).await?;
        }
        let code = self.engine.run(body, bound).await?;
        if code != 0 {
            return Err(SandboxError::Backend(
                "creating a block image failed".into(),
            ));
        }
        Ok(())
    }

    /// Removes what a failed or timed-out image helper left for `path`: the image and its temporary (`.tmp`) file;
    /// nothing there is fine.
    async fn remove_partial(&self, path: &Path) -> Result<(), SandboxError> {
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        let paths = [path.to_path_buf(), temporary.into()];
        blocking(self.config.operation_timeout, move || {
            paths
                .iter()
                .try_for_each(|path| match fs::remove_file(path) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        Err(storage(&error))
                    }
                    _ => Ok(()),
                })
        })
        .await
    }

    /// The helper that makes `images`: each directory mounted once (`/img0`, `/img1`), and the (file, size) pairs passed
    /// to [`IMAGE_SCRIPT`] as arguments, never written into it.
    fn image_helper(
        &self,
        images: &[(&Path, ByteSize)],
    ) -> Result<ContainerCreateBody, SandboxError> {
        let invalid = || {
            SandboxError::invalid(
                "disk",
                "block image path must be an absolute UTF-8 file path",
            )
        };
        let mut directories: Vec<&Path> = Vec::new();
        let mut arguments = vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            IMAGE_SCRIPT.to_owned(),
            "sh".to_owned(),
        ];
        for (path, size) in images {
            let directory = path.parent().ok_or_else(invalid)?;
            let file = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(invalid)?;
            let index = directories
                .iter()
                .position(|known| *known == directory)
                .unwrap_or_else(|| {
                    directories.push(directory);
                    directories.len() - 1
                });
            arguments.push(format!("/img{index}/{file}"));
            arguments.push(format!("{}k", size.bytes() / 1024));
        }
        let mounts = directories
            .iter()
            .enumerate()
            .map(|(index, directory)| Some((directory.to_str()?, format!("/img{index}"))))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        let cmd: Vec<&str> = arguments.iter().map(String::as_str).collect();
        Ok(self.helper_with(&mounts, &cmd))
    }

    /// A short-lived helper in the VM host image, running as the storage owner with `directory` at `/img`. Its labels
    /// let a torn-down stack's sweep and this backend's next start remove it, if it is ever left behind (SBX-F-024).
    fn helper(&self, directory: &Path, cmd: &[&str]) -> Option<ContainerCreateBody> {
        Some(self.helper_with(&[(directory.to_str()?, "/img".to_owned())], cmd))
    }

    /// As [`Self::helper`], with each `(host directory, target)` bind-mounted read-write.
    fn helper_with(&self, mounts: &[(&str, String)], cmd: &[&str]) -> ContainerCreateBody {
        ContainerCreateBody {
            image: Some(self.config.vmm_image.to_string()),
            labels: Some(helper_labels(&self.config.network)),
            user: Some(format!("{}:{}", self.owner.0, self.owner.1)),
            entrypoint: Some(Vec::new()),
            cmd: Some(cmd.iter().map(|&part| part.to_owned()).collect()),
            host_config: Some(HostConfig {
                network_mode: Some("none".into()),
                readonly_rootfs: Some(true),
                cap_drop: Some(vec!["ALL".into()]),
                security_opt: Some(vec!["no-new-privileges".into()]),
                mounts: Some(
                    mounts
                        .iter()
                        .map(|(source, target)| Mount {
                            typ: Some(MountType::BIND),
                            source: Some((*source).into()),
                            target: Some(target.clone()),
                            read_only: Some(false),
                            ..Default::default()
                        })
                        .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Proves that a helper running as the storage owner creates files this process owns and can open: the engine's
    /// uid mapping must match this process's view (the gateway in a container on the same engine, or tests run
    /// inside the engine's user namespace). Otherwise every new image would fail on first use.
    async fn check_storage_helper(&self) -> Result<(), SandboxError> {
        const REASON: &str = "storage helpers cannot create files this process owns \
            (run the gateway in a container on the same engine, or tests through tools/rootless-test.sh)";
        let name = format!(".tqa-owner-probe-{}", std::process::id());
        let body = self
            .helper(
                &self.config.storage_root,
                &["/usr/bin/touch", &format!("/img/{name}")],
            )
            .ok_or(SandboxError::Preflight(REASON))?;
        let code = self.engine.run(body, self.config.create_timeout).await?;
        let path = self.config.storage_root.join(&name);
        let owned = code == 0
            && OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .is_ok()
            && fs::metadata(&path)
                .is_ok_and(|metadata| (metadata.uid(), metadata.gid()) == self.owner);
        let _ = fs::remove_file(&path);
        if owned {
            Ok(())
        } else {
            Err(SandboxError::Preflight(REASON))
        }
    }
}

#[async_trait]
impl SandboxBackend for FirecrackerBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Firecracker
    }

    async fn create(
        &self,
        spec: SandboxSpec,
        cancel: CancellationToken,
    ) -> Result<SandboxHandle, SandboxError> {
        if cancel.is_cancelled() {
            return Err(SandboxError::Cancelled);
        }
        spec.validate(BackendKind::Firecracker)?;
        let sampled = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| SandboxError::Preflight("host clock precedes the Unix epoch"))?;
        let created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(sampled.as_secs());
        let name = format!(
            "{}-{}-{:x}",
            self.config.network,
            spec.conversation_id,
            sampled.as_nanos()
        );
        self.in_flight(&name, true)?;
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => Err(SandboxError::Cancelled),
            result = self.provision(&spec, &name, created_at) => result,
        };
        self.in_flight(&name, false)?;
        if let Err(error) = result {
            // Nothing survives a failed create: no container, and no token on the host.
            let removed = self.engine.remove(&name).await;
            let cleared = device::remove(&self.sandbox_dir(&name));
            removed.and(cleared)?;
            return Err(error);
        }
        tracing::info!(
            image_digest = spec.image.digest(),
            vmm_uid = VMM_CONTAINER_UID,
            "started microVM sandbox"
        );
        Ok(SandboxHandle {
            kind: BackendKind::Firecracker,
            id: name,
            conversation_id: spec.conversation_id,
            created_at,
        })
    }

    async fn address(&self, handle: &SandboxHandle) -> Result<std::net::IpAddr, SandboxError> {
        let container = self.owned(handle).await?;
        // As in Docker mode: an exited VM host (after a host reboot, or a VM that died) is not a live sandbox.
        if !container.running {
            return Err(SandboxError::NotFound);
        }
        container.address.ok_or_else(|| {
            SandboxError::Backend("the VM host has no address on its network".into())
        })
    }

    async fn wait_ready(
        &self,
        handle: &SandboxHandle,
        readiness: &dyn ReadinessSource,
        deadline: Duration,
        cancel: CancellationToken,
    ) -> Result<(), SandboxError> {
        let bounds = (deadline, self.config.readiness_interval);
        readiness::poll(
            &handle.conversation_id,
            readiness,
            bounds,
            &cancel,
            &mut || {
                async move {
                    let container = self.owned(handle).await?;
                    if container.running {
                        Ok(())
                    } else {
                        // A guest reset ends the VMM with 0; the runtime's code is in the exit record, and a VM
                        // host that ended without one died itself (SBX-F-038).
                        let Some(code) = exit_record::read(&self.sandbox_dir(&handle.id)) else {
                            let lines = self.engine.log_tail(&handle.id, VM_HOST_LOG_LINES).await;
                            let (meaning, own) = vm_host_death(container.exit_code, &lines);
                            tracing::warn!(
                                sandbox = %handle.id,
                                exit_code = ?container.exit_code,
                                meaning = %meaning,
                                last_lines = %own,
                                "the VM host ended before the runtime registered"
                            );
                            return Err(SandboxError::HostDied);
                        };
                        Err(SandboxError::RuntimeExited { code })
                    }
                }
                .boxed()
            },
        )
        .await
    }

    async fn stats(&self, handle: &SandboxHandle) -> Result<SandboxStats, SandboxError> {
        self.owned(handle).await?;
        let usage = self.engine.usage(&handle.id).await?;
        Ok(SandboxStats {
            cpu_millis: usage.cpu_nanos / 1_000_000,
            memory_bytes: usage.memory_bytes,
            pids: usage.pids,
            uptime: handle.created_at.elapsed().unwrap_or_default(),
        })
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        if handle.kind != BackendKind::Firecracker {
            return Ok(());
        }
        match self.engine.inspect(&handle.id).await {
            // A container of that name that is not ours is never touched. Ownership is read from this inspection:
            // a second engine call that failed under load once left a VM host behind while destroy succeeded.
            Ok(container) if !self.is_ours(&container, handle) => return Ok(()),
            Ok(_) => self.engine.remove(&handle.id).await?,
            Err(SandboxError::NotFound) => {}
            Err(error) => return Err(error),
        }
        // Idempotent: an absent container and an absent directory both succeed.
        device::remove(&self.sandbox_dir(&handle.id))?;
        tracing::info!("destroyed microVM sandbox");
        Ok(())
    }

    /// Lists this instance's VM hosts, running or exited, and removes any per-sandbox directory whose container no
    /// longer exists (a host reboot followed by an engine prune), so no token outlives its VM. Directories of
    /// sandboxes still being created are kept.
    async fn engine_networks(&self) -> Result<crate::EngineNetworks, SandboxError> {
        self.engine.networks().await
    }

    async fn list(&self) -> Result<Vec<SandboxHandle>, SandboxError> {
        let handles: Vec<SandboxHandle> = self
            .engine
            .list(&self.config.network)
            .await?
            .into_iter()
            .map(handle)
            .collect::<Result<_, _>>()?;
        let mut keep: std::collections::HashSet<String> =
            handles.iter().map(|handle| handle.id.clone()).collect();
        keep.extend(
            self.creating
                .lock()
                .map_err(|_| SandboxError::Backend("creation registry poisoned".into()))?
                .iter()
                .cloned(),
        );
        let (directory, prefix) = (
            self.config.storage_root.join("sandboxes"),
            format!("{}-", self.config.network),
        );
        blocking(self.config.operation_timeout, move || {
            sweep(&directory, &prefix, &keep)
        })
        .await?;
        Ok(handles)
    }
}

/// The smallest ext4 metadata allowance for a workspace image: its inode tables, journal, bitmaps and descriptors take
/// about 53 MiB at 1 GiB, and the files written later need directory and extent blocks too.
const MIN_WORKSPACE_HEADROOM: u64 = 128 << 20;

/// A new workspace image's size for a quota of `quota` bytes: the quota plus ext4's metadata, 5% or at least
/// [`MIN_WORKSPACE_HEADROOM`], so the space the guest can use covers the quota (SBX-F-025, measured: a 1 GiB quota gets
/// 1152 MiB). Existing images keep their size.
///
/// # Errors
/// A quota whose image would be past the byte bound.
pub(crate) fn workspace_image_size(quota: ByteSize) -> Result<ByteSize, SandboxError> {
    let bytes = quota.bytes();
    let headroom = (bytes / 20).max(MIN_WORKSPACE_HEADROOM);
    bytes
        .checked_add(headroom)
        .and_then(|total| ByteSize::new(total).ok())
        .ok_or_else(|| {
            SandboxError::invalid("disk", "the workspace quota is too large for an image")
        })
}

fn handle(container: Container) -> Result<SandboxHandle, SandboxError> {
    let invalid = || SandboxError::Backend("invalid sandbox labels in container listing".into());
    let conversation_id = container
        .labels
        .get("tqa.conversation")
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let seconds: u64 = container
        .labels
        .get("tqa.created_at")
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let created_at = SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(seconds))
        .ok_or_else(invalid)?;
    if container.name.is_empty() || container.name.len() > 128 {
        return Err(invalid());
    }
    Ok(SandboxHandle {
        kind: BackendKind::Firecracker,
        id: container.name,
        conversation_id,
        created_at,
    })
}

/// Removes this instance's per-sandbox directories that nothing keeps; bounded like the container listing.
fn sweep(
    directory: &Path,
    prefix: &str,
    keep: &std::collections::HashSet<String>,
) -> Result<(), SandboxError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(storage(&error)),
    };
    for entry in entries.take(4096) {
        let name = entry.map_err(|error| storage(&error))?.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(prefix) && !keep.contains(name) {
            device::remove(&directory.join(name))?;
            tracing::info!("removed a stale per-sandbox directory");
        }
    }
    Ok(())
}

fn block_images(spec: &SandboxSpec) -> Result<(&Path, &Path), SandboxError> {
    let WorkspaceSource::BlockImage(workspace) = &spec.workspace.source else {
        return Err(SandboxError::invalid(
            "workspace",
            "the microVM backend needs a block image workspace",
        ));
    };
    let StateSource::BlockImage(state) = &spec.state.source else {
        return Err(SandboxError::invalid(
            "state",
            "the microVM backend needs a block image state",
        ));
    };
    Ok((workspace, state))
}

fn write_public(path: &Path, bytes: &[u8]) -> Result<(), SandboxError> {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(path)
        .map_err(|error| storage(&error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| storage(&error))
}

fn storage(error: &std::io::Error) -> SandboxError {
    SandboxError::Backend(format!(
        "per-sandbox file operation failed: {}",
        error.kind()
    ))
}

async fn blocking<T: Send + 'static>(
    bound: Duration,
    work: impl FnOnce() -> Result<T, SandboxError> + Send + 'static,
) -> Result<T, SandboxError> {
    tokio::time::timeout(bound, tokio::task::spawn_blocking(work))
        .await
        .map_err(|_| {
            // "Timed out" alone does not say which step: the bound tells a file step from the rootfs check, which
            // scales with the rootfs's size.
            tracing::warn!(
                bound_seconds = bound.as_secs(),
                "a Firecracker file operation timed out"
            );
            SandboxError::Timeout
        })?
        .map_err(|_| SandboxError::Backend("a file worker stopped".into()))?
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "backend_retry_tests.rs"]
mod retry_tests;

#[cfg(test)]
#[path = "backend_death_tests.rs"]
mod death_tests;

#[cfg(test)]
#[path = "backend_image_tests.rs"]
mod image_tests;

/// How many of a dead VM host's last output lines are read to explain its end.
const VM_HOST_LOG_LINES: usize = 80;
/// The most of its own lines a report keeps, and the most characters of each.
const REPORTED_LINES: usize = 20;
const REPORTED_CHARS: usize = 240;

/// A VM host that ended without the guest's exit record died itself (its launcher, the network setup, or Firecracker
/// before or while booting): what its exit status means, and its own last lines, the launcher's (`launch:`) and
/// Firecracker's (`[anonymous-instance:...]`), joined. The guest's console is left out: it is the guest's text, not the
/// host's.
pub(crate) fn vm_host_death(exit_code: Option<i64>, lines: &[String]) -> (String, String) {
    let meaning = exit_meaning(exit_code);
    let own: Vec<String> = lines
        .iter()
        .filter(|line| line.starts_with("launch:") || line.contains("[anonymous-instance:"))
        .map(|line| {
            line.chars()
                .filter(|character| !character.is_control())
                .take(REPORTED_CHARS)
                .collect()
        })
        .collect();
    let kept = own.len().saturating_sub(REPORTED_LINES);
    (meaning, own[kept..].join(" | "))
}

/// A VM host's exit status in words. Once the launcher has exec'd Firecracker, Firecracker is the container's process:
/// it catches its own faults and exits with its codes (148 to 157). A bare signal death (128 + the signal) had no
/// handler: the launcher, `ip`, `nft` or `setpriv`, or Firecracker before its handlers are in.
fn exit_meaning(exit_code: Option<i64>) -> String {
    let Some(code) = exit_code else {
        return "no exit status".into();
    };
    let firecracker = match code {
        148 => Some("a system call its seccomp filter forbids"),
        149 => Some("SIGBUS"),
        150 => Some("SIGSEGV"),
        151 => Some("SIGXFSZ"),
        152 => Some("a configuration it refused"),
        153 => Some("arguments it couldn't parse"),
        154 => Some("SIGXCPU"),
        156 => Some("SIGHUP"),
        157 => Some("SIGILL"),
        _ => None,
    };
    if let Some(what) = firecracker {
        return format!("Firecracker stopped on {what}");
    }
    match code {
        64 => "the launcher found a VM file missing".into(),
        137 => "killed by SIGKILL (out of memory, or removed)".into(),
        139 => "killed by SIGSEGV with no handler: before Firecracker's own handlers".into(),
        129..=192 => format!("killed by signal {}", code - 128),
        other => format!("exited with status {other}"),
    }
}
