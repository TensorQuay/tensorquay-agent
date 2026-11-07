//! Deterministic runtime fixtures; real waits only bound loopback test-control I/O.
#![allow(dead_code)]
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tqa_runtime::{
    ProcessCleanup, Runtime, RuntimeConfig, RuntimeDeps, RuntimeError, RuntimeInputs,
    RuntimeLimits, WallClock,
};
use tqa_testkit::{
    FakeClock, FakeGateway, FakeGatewayOptions, FakeSleeper, FixedJitter, Script, Scripted,
};
use tqa_types::{ClientFrame, Event, ServerFrame};
type RuntimeTask = (CancellationToken, JoinHandle<Result<(), RuntimeError>>);
pub type TestResult = Result<(), Box<dyn std::error::Error>>;
pub struct FixedWall;
impl WallClock for FixedWall {
    fn timestamp(&self) -> Result<String, RuntimeError> {
        Ok("2000-01-01T00:00:00Z".into())
    }
}
pub struct NoChildren;
#[async_trait]
impl ProcessCleanup for NoChildren {
    async fn reap(&self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
pub fn session() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    Ok(ClientFrame::parse(serde_json::to_string(&serde_json::json!({"type":"session.config","conversation_id":"conversation","last_stored_seq":0,
    "route":{"name":"local","protocol":"openai_chat","model":"fake-model","relay_path":"/relay/r/local","context_window":32768,"max_output":4096,"native_tools":true,"parallel_tools":true,"vision":false,"reasoning":null},
    "network":{"allowed":[],"more":0},"limits":{"max_steps":10,"wall_clock_secs":600,"tool_result_bytes":8192},"workspace":{"display":"/workspace"},"protocol":1}))?.as_bytes())?)
}
pub fn answer() -> Script {
    Script::new(vec![Scripted::Text("answer".into())])
}
pub struct Fixture {
    pub directory: TempDir,
    pub gateway: FakeGateway,
    pub clock: FakeClock,
    pub sleeper: Arc<FakeSleeper>,
}
/// Where a fixture keeps the runtime's state, journal included.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StateStorage {
    /// The normal temporary directory on disk, so the journal's fsync path runs on real storage. The default, and
    /// required for every test whose subject is the journal, fsync, a crash or a resume.
    Disk,
    /// RAM-backed storage where the host offers it, for tests whose subject is not durability.
    Memory,
}

/// Chooses the fixture's state directory. On a loaded host a disk fsync can take seconds, longer than these tests'
/// real-time bounds (RT-I-014; reproduced with tests/support/slowsync.c at >= 2 s per sync), so tests whose subject
/// is not durability keep state on tmpfs at /dev/shm. Hosts without it (macOS) use the normal temporary directory.
fn state_root(storage: StateStorage) -> std::io::Result<TempDir> {
    let shm = std::path::Path::new("/dev/shm");
    let on_tmpfs = std::fs::read_to_string("/proc/mounts").is_ok_and(|mounts| {
        mounts.lines().any(|line| {
            let mut fields = line.split_whitespace();
            fields.nth(1) == Some("/dev/shm") && fields.next() == Some("tmpfs")
        })
    });
    if storage == StateStorage::Memory && on_tmpfs {
        TempDir::new_in(shm)
    } else {
        TempDir::new()
    }
}

impl Fixture {
    pub async fn new(scripts: Vec<Script>) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_history(scripts, vec![]).await
    }
    /// A fixture whose state lives in RAM where available; only for tests whose subject is not durability.
    pub async fn in_memory(scripts: Vec<Script>) -> Result<Self, Box<dyn std::error::Error>> {
        Self::build(scripts, vec![], StateStorage::Memory).await
    }
    pub async fn with_history(
        scripts: Vec<Script>,
        history: Vec<ServerFrame>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::build(scripts, history, StateStorage::Disk).await
    }
    async fn build(
        scripts: Vec<Script>,
        history: Vec<ServerFrame>,
        storage: StateStorage,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = state_root(storage)?;
        std::fs::create_dir(directory.path().join("state"))?;
        std::fs::create_dir(directory.path().join("workspace"))?;
        std::fs::write(
            directory.path().join("credential"),
            "test-runtime-credential\n",
        )?;
        let clock = FakeClock::new();
        let sleeper = Arc::new(FakeSleeper::new(clock.clone()));
        let gateway = FakeGateway::start(FakeGatewayOptions {
            token: "test-runtime-credential".into(),
            session: session()?,
            history,
            scripts,
            sleeper: sleeper.clone(),
        })
        .await?;
        Ok(Self {
            directory,
            gateway,
            clock,
            sleeper,
        })
    }
    pub fn deps(&self) -> Result<RuntimeDeps, Box<dyn std::error::Error>> {
        self.deps_for_egress(None)
    }
    pub fn deps_for_egress(
        &self,
        egress_proxy_url: Option<String>,
    ) -> Result<RuntimeDeps, Box<dyn std::error::Error>> {
        let config = RuntimeConfig::load(RuntimeInputs {
            gateway_url: self.gateway.origin().into(),
            egress_proxy_url,
            token_file: self.directory.path().join("credential"),
            state_dir: self.directory.path().join("state"),
            workspace: self.directory.path().join("workspace"),
            skills: self.directory.path().join("skills"),
            sync_before_result: false,
        })?;
        Ok(RuntimeDeps {
            config,
            limits: RuntimeLimits::RECOMMENDED,
            clock: Arc::new(self.clock.clone()),
            sleeper: self.sleeper.clone(),
            wall_clock: Arc::new(FixedWall),
            reconnect_jitter: Box::new(FixedJitter::new(0.5)?),
            model_jitter: Box::new(FixedJitter::new(0.5)?),
            cleanup: Arc::new(NoChildren),
            repository_cloner: Arc::new(tqa_runtime::testing::GitCloner::new(self.sleeper.clone())),
            testing: tqa_runtime::testing::TestingHooks::default(),
        })
    }
    /// Launches with `testing`'s gates, which the caller keeps a handle on.
    pub fn launch_with(
        &self,
        testing: tqa_runtime::testing::TestingHooks,
    ) -> Result<RuntimeTask, Box<dyn std::error::Error>> {
        let mut deps = self.deps()?;
        deps.testing = testing;
        let runtime = Runtime::new(deps)?;
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        Ok((
            shutdown,
            tokio::spawn(async move { runtime.run(token).await }),
        ))
    }
    pub fn launch(&self) -> Result<RuntimeTask, Box<dyn std::error::Error>> {
        let runtime = Runtime::new(self.deps()?)?;
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        Ok((
            shutdown,
            tokio::spawn(async move { runtime.run(token).await }),
        ))
    }
    /// Advances fake time once no real I/O is in flight. The runtime bounds each journal and link write with
    /// `io_timeout` on the fake sleeper, so a jump past it while a write is pending turns that write into a timeout
    /// (CORE-U-019). Waits until every pending sleep is longer than that guard; only call it when the scenario has
    /// no deliberate short sleep pending. The check and the advance run without yielding in between.
    pub async fn advance_when_idle(&self, by: Duration) -> Result<(), Box<dyn std::error::Error>> {
        let guard = RuntimeLimits::RECOMMENDED.io_timeout;
        wait_for(|| {
            self.sleeper
                .pending_durations()
                .iter()
                .all(|pending| *pending > guard)
        })
        .await?;
        self.clock.advance(by)?;
        Ok(())
    }
    pub async fn wait_event(&self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        wait_for(|| {
            self.gateway.frames().iter().any(
                |frame| matches!(frame,ServerFrame::Event {event,..} if event_name(event)==name),
            )
        })
        .await
    }
}
pub fn event_name(event: &Event) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|v| {
            v.get("type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}
/// Waits for `ready` within a liveness bound that scales with `TQA_TEST_DEADLINE_SCALE` (coverage runs set it).
pub async fn wait_for(mut ready: impl FnMut() -> bool) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        async {
            while !ready() {
                tokio::task::yield_now().await;
            }
        },
    )
    .await?;
    Ok(())
}
pub async fn stop(
    token: CancellationToken,
    task: JoinHandle<Result<(), RuntimeError>>,
) -> TestResult {
    token.cancel();
    tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await???;
    Ok(())
}
pub fn user() -> ClientFrame {
    ClientFrame::UserMessage {
        lead_brief: None,
        team_wake: None,
        text: "question".into(),
        attachments: vec![],
        host_keys: Vec::new(),
    }
}
