//! Posture and boot to ready for a verified template (TK-I-002): a sandbox created from it through
//! the Docker backend as the gateway would, the posture observed from inside, and its runtime's dial of a link
//! stand-in, which records the protocol revision the runtime sends. Everything the check creates carries its own
//! prefix and is removed at the end, whatever happened.
use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use bollard::{
    Docker,
    exec::{CreateExecOptions, StartExecResults},
    models::{ContainerCreateBody, HostConfig, NetworkCreateRequest, VolumeCreateRequest},
    query_parameters::{CreateContainerOptions, LogsOptions, RemoveContainerOptions},
};
use futures::StreamExt;
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{
    ByteSize, CpuLimit, EgressMode, EgressPolicy, ImageRef, ReadinessSource, SandboxBackend,
    SandboxError, SandboxSpec, SkillsMounts, StateMount, WorkspaceMount, WorkspaceSource,
    docker::{DockerBackend, DockerBackendConfig, SubnetPool},
};
use tqa_types::ConversationId;

use crate::report::{Check, Status};

/// How long the runtime gets to dial the link stand-in (the stack's own start bound).
const BOOT_DEADLINE: Duration = Duration::from_mins(1);
/// Each engine call.
const ENGINE_TIMEOUT: Duration = Duration::from_secs(30);
/// The port the stand-in listens on, in the sandbox's view `gateway:8080`.
const STUB_PORT: u16 = 8080;
/// The /24 slices of the default sandbox pool tried in turn; one that overlaps a network in use is skipped.
const POOL_SLICES: std::ops::Range<u16> = 200..240;
/// The stand-in: accepts each dial, prints its peer and `TQA-Protocol-Revision`, answers 403 and closes.
const STUB: &str = r#"
import socket
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", 8080)); s.listen(8)
while True:
    c, a = s.accept(); c.settimeout(5); data = b""
    try:
        while b"\r\n\r\n" not in data and len(data) < 65536:
            d = c.recv(4096)
            if not d: break
            data += d
    except Exception:
        pass
    rev = ""
    for line in data.split(b"\r\n")[1:]:
        k, _, v = line.partition(b":")
        if k.strip().lower() == b"tqa-protocol-revision": rev = v.strip()[:8].decode("ascii", "replace")
    print("dial peer=%s revision=%s" % (a[0], rev), flush=True)
    try: c.sendall(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    except Exception: pass
    c.close()
"#;

/// Why the posture and boot checks could not run at all (the environment, not the template).
#[derive(Debug)]
pub struct Unavailable(pub String);

fn unavailable(what: &str) -> Unavailable {
    Unavailable(what.to_owned())
}

/// Creates a sandbox from `image` (a verified template's local image ID), checks its posture and waits for its
/// runtime to dial the stand-in, which runs from `stub_image` (the verified base's image). `sandbox_env` is the
/// install's sandbox environment. Removes everything it created before returning.
///
/// # Errors
/// The engine, the stand-in or the scratch resources could not be set up.
pub async fn posture_and_boot(
    image: &str,
    stub_image: &str,
    sandbox_env: &BTreeMap<String, String>,
) -> Result<Vec<Check>, Unavailable> {
    let docker = Docker::connect_with_defaults()
        .map_err(|_| unavailable("the container engine address is unusable"))?;
    let docker = tokio::time::timeout(ENGINE_TIMEOUT, docker.negotiate_version())
        .await
        .map_err(|_| unavailable("the container engine did not answer"))?
        .map_err(|_| unavailable("the container engine did not answer"))?;
    let scratch = Scratch::new(docker)?;
    let result = scratch.run(image, stub_image, sandbox_env).await;
    scratch.remove().await;
    result
}

/// The check's own resources, all named with one prefix.
struct Scratch {
    docker: Docker,
    prefix: String,
    storage: tempfile::TempDir,
    state: tempfile::TempDir,
}

impl Scratch {
    fn new(docker: Docker) -> Result<Self, Unavailable> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let scratch_dir = || tempfile::tempdir().map_err(|_| unavailable("no scratch directory"));
        Ok(Self {
            docker,
            prefix: format!("tqa-extcheck-{}-{nanos}", std::process::id()),
            storage: scratch_dir()?,
            state: scratch_dir()?,
        })
    }

    fn stub(&self) -> String {
        format!("{}-gateway", self.prefix)
    }

    fn control(&self) -> String {
        format!("{}-control", self.prefix)
    }

    async fn run(
        &self,
        image: &str,
        stub_image: &str,
        sandbox_env: &BTreeMap<String, String>,
    ) -> Result<Vec<Check>, Unavailable> {
        self.start_stub(stub_image).await?;
        self.prepare_mounts(stub_image).await?;
        let backend = self.backend().await?;
        let spec = self.spec(image, sandbox_env)?;
        let mut checks = Vec::new();
        let handle = match backend.create(spec, CancellationToken::new()).await {
            Ok(handle) => handle,
            // The host can't prepare a sandbox at all: the environment's failure, not the template's.
            Err(SandboxError::Preflight(reason)) => {
                return Err(Unavailable(format!(
                    "the Docker backend's preflight failed: {}; on a rootless Docker host run through \
                     tools/rootless-test.sh",
                    bounded_text(reason)
                )));
            }
            Err(error) => {
                checks.push(Check::new(
                    &error.code().replace('.', "_"),
                    Status::Fail,
                    &format!(
                        "the Docker backend refused to create a sandbox from the image: {}",
                        bounded_text(error.to_string())
                    ),
                ));
                return Ok(checks);
            }
        };
        checks.push(self.posture(&handle.id).await);
        let dialled = Dialled {
            docker: self.docker.clone(),
            stub: self.stub(),
            revision: Mutex::new(None),
        };
        let ready = backend
            .wait_ready(&handle, &dialled, BOOT_DEADLINE, CancellationToken::new())
            .await;
        checks.push(boot_check(ready, &dialled));
        let _ = backend.destroy(&handle).await;
        Ok(checks)
    }

    /// The link stand-in on a control network of its own; the backend attaches it to each sandbox's network.
    async fn start_stub(&self, stub_image: &str) -> Result<(), Unavailable> {
        let labels = HashMap::from([("tqa.instance".to_owned(), self.prefix.clone())]);
        bounded(self.docker.create_network(NetworkCreateRequest {
            name: self.control(),
            driver: Some("bridge".into()),
            internal: Some(true),
            labels: Some(labels.clone()),
            ..Default::default()
        }))
        .await
        .ok_or_else(|| unavailable("the stand-in's network could not be created"))?;
        engine(self.docker.create_container(
            Some(CreateContainerOptions {
                name: Some(self.stub()),
                ..Default::default()
            }),
            ContainerCreateBody {
                image: Some(stub_image.to_owned()),
                user: Some("1000:1000".into()),
                entrypoint: Some(vec![
                    "/usr/bin/python3".into(),
                    "-u".into(),
                    "-c".into(),
                    STUB.into(),
                ]),
                cmd: Some(Vec::new()),
                labels: Some(labels),
                host_config: Some(HostConfig {
                    network_mode: Some(self.control()),
                    readonly_rootfs: Some(true),
                    cap_drop: Some(vec!["ALL".into()]),
                    security_opt: Some(vec!["no-new-privileges".into()]),
                    memory: Some(64 * 1024 * 1024),
                    pids_limit: Some(16),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| {
            Unavailable(format!(
                "the stand-in could not be created from the base image: {error}"
            ))
        })?;
        bounded(self.docker.start_container(&self.stub(), None))
            .await
            .ok_or_else(|| {
                unavailable("the stand-in could not start (the base image needs /usr/bin/python3)")
            })
    }

    /// The workspace volume and a state directory the runtime (uid 1000) owns, as the gateway prepares them.
    async fn prepare_mounts(&self, helper_image: &str) -> Result<(), Unavailable> {
        bounded(self.docker.create_volume(VolumeCreateRequest {
            name: Some(self.prefix.clone()),
            labels: Some(HashMap::from([(
                "tqa.instance".to_owned(),
                self.prefix.clone(),
            )])),
            ..Default::default()
        }))
        .await
        .ok_or_else(|| unavailable("the workspace volume could not be created"))?;
        self.open_workspace(helper_image).await?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = self.state.path().to_owned();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o777))
                .map_err(|_| unavailable("the state directory could not be prepared"))?;
            let _ = std::os::unix::fs::chown(&path, Some(1000), None);
        }
        Ok(())
    }

    /// Makes the new volume writable for the sandbox user, as the gateway's workspace initialiser leaves it: one short
    /// root container from the trusted base image, without network or capabilities, runs `chmod 0777 /workspace`.
    async fn open_workspace(&self, helper_image: &str) -> Result<(), Unavailable> {
        let name = format!("{}-provision", self.prefix);
        let failed = || unavailable("the workspace volume could not be prepared");
        bounded(self.docker.create_container(
            Some(CreateContainerOptions {
                name: Some(name.clone()),
                ..Default::default()
            }),
            ContainerCreateBody {
                image: Some(helper_image.to_owned()),
                user: Some("0:0".into()),
                entrypoint: Some(vec![
                    "/bin/chmod".into(),
                    "0777".into(),
                    "/workspace".into(),
                ]),
                cmd: Some(Vec::new()),
                labels: Some(HashMap::from([(
                    "tqa.instance".to_owned(),
                    self.prefix.clone(),
                )])),
                host_config: Some(HostConfig {
                    network_mode: Some("none".into()),
                    readonly_rootfs: Some(true),
                    cap_drop: Some(vec!["ALL".into()]),
                    security_opt: Some(vec!["no-new-privileges".into()]),
                    memory: Some(32 * 1024 * 1024),
                    pids_limit: Some(16),
                    mounts: Some(vec![bollard::models::Mount {
                        typ: Some(bollard::models::MountType::VOLUME),
                        source: Some(self.prefix.clone()),
                        target: Some("/workspace".into()),
                        // Without copying the image's /workspace in, the new directory stays the container root's,
                        // which may then open it without any capability.
                        volume_options: Some(bollard::models::MountVolumeOptions {
                            no_copy: Some(true),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ))
        .await
        .ok_or_else(failed)?;
        let started = bounded(self.docker.start_container(&name, None)).await;
        let exit = match started {
            Some(()) => {
                let mut wait = self.docker.wait_container(&name, None);
                tokio::time::timeout(ENGINE_TIMEOUT, wait.next())
                    .await
                    .ok()
                    .flatten()
                    .and_then(Result::ok)
                    .map(|response| response.status_code)
            }
            None => None,
        };
        let _ = bounded(self.docker.remove_container(
            &name,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        ))
        .await;
        (exit == Some(0)).then_some(()).ok_or_else(failed)
    }

    /// The Docker backend with a free slice of the sandbox pool.
    async fn backend(&self) -> Result<DockerBackend, Unavailable> {
        for slice in POOL_SLICES {
            let mut config = DockerBackendConfig::new(self.prefix.clone(), self.storage.path())
                .map_err(|_| unavailable("the backend configuration was refused"))?;
            let pool = SubnetPool::new(&format!("10.213.{slice}.0/24"), 29)
                .map_err(|_| unavailable("the sandbox pool was refused"))?;
            config
                .set_sandbox_network(pool, Some(self.stub()))
                .map_err(|_| unavailable("the sandbox network was refused"))?;
            match DockerBackend::connect(config).await {
                Ok(backend) => return Ok(backend),
                Err(SandboxError::Preflight(reason)) if reason.contains("overlaps") => {}
                Err(_) => return Err(unavailable("the Docker backend could not connect")),
            }
        }
        Err(unavailable("no free sandbox network slice"))
    }

    fn spec(
        &self,
        image: &str,
        sandbox_env: &BTreeMap<String, String>,
    ) -> Result<SandboxSpec, Unavailable> {
        let invalid = |_| unavailable("the sandbox specification was refused");
        Ok(SandboxSpec {
            conversation_id: "conversation_extcheck"
                .parse()
                .map_err(|_| unavailable("id"))?,
            workspace: WorkspaceMount {
                id: "workspace_extcheck"
                    .parse()
                    .map_err(|_| unavailable("id"))?,
                source: WorkspaceSource::NamedVolume(self.prefix.clone()),
            },
            state: StateMount::directory(self.state.path()),
            image: ImageRef::local_id(image).map_err(invalid)?,
            cpu: CpuLimit::new(1000).map_err(invalid)?,
            memory: ByteSize::new(512 * 1024 * 1024).map_err(invalid)?,
            disk: None,
            pids: 256,
            egress: EgressPolicy {
                mode: EgressMode::Internal,
                allow: vec![],
                proxy_url: None,
            },
            gateway_url: format!("http://gateway:{STUB_PORT}/")
                .parse()
                .map_err(|_| unavailable("the stand-in URL"))?,
            // The stand-in never checks it; it only records the dial.
            runtime_token: SecretString::from(self.prefix.clone()),
            env: sandbox_env.clone(),
            skills: SkillsMounts::default(),
            desktop: false,
        })
    }

    /// The posture as seen from inside the sandbox, as its own user.
    async fn posture(&self, id: &str) -> Check {
        let script = "id -u; grep -E '^(CapEff|NoNewPrivs):' /proc/self/status; \
                      (touch /usr/.extcheck 2>/dev/null && echo usr=writable) || echo usr=readonly; \
                      (touch /tmp/.extcheck && echo tmp=writable) || echo tmp=readonly; \
                      (touch /workspace/.extcheck && echo workspace=writable) || echo workspace=readonly";
        let Some(output) = self.exec(id, script).await else {
            return Check::new(
                "posture_unreadable",
                Status::Fail,
                "the posture could not be read inside the sandbox",
            );
        };
        posture_check(&output)
    }

    async fn exec(&self, id: &str, script: &str) -> Option<String> {
        let exec = bounded(self.docker.create_exec(
            id,
            CreateExecOptions {
                cmd: Some(vec!["/bin/sh", "-c", script]),
                attach_stdout: Some(true),
                attach_stderr: Some(false),
                ..Default::default()
            },
        ))
        .await?;
        let StartExecResults::Attached { mut output, .. } =
            bounded(self.docker.start_exec(&exec.id, None)).await?
        else {
            return None;
        };
        let mut text = String::new();
        while let Ok(Some(Ok(chunk))) = tokio::time::timeout(ENGINE_TIMEOUT, output.next()).await {
            text.push_str(&chunk.to_string());
        }
        Some(text)
    }

    /// Removes the stand-in, its network and the workspace volume; the backend removed the sandbox and its network.
    async fn remove(&self) {
        let _ = bounded(self.docker.remove_container(
            &self.stub(),
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        ))
        .await;
        let _ = bounded(self.docker.remove_network(&self.control())).await;
        let _ = bounded(self.docker.remove_volume(
            &self.prefix,
            None::<bollard::query_parameters::RemoveVolumeOptions>,
        ))
        .await;
    }
}

/// The posture lines, read into one check: the first violation found, or a pass.
fn posture_check(output: &str) -> Check {
    let value = |key: &str| {
        output
            .lines()
            .find_map(|line| line.strip_prefix(key).map(str::trim))
            .unwrap_or_default()
            .to_owned()
    };
    let uid = output.lines().next().unwrap_or_default().trim();
    let capabilities = value("CapEff:");
    let failures = [
        (
            uid.is_empty() || uid == "0",
            "posture_root_user",
            "the runtime runs as root",
        ),
        (
            value("usr=") != "readonly",
            "posture_root_writable",
            "the root filesystem is writable",
        ),
        (
            value("tmp=") != "writable",
            "posture_tmp_readonly",
            "/tmp is not writable",
        ),
        (
            value("workspace=") != "writable",
            "posture_workspace_readonly",
            "/workspace is not writable",
        ),
        (
            capabilities.is_empty() || !capabilities.trim_start_matches('0').is_empty(),
            "posture_capabilities",
            "the runtime has effective capabilities (or they could not be read)",
        ),
        (
            value("NoNewPrivs:") != "1",
            "posture_new_privileges",
            "no_new_privs is not set",
        ),
    ];
    failures
        .iter()
        .find(|(failed, ..)| *failed)
        .map_or_else(
            || {
                Check::new(
                    "posture",
                    Status::Pass,
                    "non-root, read-only root, writable /tmp and /workspace, no capabilities, no_new_privs",
                )
            },
            |(_, code, message)| Check::new(code, Status::Fail, message),
        )
}

fn boot_check(ready: Result<(), SandboxError>, dialled: &Dialled) -> Check {
    let revision = dialled.revision.lock().ok().and_then(|revision| *revision);
    match (ready, revision) {
        (Ok(()), Some(revision)) if tqa_types::check_runtime_revision(revision).is_ok() => {
            Check::new(
                "boot_ready",
                Status::Pass,
                &format!("the runtime dialled the gateway with protocol revision {revision}"),
            )
        }
        (Ok(()), Some(revision)) => Check::new(
            "runtime_too_old",
            Status::Fail,
            &format!(
                "the runtime dialled with protocol revision {revision}, below the gateway's floor"
            ),
        ),
        (Ok(()), None) => Check::new(
            "boot_no_revision",
            Status::Fail,
            "the runtime dialled without a readable protocol revision",
        ),
        (Err(SandboxError::RuntimeExited { .. }), _) => Check::new(
            "boot_runtime_exited",
            Status::Fail,
            "the runtime exited before dialling the gateway",
        ),
        (Err(_), _) => Check::new(
            "boot_timeout",
            Status::Fail,
            "the runtime did not dial the gateway within 60 s",
        ),
    }
}

/// Ready once the stand-in has logged a dial; records the revision it carried.
struct Dialled {
    docker: Docker,
    stub: String,
    revision: Mutex<Option<u16>>,
}

#[async_trait]
impl ReadinessSource for Dialled {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        let mut logs = self.docker.logs(
            &self.stub,
            Some(LogsOptions {
                stdout: true,
                ..Default::default()
            }),
        );
        let mut text = String::new();
        while let Ok(Some(Ok(chunk))) = tokio::time::timeout(ENGINE_TIMEOUT, logs.next()).await {
            text.push_str(&chunk.to_string());
        }
        let Some(line) = text.lines().find(|line| line.starts_with("dial ")) else {
            return false;
        };
        let revision = line
            .split_once("revision=")
            .and_then(|(_, revision)| revision.trim().parse().ok());
        if let Ok(mut recorded) = self.revision.lock() {
            *recorded = revision;
        }
        true
    }
}

/// A diagnostic, at most 300 characters and without control characters.
fn bounded_text(text: impl AsRef<str>) -> String {
    let text = text.as_ref();
    text.chars()
        .filter(|character| !character.is_control())
        .take(300)
        .collect()
}

/// One engine call within its bound, with the engine's reason when it fails.
async fn engine<T>(
    future: impl std::future::Future<Output = Result<T, bollard::errors::Error>>,
) -> Result<T, String> {
    match tokio::time::timeout(ENGINE_TIMEOUT, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(bounded_text(error.to_string())),
        Err(_) => Err("the engine did not answer in time".into()),
    }
}

/// One engine call within its bound; `None` when it failed or timed out.
async fn bounded<T>(
    future: impl std::future::Future<Output = Result<T, bollard::errors::Error>>,
) -> Option<T> {
    tokio::time::timeout(ENGINE_TIMEOUT, future)
        .await
        .ok()?
        .ok()
}
