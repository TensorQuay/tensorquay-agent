use std::{
    collections::HashMap,
    error::Error,
    future::Future,
    panic::AssertUnwindSafe,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use bollard::{
    Docker,
    exec::{CreateExecOptions, StartExecResults},
    models::{ContainerCreateBody, HostConfig, Mount, MountType, VolumeCreateRequest},
    query_parameters::{
        CreateContainerOptions, ListContainersOptions, ListVolumesOptions, RemoveContainerOptions,
    },
};
use futures::{FutureExt, StreamExt, future::LocalBoxFuture};
use tempfile::TempDir;
use tokio::time::timeout;
use tqa_sandbox::{
    ImageRef, ReadinessSource, SandboxSpec, WorkspaceSource,
    docker::{DockerBackend, DockerBackendConfig},
};
use tqa_types::ConversationId;

#[path = "spec.rs"]
pub mod specifications;

pub type TestResult = Result<(), Box<dyn Error>>;
pub const IMAGE: &str = "tianon/toybox";
pub const DIGEST: &str = "sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7";
static NEXT: AtomicU64 = AtomicU64::new(0);
/// Pool slices a fixture tries before giving up.
const MAX_POOL_ATTEMPTS: u64 = 16;

pub struct Fixture {
    pub backend: DockerBackend,
    pub docker: Docker,
    pub prefix: String,
    /// This fixture's slice of the sandbox network pool.
    pub pool: tqa_sandbox::docker::SubnetPool,
    pub workspace: TempDir,
    pub storage: TempDir,
    pub state: TempDir,
}

#[allow(
    dead_code,
    reason = "Shared fixture is compiled separately by each integration test binary."
)]
pub struct Ready;

#[async_trait]
impl ReadinessSource for Ready {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        true
    }
}

#[allow(clippy::print_stderr)]
pub async fn run(test: impl for<'a> AsyncFnOnce(&'a Fixture) -> TestResult) -> TestResult {
    if std::env::var("TQA_DOCKER_TESTS").as_deref() != Ok("1") {
        return Err(
            "Docker integration requires TQA_DOCKER_TESTS=1; run with --run-ignored all and the flag set"
                .into(),
        );
    }
    let fixture = Fixture::new().await?;
    let result = AssertUnwindSafe(test(&fixture)).catch_unwind().await;
    let cleanup = fixture.cleanup().await;
    match result {
        Ok(result) => {
            cleanup?;
            result
        }
        Err(payload) => {
            if let Err(error) = cleanup {
                eprintln!("Test cleanup failed: {error}");
            }
            std::panic::resume_unwind(payload)
        }
    }
}

pub async fn bounded<T>(
    future: impl Future<Output = Result<T, bollard::errors::Error>>,
) -> Result<T, Box<dyn Error>> {
    Ok(timeout(Duration::from_secs(30), future).await??)
}

impl Fixture {
    async fn new() -> Result<Self, Box<dyn Error>> {
        let counter = NEXT.fetch_add(1, Ordering::Relaxed);
        let prefix = format!(
            "tqa-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_micros(),
            counter
        );
        let storage = tempfile::tempdir()?;
        let docker = bounded(Docker::connect_with_defaults()?.negotiate_version()).await?;
        let gateway = format!("{prefix}-gateway");
        let (backend, pool) = match connect(&docker, &prefix, &gateway, &storage, counter).await {
            Ok(connected) => connected,
            Err(error) => {
                return Err(setup_failed(
                    error,
                    cleanup_resources(&docker, &prefix, storage.path()).await,
                ));
            }
        };
        let fixture = Self {
            backend,
            docker,
            prefix,
            pool,
            workspace: tempfile::tempdir()?,
            storage,
            state: tempfile::tempdir()?,
        };
        if let Err(error) = fixture.provision().await {
            return Err(setup_failed(error, fixture.cleanup().await));
        }
        Ok(fixture)
    }

    async fn provision(&self) -> TestResult {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            timeout(
                Duration::from_secs(5),
                tokio::fs::set_permissions(
                    self.workspace.path(),
                    std::fs::Permissions::from_mode(0o777),
                ),
            )
            .await??;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            timeout(
                Duration::from_secs(5),
                tokio::fs::set_permissions(
                    self.state.path(),
                    std::fs::Permissions::from_mode(0o777),
                ),
            )
            .await??;
        }
        // The runtime (uid 1000) makes its state directory private and so must own it, as it does when the
        // gateway creates it. Desktop file sharing hides this; a native Linux daemon enforces it.
        #[cfg(target_os = "linux")]
        {
            let path = self.state.path().to_owned();
            tokio::task::spawn_blocking(move || std::os::unix::fs::chown(path, Some(1000), None))
                .await?
                .map_err(|_| {
                    "the runtime state directory must be owned by uid 1000; run as root, or through \
                     tools/rootless-test.sh on a rootless daemon"
                })?;
        }
        self.provision_volume().await
    }

    async fn provision_volume(&self) -> TestResult {
        bounded(self.docker.create_volume(VolumeCreateRequest {
            name: Some(self.prefix.clone()),
            ..Default::default()
        }))
        .await?;
        let response = bounded(self.docker.create_container(
            Some(CreateContainerOptions {
                name: Some(format!("{}-provision", self.prefix)),
                ..Default::default()
            }),
            ContainerCreateBody {
                image: Some(format!("{IMAGE}@{DIGEST}")),
                user: Some("0:0".into()),
                cmd: Some(vec!["chmod".into(), "0777".into(), "/workspace".into()]),
                labels: Some(HashMap::from([(
                    "tqa.instance".into(),
                    self.prefix.clone(),
                )])),
                host_config: Some(HostConfig {
                    network_mode: Some("none".into()),
                    readonly_rootfs: Some(true),
                    cap_drop: Some(vec!["ALL".into()]),
                    security_opt: Some(vec!["no-new-privileges".into(), "seccomp=builtin".into()]),
                    memory: Some(32 * 1024 * 1024),
                    memory_swap: Some(32 * 1024 * 1024),
                    pids_limit: Some(16),
                    mounts: Some(vec![Mount {
                        typ: Some(MountType::VOLUME),
                        source: Some(self.prefix.clone()),
                        target: Some("/workspace".into()),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ))
        .await?;
        bounded(self.docker.start_container(&response.id, None)).await?;
        let mut wait = self.docker.wait_container(&response.id, None);
        let exit = timeout(Duration::from_secs(10), wait.next())
            .await?
            .ok_or("missing provision exit")??;
        assert_eq!(exit.status_code, 0);
        bounded(self.docker.remove_container(
            &response.id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        ))
        .await?;
        Ok(())
    }

    pub fn spec(&self, suffix: &str, bind: bool) -> Result<SandboxSpec, Box<dyn Error>> {
        let mut spec = specifications::spec()?;
        spec.conversation_id = format!("conversation_{suffix}").parse()?;
        spec.image = ImageRef::new(IMAGE, DIGEST)?;
        spec.state = tqa_sandbox::StateMount::directory(self.state.path());
        spec.workspace.source = if bind {
            WorkspaceSource::HostDirectory(self.workspace.path().to_path_buf())
        } else {
            WorkspaceSource::NamedVolume(self.prefix.clone())
        };
        for (key, value) in [
            (
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            ),
            ("HOME", "/workspace"),
            ("HOSTNAME", "sandbox-fixture"),
        ] {
            spec.env.insert(key.into(), value.into());
        }
        Ok(spec)
    }

    #[allow(
        dead_code,
        reason = "Some integration binaries need lifecycle operations only."
    )]
    pub async fn exec(
        &self,
        id: &str,
        arguments: &[&str],
    ) -> Result<(i64, String), Box<dyn Error>> {
        let created = bounded(self.docker.create_exec(
            id,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(arguments.to_vec()),
                ..Default::default()
            },
        ))
        .await?;
        let result = bounded(self.docker.start_exec(&created.id, None)).await?;
        let StartExecResults::Attached { mut output, .. } = result else {
            return Err("exec was not attached".into());
        };
        let mut text = String::new();
        timeout(Duration::from_secs(20), async {
            while let Some(chunk) = output.next().await {
                text.push_str(&chunk?.to_string());
                check_output_bound(&text)?;
            }
            Ok::<_, Box<dyn Error>>(())
        })
        .await??;
        let exit = bounded(self.docker.inspect_exec(&created.id))
            .await?
            .exit_code
            .ok_or("missing exec exit")?;
        Ok((exit, text))
    }

    /// What a failed or finished sandbox could leave behind (SBX-C-010): a container the backend made, in any state
    /// (it would hold processes and anonymous volumes), a volume named for this fixture other than its workspace
    /// volume, a missing workspace volume, and per-sandbox token directories. Empty means nothing is left.
    #[allow(
        dead_code,
        reason = "Some integration binaries need lifecycle operations only."
    )]
    pub async fn leftovers(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut found = Vec::new();
        let containers = bounded(self.docker.list_containers(Some(ListContainersOptions {
            all: true,
            filters: Some(HashMap::from([(
                "label".into(),
                vec![
                    format!("tqa.instance={}", self.prefix),
                    "tqa.token_id".into(),
                ],
            )])),
            ..Default::default()
        })))
        .await?;
        found.extend(
            containers.into_iter().map(|container| {
                format!("container {:?} ({:?})", container.names, container.state)
            }),
        );
        let volumes = bounded(self.docker.list_volumes(Some(ListVolumesOptions {
            filters: Some(HashMap::from([("name".into(), vec![self.prefix.clone()])])),
        })))
        .await?
        .volumes
        .unwrap_or_default();
        if !volumes.iter().any(|volume| volume.name == self.prefix) {
            found.push("the workspace volume was removed".into());
        }
        found.extend(
            volumes
                .into_iter()
                .filter(|volume| volume.name != self.prefix)
                .map(|volume| format!("volume {}", volume.name)),
        );
        let networks = bounded(self.docker.list_networks(Some(
            bollard::query_parameters::ListNetworksOptions {
                filters: Some(HashMap::from([(
                    "label".into(),
                    vec![format!("tqa.instance={}", self.prefix)],
                )])),
            },
        )))
        .await?;
        let control = format!("{}-control", self.prefix);
        found.extend(
            networks
                .into_iter()
                .filter_map(|network| network.name)
                .filter(|name| *name != control)
                .map(|name| format!("network {name}")),
        );
        let tokens = self.storage.path().join("sandboxes");
        if tokens.exists() {
            for entry in std::fs::read_dir(&tokens)? {
                found.push(format!("token directory {}", entry?.file_name().display()));
            }
        }
        Ok(found)
    }

    async fn cleanup(&self) -> TestResult {
        cleanup_resources(&self.docker, &self.prefix, self.storage.path()).await
    }

    /// A backend configuration for this fixture's daemon slice: its prefix, storage, pool and gateway stand-in, for
    /// tests that connect a backend of their own.
    #[allow(
        dead_code,
        reason = "Some integration binaries need lifecycle operations only."
    )]
    pub fn backend_config(&self) -> Result<DockerBackendConfig, Box<dyn Error>> {
        let mut config = DockerBackendConfig::new(self.prefix.clone(), self.storage.path())?;
        config.set_sandbox_network(self.pool, Some(format!("{}-gateway", self.prefix)))?;
        Ok(config)
    }

    /// The network a sandbox was attached to: its own, named on the container (ADR-0021 amendment).
    #[allow(
        dead_code,
        reason = "Some integration binaries need lifecycle operations only."
    )]
    pub async fn sandbox_network(&self, id: &str) -> Result<String, Box<dyn Error>> {
        Ok(bounded(self.docker.inspect_container(id, None))
            .await?
            .config
            .and_then(|config| config.labels)
            .and_then(|labels| labels.get("tqa.network").cloned())
            .ok_or("the sandbox names no network")?)
    }

    /// A sandbox's address on its own network.
    #[allow(
        dead_code,
        reason = "Some integration binaries need lifecycle operations only."
    )]
    pub async fn sandbox_address(&self, id: &str) -> Result<String, Box<dyn Error>> {
        let network = self.sandbox_network(id).await?;
        Ok(bounded(self.docker.inspect_container(id, None))
            .await?
            .network_settings
            .and_then(|settings| settings.networks)
            .and_then(|mut networks| networks.remove(&network))
            .and_then(|network| network.ip_address)
            .filter(|address| !address.is_empty())
            .ok_or("the sandbox has no address on its network")?)
    }
}

/// Starts the gateway stand-in on a control network of its own (a container with no network cannot join others),
/// then connects the backend with a pool slice of its own, so parallel test processes don't compete for subnets.
async fn connect(
    docker: &Docker,
    prefix: &str,
    gateway: &str,
    storage: &TempDir,
    counter: u64,
) -> Result<(DockerBackend, tqa_sandbox::docker::SubnetPool), Box<dyn Error>> {
    pull(docker).await?;
    let control = format!("{prefix}-control");
    bounded(
        docker.create_network(bollard::models::NetworkCreateRequest {
            name: control.clone(),
            driver: Some("bridge".into()),
            internal: Some(true),
            labels: Some(HashMap::from([("tqa.instance".into(), prefix.to_owned())])),
            ..Default::default()
        }),
    )
    .await?;
    let created = bounded(docker.create_container(
        Some(CreateContainerOptions {
            name: Some(gateway.to_owned()),
            ..Default::default()
        }),
        ContainerCreateBody {
            image: Some(format!("{IMAGE}@{DIGEST}")),
            user: Some("1000:1000".into()),
            cmd: Some(vec!["sh".into(), "-c".into(), "nc -L -p 8080 cat".into()]),
            labels: Some(HashMap::from([("tqa.instance".into(), prefix.to_owned())])),
            host_config: Some(HostConfig {
                network_mode: Some(control),
                readonly_rootfs: Some(true),
                cap_drop: Some(vec!["ALL".into()]),
                security_opt: Some(vec!["no-new-privileges".into(), "seccomp=builtin".into()]),
                memory: Some(32 * 1024 * 1024),
                memory_swap: Some(32 * 1024 * 1024),
                pids_limit: Some(16),
                ..Default::default()
            }),
            ..Default::default()
        },
    ))
    .await?;
    bounded(docker.start_container(&created.id, None)).await?;
    // Parallel test processes each need a pool slice no other one uses: start from a spread-out slice and move on
    // while the backend's start-up check finds it taken.
    let start = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .subsec_nanos();
    for attempt in 0..MAX_POOL_ATTEMPTS {
        let slice =
            (u64::from(start) + u64::from(std::process::id()) * 31 + counter * 7 + attempt * 37)
                % 256;
        let mut config = DockerBackendConfig::new(prefix.to_owned(), storage.path())?;
        config.test_command_override = Some(vec![
            "sh".into(),
            "-c".into(),
            "trap 'exit 0' TERM; sleep 600 & wait".into(),
        ]);
        let pool = tqa_sandbox::docker::SubnetPool::new(&format!("10.213.{slice}.0/24"), 29)?;
        config.set_sandbox_network(pool, Some(gateway.to_owned()))?;
        match DockerBackend::connect(config).await {
            Ok(backend) => return Ok((backend, pool)),
            Err(tqa_sandbox::SandboxError::Preflight(reason)) if reason.contains("overlaps") => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err("no free sandbox network pool slice for this test".into())
}

async fn pull(docker: &Docker) -> TestResult {
    let mut pull = docker.create_image(
        Some(bollard::query_parameters::CreateImageOptions {
            from_image: Some(format!("{IMAGE}@{DIGEST}")),
            ..Default::default()
        }),
        None,
        None,
    );
    timeout(Duration::from_mins(2), async {
        while let Some(item) = pull.next().await {
            item?;
        }
        Ok::<_, bollard::errors::Error>(())
    })
    .await??;
    Ok(())
}

/// Removes every container carrying this fixture's instance label, with its anonymous volumes.
async fn remove_containers(docker: &Docker, prefix: &str) -> TestResult {
    let label = HashMap::from([("label".into(), vec![format!("tqa.instance={prefix}")])]);
    let containers = bounded(docker.list_containers(Some(ListContainersOptions {
        all: true,
        filters: Some(label),
        ..Default::default()
    })))
    .await?;
    for container in containers {
        if let Some(id) = container.id {
            bounded(docker.remove_container(
                &id,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            ))
            .await?;
        }
    }
    Ok(())
}

/// Removes everything this fixture made, on an engine that may be slow to answer: every step runs even after an
/// earlier one failed, and each is tried again while it times out (SBX-U-013). Candidate 13g's overloaded coverage run
/// showed why: a setup that timed out was followed by a cleanup whose first call timed out too, and ending the cleanup
/// there left 22 gateway stand-ins behind. The last step sweeps the containers once more, for a create that timed out
/// on the client but completed in the engine after the first sweep.
async fn cleanup_resources(docker: &Docker, prefix: &str, storage: &std::path::Path) -> TestResult {
    every_step(
        CLEANUP_ATTEMPTS,
        CLEANUP_PAUSE,
        vec![
            step("containers", || remove_containers(docker, prefix)),
            // A late container on a network holds it, so each "in use" refusal sweeps the containers again.
            step("networks", || remove_networks(docker, prefix)),
            // A late container also holds the volume, likewise.
            step("volume", || {
                retry_in_use(
                    VOLUME_ATTEMPTS,
                    VOLUME_PAUSE,
                    || remove_containers(docker, prefix),
                    || async {
                        bounded(docker.remove_volume(
                            prefix,
                            None::<bollard::query_parameters::RemoveVolumeOptions>,
                        ))
                        .await
                    },
                )
            }),
            step("token directories", || async move {
                match timeout(
                    Duration::from_secs(5),
                    tokio::fs::remove_dir_all(storage.join("sandboxes")),
                )
                .await?
                {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
                    _ => Ok(()),
                }
            }),
            step("image", || async {
                ignore_missing(
                    bounded(docker.remove_image(
                        &format!("{IMAGE}:{prefix}"),
                        None::<bollard::query_parameters::RemoveImageOptions>,
                        None,
                    ))
                    .await
                    .map(|_| ()),
                )
            }),
            step("late containers", || remove_containers(docker, prefix)),
        ],
    )
    .await
}

/// Removes every network carrying this fixture's instance label; one still in use sweeps the containers and is tried
/// again (a late container joined it).
async fn remove_networks(docker: &Docker, prefix: &str) -> TestResult {
    let label = HashMap::from([("label".into(), vec![format!("tqa.instance={prefix}")])]);
    let networks = bounded(docker.list_networks(Some(
        bollard::query_parameters::ListNetworksOptions {
            filters: Some(label),
        },
    )))
    .await?;
    for name in networks.into_iter().filter_map(|network| network.name) {
        retry_in_use(
            VOLUME_ATTEMPTS,
            VOLUME_PAUSE,
            || remove_containers(docker, prefix),
            || async { bounded(docker.remove_network(&name)).await },
        )
        .await?;
    }
    Ok(())
}

/// How often one cleanup step is tried while the engine doesn't answer within its bound, and the pause between tries:
/// on a loaded host the engine was seen to start containers up to about 30 s late (docs/evals/gate-6.md).
const CLEANUP_ATTEMPTS: usize = 3;
const CLEANUP_PAUSE: Duration = Duration::from_secs(5);

/// One named cleanup step: called again for each try.
pub type Step<'a> = (
    &'static str,
    Box<dyn FnMut() -> LocalBoxFuture<'a, TestResult> + 'a>,
);

/// A cleanup step from a closure that starts one try.
pub fn step<'a, F, Fut>(name: &'static str, mut start: F) -> Step<'a>
where
    F: FnMut() -> Fut + 'a,
    Fut: Future<Output = TestResult> + 'a,
{
    (name, Box::new(move || start().boxed_local()))
}

/// Runs every step in order, each through [`retry_elapsed`], even after an earlier one failed, so one stalled call
/// never leaves the rest behind; the failures are named together, in order.
pub async fn every_step(attempts: usize, pause: Duration, steps: Vec<Step<'_>>) -> TestResult {
    let mut failures = Vec::new();
    for (name, mut start) in steps {
        if let Err(error) = retry_elapsed(attempts, pause, &mut start).await {
            failures.push(format!("{name}: {error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; ").into())
    }
}

/// Runs `start`, again while its bound elapses (the engine didn't answer in time), `attempts` times in all; any other
/// outcome is returned at once.
pub async fn retry_elapsed<S, SF>(attempts: usize, pause: Duration, mut start: S) -> TestResult
where
    S: FnMut() -> SF,
    SF: Future<Output = TestResult>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match start().await {
            Err(error)
                if error
                    .downcast_ref::<tokio::time::error::Elapsed>()
                    .is_some() =>
            {
                if attempt >= attempts {
                    return Err(format!("{error} after {attempts} attempts").into());
                }
                tokio::time::sleep(pause).await;
            }
            outcome => return outcome,
        }
    }
}

fn check_output_bound(text: &str) -> TestResult {
    if text.len() > 65_536 {
        return Err("exec output exceeded test bound".into());
    }
    Ok(())
}
/// How often a fixture's volume removal is tried while a late container holds it, and the pause between tries.
const VOLUME_ATTEMPTS: usize = 10;
const VOLUME_PAUSE: Duration = Duration::from_secs(3);

/// The engine's HTTP status inside a test error, when it is an engine answer.
fn engine_status(error: &(dyn Error + 'static)) -> Option<u16> {
    match error.downcast_ref::<bollard::errors::Error>() {
        Some(bollard::errors::Error::DockerResponseServerError { status_code, .. }) => {
            Some(*status_code)
        }
        _ => None,
    }
}

/// Removes a volume with `remove`: gone already (404) counts as removed; while it is in use (409), `sweep` removes the
/// containers again and the removal is tried again, `attempts` times in all; any other error is returned at once.
pub async fn retry_in_use<C, CF, V, VF>(
    attempts: usize,
    pause: Duration,
    mut sweep: C,
    mut remove: V,
) -> TestResult
where
    C: FnMut() -> CF,
    CF: Future<Output = TestResult>,
    V: FnMut() -> VF,
    VF: Future<Output = TestResult>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        let Err(error) = remove().await else {
            return Ok(());
        };
        match engine_status(error.as_ref()) {
            Some(404) => return Ok(()),
            Some(409) if attempt < attempts => {
                tokio::time::sleep(pause).await;
                sweep().await?;
            }
            Some(409) => {
                return Err(format!(
                    "the volume is still in use after {attempts} attempts: {error}"
                )
                .into());
            }
            _ => return Err(error),
        }
    }
}

/// A failed setup's error, with the cleanup's own failure after it, so the first failure is never masked.
pub fn setup_failed(error: Box<dyn Error>, cleanup: TestResult) -> Box<dyn Error> {
    match cleanup {
        Ok(()) => error,
        Err(cleanup) => format!("{error}; cleanup also failed: {cleanup}").into(),
    }
}

fn ignore_missing(result: TestResult) -> TestResult {
    match result {
        Err(error)
            if error
                .downcast_ref::<bollard::errors::Error>()
                .is_some_and(|error| {
                    matches!(
                        error,
                        bollard::errors::Error::DockerResponseServerError {
                            status_code: 404,
                            ..
                        }
                    )
                }) =>
        {
            Ok(())
        }
        result => result,
    }
}
