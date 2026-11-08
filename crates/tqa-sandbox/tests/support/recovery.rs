//! Real runtime recovery through the production sandbox backend and persistent mounts.
#[path = "recovery_scenario.rs"]
mod recovery_scenario;
use super::support::{self, Fixture, TestResult, bounded};
use bollard::Docker;
pub use recovery_scenario::{Cut, Task};
use recovery_scenario::{MODEL, START};
use std::{collections::BTreeMap, error::Error, net::Ipv4Addr, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{
    ByteSize, CpuLimit, ImageRef, SandboxBackend, SandboxHandle, SandboxSpec,
    docker::{DockerBackend, DockerBackendConfig, NetworkMode},
};
use tqa_testkit::{FakeClock, FakeGateway, FakeGatewayOptions, FakeSleeper, reach};
use tqa_types::{ClientFrame, Event, Seq, ServerFrame, ToolStatus};

pub async fn exercise(
    task: Task,
    cut: Cut,
    interrupt: impl for<'a> AsyncFnOnce(&'a DockerBackend, &'a Docker, &'a SandboxHandle) -> TestResult,
) -> TestResult {
    support::run(async |fixture| {
        let harness = Harness::new(fixture, task, cut).await?;
        let first = harness.create().await?;
        harness.reachable(&first).await?;
        harness.gateway.wait_for_registrations(1).await?;
        harness
            .begin(cut)
            .await
            .map_err(|error| format!("interruption checkpoint was not reached: {error}"))?;
        interrupt(&harness.backend, &fixture.docker, &first).await?;
        assert_stopped(&fixture.docker, &first).await?;
        let retained = harness.retained()?;
        harness.backend.destroy(&first).await?;
        harness.gateway.set_persist(true);
        let replacement = harness.create().await?;
        assert_ne!(replacement.id, first.id);
        assert_eq!(replacement.conversation_id, first.conversation_id);
        harness
            .gateway
            .wait_for_registrations(2)
            .await
            .map_err(|error| format!("replacement registration failed: {error}"))?;
        wait(|| harness.gateway.stored().iter().any(completed))
            .await
            .map_err(|error| format!("replacement did not complete after WAL replay: {error}"))?;
        harness.verify(task, cut, &retained, &replacement).await?;
        harness.backend.destroy(&replacement).await?;
        assert!(harness.backend.list().await?.is_empty());
        harness.gateway.shutdown().await?;
        Ok(())
    })
    .await
}

/// SBX-C-018 (GW-I-158's case): how long destroying an idle runtime takes: linked to the gateway and registered, with
/// no run. The slot a waiting task needs frees only when this ends.
pub async fn idle_destroy() -> Result<Duration, Box<dyn Error>> {
    let mut elapsed = Duration::ZERO;
    support::run(async |fixture| {
        let harness = Harness::new(fixture, Task::Arithmetic, Cut::Tool).await?;
        let idle = harness.create().await?;
        harness.reachable(&idle).await?;
        harness.gateway.wait_for_registrations(1).await?;
        let started = std::time::Instant::now();
        harness.backend.destroy(&idle).await?;
        elapsed = started.elapsed();
        assert!(harness.backend.list().await?.is_empty());
        harness.gateway.shutdown().await?;
        Ok(())
    })
    .await?;
    Ok(elapsed)
}

struct Harness<'a> {
    fixture: &'a Fixture,
    backend: DockerBackend,
    gateway: FakeGateway,
    clock: FakeClock,
    sleeper: Arc<FakeSleeper>,
    spec: SandboxSpec,
}

impl<'a> Harness<'a> {
    async fn new(fixture: &'a Fixture, task: Task, cut: Cut) -> Result<Self, Box<dyn Error>> {
        assert!(fixture.backend.list().await?.is_empty());
        let clock = FakeClock::new();
        let sleeper = Arc::new(FakeSleeper::new(clock.clone()));
        let mut spec = runtime_spec(fixture).await?;
        let session = session(spec.conversation_id.as_str())?;
        let gateway = FakeGateway::start_on(
            FakeGatewayOptions {
                token: "test-runtime-credential".into(),
                session,
                history: vec![],
                scripts: recovery_scenario::scripts(task, cut)?,
                sleeper: sleeper.clone(),
            },
            Ipv4Addr::UNSPECIFIED,
        )
        .await?;
        gateway.set_bound(true);
        let port = url::Url::parse(gateway.origin())?
            .port()
            .ok_or("gateway port")?;
        spec.gateway_url =
            format!("http://{}:{port}", gateway_host(&fixture.docker).await?).parse()?;
        let mut config = DockerBackendConfig::new(&fixture.prefix, fixture.storage.path())?;
        config.network = NetworkMode::DevHostGateway;
        let backend = DockerBackend::connect(config).await?;
        Ok(Self {
            fixture,
            backend,
            gateway,
            clock,
            sleeper,
            spec,
        })
    }

    async fn create(&self) -> Result<SandboxHandle, Box<dyn Error>> {
        Ok(self
            .backend
            .create(self.spec.clone(), CancellationToken::new())
            .await?)
    }

    /// A runtime that can't reach the fake gateway would only time out waiting to register; say why at once.
    async fn reachable(&self, handle: &SandboxHandle) -> TestResult {
        let host = self.spec.gateway_url.host_str().ok_or("gateway host")?;
        let port = self.spec.gateway_url.port().ok_or("gateway port")?;
        if let Some(message) = reach::probe(&handle.id, host, port)
            .await
            .failure(host, port)
        {
            self.backend.destroy(handle).await?;
            return Err(message.into());
        }
        Ok(())
    }

    async fn begin(&self, cut: Cut) -> TestResult {
        self.gateway
            .send(ClientFrame::UserMessage {
                lead_brief: None,
                team_wake: None,
                text: "Implement the task, test it, fix it, and verify the result.".into(),
                attachments: vec![],
                host_keys: Vec::new(),
            })
            .await?;
        wait(|| self.sleeper.pending_durations().contains(&START)).await?;
        assert!(!self.gateway.stored().is_empty());
        self.gateway.set_persist(false);
        self.clock.advance(START)?;
        match cut {
            Cut::Model => wait(|| self.sleeper.pending_durations().contains(&MODEL)).await?,
            Cut::Tool => wait(|| self.fixture.workspace.path().join("tool-ready").exists()).await?,
        }
        assert!(!self.gateway.frames().iter().any(completed));
        assert_eq!(
            std::fs::read_to_string(self.fixture.workspace.path().join("effects.log"))?,
            "created\ninitial\n"
        );
        Ok(())
    }

    fn retained(&self) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
        let path = self.fixture.state.path().join("events.jsonl");
        let before = std::fs::read(&path)?;
        let complete = before
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        let (wal, recovered) = tqa_context::Wal::open(self.fixture.state.path())?;
        assert_eq!(recovered.truncated_tail, complete != before.len());
        assert_eq!(std::fs::read(path)?, before[..complete]);
        let mut history = self.gateway.stored();
        let watermark = history.last().map_or(Seq::ZERO, sequence);
        let tail = recovered
            .records
            .into_iter()
            .filter(|(seq, _)| *seq > watermark)
            .map(|(_, frame)| frame)
            .collect::<Vec<_>>();
        assert!(
            !tail.is_empty(),
            "recovery must depend on unacknowledged WAL records"
        );
        history.extend(tail);
        contiguous(&history);
        for frame in self
            .gateway
            .frames()
            .iter()
            .filter(|frame| sequence(frame) > watermark)
        {
            assert!(
                history.contains(frame),
                "an observed event was lost from the WAL"
            );
        }
        assert!(!history.iter().any(completed));
        drop(wal);
        Ok(history)
    }

    async fn verify(
        &self,
        task: Task,
        cut: Cut,
        retained: &[ServerFrame],
        handle: &SandboxHandle,
    ) -> TestResult {
        let history = self.gateway.stored();
        check_history(&history, retained, cut);
        assert_eq!(
            self.gateway.model_requests().len(),
            if cut == Cut::Model { 8 } else { 7 }
        );
        let workspace = self.fixture.workspace.path();
        assert_eq!(
            std::fs::read_to_string(workspace.join("effects.log"))?,
            "created\ninitial\nverified\n"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("demo/solution.py"))?,
            task.recipe().1
        );
        assert!(
            std::fs::read_to_string(workspace.join("demo/README.md"))?
                .contains("Verified: two tests pass.")
        );
        let (exit, output) = self
            .fixture
            .exec(
                &handle.id,
                &[
                    "python3", "-B", "-m", "unittest", "discover", "-s", "demo", "-v",
                ],
            )
            .await?;
        assert_eq!(exit, 0, "{output}");
        assert!(
            output.contains("Ran 2 tests") && output.contains("OK"),
            "{output}"
        );
        Ok(())
    }
}

fn check_history(history: &[ServerFrame], retained: &[ServerFrame], cut: Cut) {
    assert_eq!(
        &history[..retained.len()],
        retained,
        "WAL replay changed or lost the prefix"
    );
    contiguous(history);
    for name in [
        "message.user",
        "run.started",
        "run.resumed",
        "run.completed",
    ] {
        assert_eq!(
            history
                .iter()
                .filter(|frame| event_name(frame) == name)
                .count(),
            1,
            "{name}"
        );
    }
    assert!(
        !history
            .iter()
            .any(|frame| matches!(frame, ServerFrame::Error { .. }))
    );
    let original = history.iter().find_map(|frame| match frame {
        ServerFrame::Event {
            event: Event::RunStarted { run_id },
            ..
        } => Some(run_id),
        _ => None,
    });
    for frame in history {
        if let ServerFrame::Event {
            event: Event::RunResumed { run_id, from_step },
            ..
        } = frame
        {
            assert_eq!(Some(run_id), original);
            assert_eq!(*from_step, if cut == Cut::Model { 4 } else { 3 });
        }
    }
    if cut == Cut::Tool {
        assert!(history.iter().any(|frame| matches!(
            frame,
            ServerFrame::Event {
                event: Event::ToolResult {
                    status: ToolStatus::Interrupted,
                    ..
                },
                ..
            }
        )));
    }
    check_tools(history);
}

fn check_tools(history: &[ServerFrame]) {
    let mut starts = std::collections::HashSet::new();
    let mut results = std::collections::HashSet::new();
    for frame in history {
        if let ServerFrame::Event {
            run_id,
            step,
            event,
            ..
        } = frame
        {
            match event {
                Event::ToolStart { call_id } => {
                    assert!(starts.insert((run_id, step, call_id)), "tool ran twice");
                }
                Event::ToolResult { call_id, .. } => assert!(
                    results.insert((run_id, step, call_id)),
                    "duplicate tool result"
                ),
                _ => {}
            }
        }
    }
    assert_eq!(starts.len(), 13);
    assert_eq!(results, starts);
}

async fn runtime_spec(fixture: &Fixture) -> Result<SandboxSpec, Box<dyn Error>> {
    let selected = match std::env::var("TQA_RUNTIME_TEST_IMAGE") {
        Ok(value) if !value.is_empty() => value,
        Err(std::env::VarError::NotPresent) => "tqa-runtime:workstation".into(),
        _ => return Err("TQA_RUNTIME_TEST_IMAGE must be nonempty UTF-8".into()),
    };
    let identity = bounded(fixture.docker.inspect_image(&selected))
        .await?
        .id
        .ok_or("runtime image ID")?;
    let mut spec = fixture.spec("recovery", true)?;
    spec.image = ImageRef::local_id(identity)?;
    spec.env = BTreeMap::from([
        (
            "PATH".into(),
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        ),
        ("HOME".into(), "/tmp".into()),
    ]);
    spec.runtime_token = secrecy::SecretString::from("test-runtime-credential");
    spec.cpu = CpuLimit::new(1000)?;
    spec.memory = ByteSize::new(512 * 1024 * 1024)?;
    spec.pids = 256;
    Ok(spec)
}

fn session(conversation: &str) -> Result<ClientFrame, Box<dyn Error>> {
    Ok(ClientFrame::parse(&serde_json::to_vec(
        &serde_json::json!({
            "type":"session.config","conversation_id":conversation,"last_stored_seq":0,
            "route":{"name":"local","protocol":"openai_chat","model":"fixture","relay_path":"/relay/r/local",
            "context_window":32768,"max_output":4096,"native_tools":true,"parallel_tools":true,"vision":false,"reasoning":null},
            "network":{"allowed":[],"more":0},"limits":{"max_steps":10,"wall_clock_secs":600,"tool_result_bytes":8192},
            "workspace":{"display":"/workspace"},"protocol":1
        }),
    )?)?)
}

async fn gateway_host(docker: &Docker) -> Result<String, Box<dyn Error>> {
    let info = bounded(docker.info()).await?;
    if info
        .operating_system
        .is_some_and(|name| name.contains("Docker Desktop"))
    {
        return Ok("host.docker.internal".into());
    }
    bounded(docker.inspect_network("bridge", None))
        .await?
        .ipam
        .and_then(|ipam| ipam.config)
        .into_iter()
        .flatten()
        .filter_map(|config| config.gateway)
        .find(|host| host.parse::<Ipv4Addr>().is_ok())
        .ok_or_else(|| "host gateway address missing".into())
}

async fn assert_stopped(docker: &Docker, handle: &SandboxHandle) -> TestResult {
    match bounded(docker.inspect_container(&handle.id, None)).await {
        Ok(container) => assert_ne!(container.state.and_then(|state| state.running), Some(true)),
        Err(error)
            if matches!(
                error.downcast_ref::<bollard::errors::Error>(),
                Some(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404,
                    ..
                })
            ) => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn sequence(frame: &ServerFrame) -> Seq {
    match frame {
        ServerFrame::Event { seq, .. } | ServerFrame::PermissionRequest { seq, .. } => *seq,
        ServerFrame::Error { seq, .. } => seq.unwrap_or(Seq::ZERO),
        _ => Seq::ZERO,
    }
}
fn contiguous(frames: &[ServerFrame]) {
    for (index, frame) in frames.iter().enumerate() {
        assert_eq!(sequence(frame), Seq::from(index as u64 + 1));
    }
}
fn event_name(frame: &ServerFrame) -> String {
    serde_json::to_value(frame)
        .ok()
        .and_then(|value| value["event"]["type"].as_str().map(str::to_owned))
        .unwrap_or_default()
}
fn completed(frame: &ServerFrame) -> bool {
    matches!(
        frame,
        ServerFrame::Event {
            event: Event::RunCompleted { .. },
            ..
        }
    )
}
async fn wait(mut ready: impl FnMut() -> bool) -> TestResult {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}
