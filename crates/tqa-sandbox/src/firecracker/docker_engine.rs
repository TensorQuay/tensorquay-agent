//! The [`Engine`] over the Docker Engine API (bollard): every request is bounded by the configured timeout, and a
//! missing object maps to `NotFound`. A failure's error names the call and the HTTP status; the engine's own message is
//! never passed on in it. The operator's warning also names the request's target and what came back: the engine's
//! reason, or the kind of failure when no answer came (ENGINEERING-GUARDRAILS §1.5, SBX-F-037). The reason goes through
//! the diagnostic redaction the gateway applies to other engine and upstream text (`tqa_types::redact`: credential values
//! and key-shaped tokens masked; §1.11, redaction on by default) before it is capped at 200 bytes.

use std::{
    collections::HashMap,
    future::Future,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use bollard::{
    Docker,
    models::{ContainerCreateBody, ContainerInspectResponse},
    query_parameters::{
        CreateContainerOptions, ListContainersOptions, LogsOptions, RemoveContainerOptions,
        StatsOptions, StopContainerOptions,
    },
};
use futures::StreamExt;

use super::engine::{BACKEND_LABEL, Container, Created, Engine, HELPER_LABEL, NetworkState, Usage};
use crate::SandboxError;

const MAX_CONTAINERS: usize = 4096;

/// An engine request, named in its error and warning: what was attempted (ENGINEERING-GUARDRAILS §1.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    InspectImage,
    InspectNetwork,
    ListNetworks,
    Create,
    Start,
    Inspect,
    Stop,
    Remove,
    List,
    Stats,
    Wait,
    Logs,
}

impl Call {
    fn doing(self) -> &'static str {
        match self {
            Self::InspectImage => "inspecting an image",
            Self::InspectNetwork => "inspecting a network",
            Self::ListNetworks => "listing networks",
            Self::Create => "creating a container",
            Self::Start => "starting a container",
            Self::Inspect => "inspecting a container",
            Self::Stop => "stopping a container",
            Self::Remove => "removing a container",
            Self::List => "listing containers",
            Self::Stats => "reading a container's usage",
            Self::Wait => "waiting for a container",
            Self::Logs => "reading a container's output",
        }
    }
}

/// The error for an engine request the engine refused or did not answer. A missing object is `NotFound`. A create
/// refused because the name is taken (409) is an earlier attempt's create that the engine finished after its caller
/// gave up: it is reported as a timeout, so the retry removes that container by name and creates again (SBX-F-033).
/// Any other failure's error names the call and the HTTP status, never the engine's message; the warning logged with it
/// names the target and the engine's answer too ([`answer`]).
/// Whether the engine refused a create because the name is held (409).
fn conflict(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 409,
            ..
        }
    )
}

/// What came back for the warning: the engine's own reason on one line, redacted and then capped, or the kind of
/// failure when the engine gave no answer (bollard's error variant, never its data).
fn answer(error: &bollard::errors::Error) -> String {
    const CAP: usize = 200;
    match error {
        bollard::errors::Error::DockerResponseServerError {
            status_code,
            message,
        } => {
            let line: String = message
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            // Redacted whole, before the cap, so a token is never shown in part.
            let line = tqa_types::redact(line.trim());
            let line = line.as_str();
            if line.len() <= CAP {
                return format!("HTTP {status_code}: {line}");
            }
            let mut end = CAP;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            format!("HTTP {status_code}: {}…", &line[..end])
        }
        bollard::errors::Error::RequestTimeoutError => "no answer in time".to_owned(),
        other => {
            let debug = format!("{other:?}");
            let kind = debug
                .split(|c: char| !c.is_alphanumeric())
                .next()
                .unwrap_or_default();
            format!("no answer from the engine ({kind})")
        }
    }
}

fn engine_error(call: Call, target: &str, error: &bollard::errors::Error) -> SandboxError {
    let found = match error {
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        } => return SandboxError::NotFound,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 409, ..
        } if call == Call::Create => {
            tracing::warn!(
                call = call.doing(),
                target,
                status = 409,
                "the container name is taken, by an earlier attempt's late create; it is removed before the next"
            );
            return SandboxError::Timeout;
        }
        bollard::errors::Error::DockerResponseServerError { status_code, .. } => {
            format!("the engine answered HTTP {status_code}")
        }
        _ => "no usable answer from the engine".to_owned(),
    };
    let detail = format!("{}: {found}", call.doing());
    // A failed stop is settled by the forced removal after it, and is not reported.
    if call != Call::Stop {
        tracing::warn!(
            call = call.doing(),
            target,
            answer = answer(error),
            "a Docker API request for the Firecracker backend failed: {detail}"
        );
    }
    SandboxError::Backend(detail)
}

/// The Docker-backed engine for one gateway instance's internal network.
pub(crate) struct DockerEngine {
    docker: Docker,
    network: String,
    timeout: Duration,
}

impl DockerEngine {
    pub(crate) fn new(docker: Docker, network: String, timeout: Duration) -> Self {
        Self {
            docker,
            network,
            timeout,
        }
    }

    async fn request<T>(
        &self,
        (call, target): (Call, &str),
        future: impl Future<Output = Result<T, bollard::errors::Error>>,
    ) -> Result<T, SandboxError> {
        self.request_within((call, target), self.timeout, future)
            .await
    }

    /// As [`Self::request`], bounded by `bound` (a retried call's share of its create budget, SBX-F-031).
    async fn request_within<T>(
        &self,
        (call, target): (Call, &str),
        bound: Duration,
        future: impl Future<Output = Result<T, bollard::errors::Error>>,
    ) -> Result<T, SandboxError> {
        self.request_answered((call, target), bound, future)
            .await?
            .map_err(|error| engine_error(call, target, &error))
    }

    /// As [`Self::request_within`], with the engine's own answer when it answered in time (a create tells a held name
    /// apart, SBX-F-035).
    async fn request_answered<T>(
        &self,
        (call, target): (Call, &str),
        bound: Duration,
        future: impl Future<Output = Result<T, bollard::errors::Error>>,
    ) -> Result<Result<T, bollard::errors::Error>, SandboxError> {
        tokio::time::timeout(bound, future).await.map_err(|_| {
            tracing::warn!(
                call = call.doing(),
                target,
                bound_seconds = bound.as_secs(),
                "a Docker API request for the Firecracker backend timed out"
            );
            SandboxError::Timeout
        })
    }

    fn container(&self, inspected: ContainerInspectResponse) -> Container {
        let state = inspected.state.unwrap_or_default();
        let running = state.running.unwrap_or(false);
        Container {
            name: inspected
                .name
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_owned(),
            labels: inspected
                .config
                .and_then(|config| config.labels)
                .unwrap_or_default(),
            running,
            exit_code: if running { None } else { state.exit_code },
            address: inspected
                .network_settings
                .and_then(|settings| settings.networks)
                .and_then(|mut networks| networks.remove(&self.network))
                .and_then(|network| network.ip_address)
                .and_then(|address| address.parse().ok()),
        }
    }
}

#[async_trait]
impl Engine for DockerEngine {
    async fn image_present(&self, image: &str) -> Result<bool, SandboxError> {
        match self
            .request(
                (Call::InspectImage, image),
                self.docker.inspect_image(image),
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(SandboxError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn networks(&self) -> Result<crate::EngineNetworks, SandboxError> {
        let networks = self
            .request(
                (Call::ListNetworks, "networks"),
                self.docker
                    .list_networks(None::<bollard::query_parameters::ListNetworksOptions>),
            )
            .await?;
        Ok(crate::docker::engine_networks(&networks))
    }

    async fn network(&self, name: &str) -> Result<NetworkState, SandboxError> {
        match self
            .request(
                (Call::InspectNetwork, name),
                self.docker.inspect_network(name, None),
            )
            .await
        {
            Ok(network) if network.internal == Some(true) => Ok(NetworkState::Internal),
            Ok(_) => Ok(NetworkState::NotInternal),
            Err(SandboxError::NotFound) => Ok(NetworkState::Missing),
            Err(error) => Err(error),
        }
    }

    async fn create_and_start(
        &self,
        name: &str,
        body: ContainerCreateBody,
        bound: Duration,
    ) -> Result<Created, SandboxError> {
        let options = CreateContainerOptions {
            name: Some(name.into()),
            ..Default::default()
        };
        match self
            .request_answered(
                (Call::Create, name),
                bound,
                self.docker.create_container(Some(options), body),
            )
            .await
        {
            Ok(Ok(_)) => {}
            // Another create holds the name: an earlier attempt's, still in flight or landed late (SBX-F-035).
            Ok(Err(error)) if conflict(&error) => {
                tracing::warn!(
                    call = Call::Create.doing(),
                    target = name,
                    status = 409,
                    "the container name is held by an earlier attempt's create; waiting for it"
                );
                return Ok(Created::NameHeld);
            }
            Ok(Err(error)) => return Err(engine_error(Call::Create, name, &error)),
            Err(error) => {
                // A timed-out create may still have succeeded; the name is ours, so remove whatever exists.
                self.remove(name).await?;
                return Err(error);
            }
        }
        if let Err(error) = self
            .request_within(
                (Call::Start, name),
                bound,
                self.docker.start_container(name, None),
            )
            .await
        {
            self.remove(name).await?;
            return Err(error);
        }
        Ok(Created::Started)
    }

    async fn log_tail(&self, name: &str, lines: usize) -> Vec<String> {
        let options = LogsOptions {
            stdout: true,
            stderr: true,
            tail: lines.to_string(),
            ..Default::default()
        };
        let collect = async {
            let mut out = Vec::new();
            let mut stream = self.docker.logs(name, Some(options));
            while let Some(chunk) = stream.next().await {
                let text = chunk?.into_bytes();
                out.extend(String::from_utf8_lossy(&text).lines().map(str::to_owned));
            }
            Ok::<_, bollard::errors::Error>(out)
        };
        let mut found = self
            .request((Call::Logs, name), collect)
            .await
            .unwrap_or_default();
        let excess = found.len().saturating_sub(lines);
        found.drain(..excess);
        found
    }

    async fn inspect(&self, name: &str) -> Result<Container, SandboxError> {
        let inspected = self
            .request(
                (Call::Inspect, name),
                self.docker.inspect_container(name, None),
            )
            .await?;
        Ok(self.container(inspected))
    }

    async fn remove(&self, name: &str) -> Result<(), SandboxError> {
        let stop = StopContainerOptions {
            t: Some(2),
            ..Default::default()
        };
        // A failed stop (absent, already stopped, slow) is settled by the forced removal.
        let _ = self
            .request(
                (Call::Stop, name),
                self.docker.stop_container(name, Some(stop)),
            )
            .await;
        let remove = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        match self
            .request(
                (Call::Remove, name),
                self.docker.remove_container(name, Some(remove)),
            )
            .await
        {
            Ok(()) | Err(SandboxError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn list(&self, instance: &str) -> Result<Vec<Container>, SandboxError> {
        let labels = vec![
            format!("tqa.instance={instance}"),
            format!("{}={}", BACKEND_LABEL.0, BACKEND_LABEL.1),
        ];
        let options = ListContainersOptions {
            all: true,
            filters: Some(HashMap::from([("label".into(), labels)])),
            ..Default::default()
        };
        let summaries = self
            .request(
                (Call::List, instance),
                self.docker.list_containers(Some(options)),
            )
            .await?;
        if summaries.len() > MAX_CONTAINERS {
            return Err(SandboxError::Backend(
                "container listing exceeded its bound".into(),
            ));
        }
        Ok(summaries
            .into_iter()
            .map(|summary| Container {
                name: summary
                    .names
                    .and_then(|names| names.into_iter().next())
                    .unwrap_or_default()
                    .trim_start_matches('/')
                    .to_owned(),
                labels: summary.labels.unwrap_or_default(),
                running: summary
                    .state
                    .is_some_and(|state| state.to_string() == "running"),
                exit_code: None,
                address: None,
            })
            .collect())
    }

    async fn helpers(&self, instance: &str) -> Result<Vec<String>, SandboxError> {
        let labels = vec![
            format!("tqa.instance={instance}"),
            format!("{}={}", HELPER_LABEL.0, HELPER_LABEL.1),
        ];
        let options = ListContainersOptions {
            all: true,
            filters: Some(HashMap::from([("label".into(), labels)])),
            ..Default::default()
        };
        let summaries = self
            .request(
                (Call::List, instance),
                self.docker.list_containers(Some(options)),
            )
            .await?;
        if summaries.len() > MAX_CONTAINERS {
            return Err(SandboxError::Backend(
                "container listing exceeded its bound".into(),
            ));
        }
        Ok(summaries
            .into_iter()
            .filter_map(|summary| summary.names?.into_iter().next())
            .map(|name| name.trim_start_matches('/').to_owned())
            .collect())
    }

    async fn usage(&self, name: &str) -> Result<Usage, SandboxError> {
        let mut stream = self.docker.stats(
            name,
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        );
        let sample = self
            .request((Call::Stats, name), async {
                stream
                    .next()
                    .await
                    .unwrap_or(Err(bollard::errors::Error::RequestTimeoutError))
            })
            .await?;
        let missing =
            || SandboxError::Backend("required Linux resource accounting is missing".into());
        Ok(Usage {
            cpu_nanos: sample
                .cpu_stats
                .and_then(|s| s.cpu_usage)
                .and_then(|u| u.total_usage)
                .ok_or_else(missing)?,
            memory_bytes: sample
                .memory_stats
                .and_then(|s| s.usage)
                .ok_or_else(missing)?,
            pids: sample
                .pids_stats
                .and_then(|s| s.current)
                .ok_or_else(missing)?,
        })
    }

    async fn run(&self, body: ContainerCreateBody, bound: Duration) -> Result<i64, SandboxError> {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!("{}-helper-{stamp:x}", self.network);
        // A helper's name is new to this call: held, it belongs to no attempt of ours.
        if self.create_and_start(&name, body, bound).await? == Created::NameHeld {
            return Err(SandboxError::Backend(
                "creating a helper container: its name was taken".into(),
            ));
        }
        let mut wait = self.docker.wait_container(&name, None);
        let waited = self
            .request_within((Call::Wait, &name), bound, async {
                wait.next()
                    .await
                    .unwrap_or(Err(bollard::errors::Error::RequestTimeoutError))
            })
            .await;
        // A non-zero exit arrives as an error from the wait endpoint, so the inspected state is authoritative; a
        // timeout still removes the helper before it is reported.
        let code = match waited {
            Err(SandboxError::Timeout) => Err(SandboxError::Timeout),
            _ => self.inspect(&name).await.and_then(|container| {
                container.exit_code.ok_or_else(|| {
                    SandboxError::Backend("the helper container has no exit code".into())
                })
            }),
        };
        self.remove(&name).await?;
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answered(status_code: u16) -> bollard::errors::Error {
        bollard::errors::Error::DockerResponseServerError {
            status_code,
            message: "Conflict. The container name \"/private-name\" is already in use".into(),
        }
    }

    // TEST: SBX-F-033
    #[test]
    fn sbx_f_033_a_taken_name_on_create_is_a_timeout_to_retry() {
        // An earlier attempt's create that the engine finished late holds the name; the next attempt removes it.
        assert_eq!(
            engine_error(Call::Create, "vm-host-1", &answered(409)),
            SandboxError::Timeout
        );
    }

    // TEST: SBX-F-033
    #[test]
    fn sbx_f_033_an_engine_refusal_names_the_call_and_the_status_and_nothing_the_engine_said() {
        for (call, status, expected) in [
            (
                Call::Start,
                409,
                "starting a container: the engine answered HTTP 409",
            ),
            (
                Call::Create,
                500,
                "creating a container: the engine answered HTTP 500",
            ),
            (
                Call::Remove,
                409,
                "removing a container: the engine answered HTTP 409",
            ),
        ] {
            assert_eq!(
                engine_error(call, "vm-host-1", &answered(status)),
                SandboxError::Backend(expected.into())
            );
        }
        assert_eq!(
            engine_error(Call::Inspect, "vm-host-1", &answered(404)),
            SandboxError::NotFound
        );
        assert_eq!(
            engine_error(
                Call::Inspect,
                "vm-host-1",
                &bollard::errors::Error::RequestTimeoutError
            ),
            SandboxError::Backend(
                "inspecting a container: no usable answer from the engine".into()
            )
        );
    }

    /// The WARN lines logged while `work` runs.
    fn warnings(work: impl FnOnce()) -> Result<String, Box<dyn std::error::Error>> {
        #[derive(Clone, Default)]
        struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
            type Writer = Self;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }
        impl std::io::Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0
                    .lock()
                    .map_err(|_| std::io::Error::other("capture lock"))?
                    .extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let capture = Capture::default();
        tracing::subscriber::with_default(
            tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::WARN)
                .finish(),
            work,
        );
        let bytes = capture.0.lock().map_err(|_| "capture lock")?.clone();
        Ok(String::from_utf8(bytes)?)
    }

    // TEST: SBX-F-037
    #[test]
    fn sbx_f_037_a_failure_warning_names_its_target_and_what_came_back()
    -> Result<(), Box<dyn std::error::Error>> {
        let refused = bollard::errors::Error::DockerResponseServerError {
            status_code: 500,
            message: format!(
                "driver failed\nprogramming external connectivity with token sk-live-123 key \
                 0123456789abcdef0123456789abcdef then {}",
                "more words ".repeat(30)
            ),
        };
        let mut error = None;
        let logged =
            warnings(|| error = Some(engine_error(Call::Create, "tqa-vm-host-7", &refused)))?;
        // The error itself keeps naming only the call and the status (SBX-F-033).
        assert_eq!(
            error,
            Some(SandboxError::Backend(
                "creating a container: the engine answered HTTP 500".into()
            ))
        );
        assert!(logged.contains("creating a container"), "{logged}");
        assert!(logged.contains("target=\"tqa-vm-host-7\""), "{logged}");
        assert!(
            logged.contains("HTTP 500: driver failed programming external connectivity with token"),
            "the engine's reason, on one line: {logged}"
        );
        // Token-shaped text and credential values are redacted before the line is cut.
        for secret in ["sk-live-123", "0123456789abcdef0123456789abcdef"] {
            assert!(!logged.contains(secret), "{secret} in {logged}");
        }
        assert!(logged.contains("[redacted]"), "{logged}");
        let reason = logged.split("HTTP 500: ").nth(1).ok_or("no reason")?;
        let reason = reason.split('"').next().ok_or("no reason")?;
        assert!(
            reason.ends_with('…') && reason.len() <= 200 + '…'.len_utf8(),
            "capped: {reason}"
        );
        assert_eq!(logged.lines().count(), 1, "one line: {logged}");
        // A credential value: redacted from its name to the end of the line.
        let credential = bollard::errors::Error::DockerResponseServerError {
            status_code: 401,
            message: "registry refused: authorization: Bearer abc.def".into(),
        };
        let logged = warnings(|| {
            engine_error(Call::InspectImage, "example/image:stable", &credential);
        })?;
        assert!(
            logged.contains("HTTP 401: registry refused: authorization: [redacted]")
                && !logged.contains("abc.def"),
            "{logged}"
        );
        Ok(())
    }

    // TEST: SBX-F-037
    #[test]
    fn sbx_f_037_without_an_answer_only_the_kind_of_failure_is_named()
    -> Result<(), Box<dyn std::error::Error>> {
        // No answer: the kind of failure, never its data.
        let transport = bollard::errors::Error::IOError {
            err: std::io::Error::other("connection reset at /run/user/1000/docker.sock"),
        };
        let logged = warnings(|| {
            engine_error(Call::Start, "tqa-vm-host-7", &transport);
        })?;
        assert!(
            logged.contains("starting a container")
                && logged.contains("no answer from the engine (IOError)"),
            "{logged}"
        );
        assert!(!logged.contains("docker.sock"), "{logged}");
        let logged = warnings(|| {
            engine_error(
                Call::Inspect,
                "tqa-vm-host-7",
                &bollard::errors::Error::RequestTimeoutError,
            );
        })?;
        assert!(logged.contains("no answer in time"), "{logged}");
        // A taken name on create, and a failed stop: the target is named, or nothing is logged.
        let logged = warnings(|| {
            engine_error(Call::Create, "tqa-vm-host-7", &answered(409));
        })?;
        assert!(
            logged.contains("target=\"tqa-vm-host-7\"") && logged.contains("status=409"),
            "{logged}"
        );
        let logged = warnings(|| {
            engine_error(Call::Stop, "tqa-vm-host-7", &answered(500));
        })?;
        assert_eq!(
            logged, "",
            "a failed stop is settled by the removal after it"
        );
        Ok(())
    }
}
