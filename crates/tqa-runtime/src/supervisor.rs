//! A single owner serializes actions, journal projection, run lifetime and activeness.
mod actions;
mod environment;
mod shutdown;

/// The name of the supervisor's bounded wait for how the link ended, once its channels have closed (RT-I-038).
pub const LINK_END_WAIT: &str = "supervisor.link_end";
use crate::{
    Journal, JournalOptions, RuntimeDeps, RuntimeError, RuntimeExit, history,
    link::{Control, Link, LinkEvent},
    run_services::{Invocation, Services},
    session::Session,
    time::Time,
};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, UserTurn};
use tqa_types::{Event, RunId, Seq, ServerFrame};

pub(crate) async fn run(
    deps: RuntimeDeps,
    shutdown: CancellationToken,
) -> Result<RuntimeExit, RuntimeError> {
    let mut supervisor = Supervisor::open(deps).await?;
    let result = supervisor.drive(&shutdown).await;
    let cleanup = supervisor.finish(&result).await;
    match result {
        Ok(reason) => cleanup.map(|()| reason),
        Err(RuntimeError::Stopped) => cleanup.map(|()| RuntimeExit::Shutdown),
        Err(error) => {
            if cleanup.is_err() {
                tracing::warn!("runtime shutdown cleanup failed");
            }
            Err(error)
        }
    }
}
struct Supervisor {
    services: Services,
    frames: mpsc::Receiver<ServerFrame>,
    actions: mpsc::Receiver<LinkEvent>,
    controls: mpsc::Sender<Control>,
    /// The gateway's workspace requests (ADR-0030).
    workspace: crate::workspace_service::Service,
    link: JoinSet<Result<(), RuntimeError>>,
    runs: JoinSet<Result<RunOutcome, RuntimeError>>,
    link_shutdown: CancellationToken,
    /// The screen bridge of a session whose environment declares `desktop` (ADR-0039 section 5); it ends with the link.
    screen: JoinSet<()>,
    run_shutdown: CancellationToken,
    cancel: CancellationToken,
    session: Option<Session>,
    history: Vec<ServerFrame>,
    history_bytes: usize,
    last: Seq,
    active: Option<RunId>,
    heartbeat: Option<Instant>,
    bootstrap_idle: bool,
    pending_message: Option<(UserTurn, Vec<tqa_types::HostKeyGrant>)>,
    #[cfg(feature = "testing")]
    testing: crate::testing::TestingHooks,
}
/// Where and how the journal keeps the run's frames.
fn journal_options(deps: &RuntimeDeps) -> JournalOptions {
    JournalOptions {
        directory: deps.config.state_dir.clone(),
        wall_clock: deps.wall_clock.clone(),
        sleeper: deps.sleeper.clone(),
        limits: deps.limits,
        workspace_sync: deps.config.sync_before_result.then(|| {
            Arc::new(crate::SyncFs::new(deps.config.workspace.clone()))
                as Arc<dyn crate::WorkspaceSync>
        }),
        #[cfg(feature = "testing")]
        testing: deps.testing.clone(),
    }
}

impl Supervisor {
    async fn open(deps: RuntimeDeps) -> Result<Self, RuntimeError> {
        let mcp = mcp_supervisor(&deps);
        let journal = Journal::open(journal_options(&deps)).await?;
        let time = Time {
            clock: deps.clock,
            sleeper: deps.sleeper,
        };
        let (actions, receive) = mpsc::channel(deps.limits.queue_capacity);
        let (controls, control_receive) = mpsc::channel(deps.limits.queue_capacity);
        let link_shutdown = CancellationToken::new();
        // The link's state and probes, for the runner's GatewayLink (CORE-U-062).
        let (link_state, probe_receive, gateway_link) =
            crate::link::LinkHandle::open(time.clone(), &deps.limits);
        let mut link = JoinSet::new();
        link.spawn(
            Link {
                config: deps.config.clone(),
                limits: deps.limits,
                time: time.clone(),
                jitter: deps.reconnect_jitter,
                journal: journal.journal.clone(),
                changes: journal.changes,
                controls: control_receive,
                actions,
                shutdown: link_shutdown.clone(),
                state: link_state,
                probes: probe_receive,
                #[cfg(feature = "testing")]
                testing: deps.testing.clone(),
            }
            .run(),
        );
        let screen_store = Arc::new(crate::screen_store::LinkScreenStore::new(
            controls.clone(),
            time.clone(),
        ));
        Ok(Self {
            workspace: crate::workspace_service::Service::new(
                deps.config.workspace.clone(),
                controls.clone(),
            ),
            services: Services {
                #[cfg(feature = "testing")]
                testing: deps.testing.clone(),
                mcp,
                desktop: crate::desktop::Slot::default(),
                config: deps.config,
                limits: deps.limits,
                time,
                journal: journal.journal,
                cleanup: deps.cleanup,
                repository_cloner: deps.repository_cloner,
                jitter: Arc::new(Mutex::new(deps.model_jitter)),
                gateway_link,
                screen_store,
            },
            frames: journal.frames,
            actions: receive,
            controls,
            link,
            runs: JoinSet::new(),
            link_shutdown,
            screen: JoinSet::new(),
            run_shutdown: CancellationToken::new(),
            cancel: CancellationToken::new(),
            session: None,
            history: vec![],
            history_bytes: 0,
            last: Seq::ZERO,
            active: None,
            heartbeat: None,
            bootstrap_idle: false,
            pending_message: None,
            #[cfg(feature = "testing")]
            testing: deps.testing,
        })
    }
    async fn drive(&mut self, shutdown: &CancellationToken) -> Result<RuntimeExit, RuntimeError> {
        loop {
            #[cfg(feature = "testing")]
            self.testing.supervisor.checkpoint(shutdown).await;
            let interval = self
                .heartbeat
                .map_or(self.services.limits.heartbeat_interval, |deadline| {
                    deadline.saturating_duration_since(self.services.time.clock.now())
                });
            tokio::select! {biased;
                ()=shutdown.cancelled()=>return Ok(RuntimeExit::Shutdown),
                result=self.link.join_next()=>{
                    result.ok_or(RuntimeError::Worker)?.map_err(|_|RuntimeError::Worker)??;
                    return Ok(RuntimeExit::Shutdown);
                },
                frame=self.frames.recv()=>self.record(frame.ok_or(RuntimeError::Worker)?)?,
                result=self.runs.join_next(),if !self.runs.is_empty()=>{
                    result.ok_or(RuntimeError::Worker)?.map_err(|_|RuntimeError::Worker)??;
                    self.bootstrap_idle=false;self.active=None;self.heartbeat=None;
                    self.signed_out_after_run().await;
                    if let Some((message, host_keys)) = self.pending_message.take() { self.start(Some(message), host_keys)?; }
                }
                ()=self.services.time.sleeper.sleep(interval),if self.active.is_some()=>self.heartbeat(),
                action=self.actions.recv()=>{
                    // The link holds the only sender: closed, it has ended or is ending, so how it ended decides.
                    let Some(action)=action else { return self.link_ended().await; };
                    if let Some(reason)=self.action(action,shutdown).await? {
                        return Ok(reason);
                    }
                },
            }
        }
    }
    /// How the link ended, once its channels have closed: on a multi-threaded runtime the supervisor can see them
    /// closed before the link's task is joined. A clean stop (the gateway retired the sandbox and refused the redial)
    /// is a shutdown; the link's error stays its error; no answer within the I/O bound is a timeout (RT-I-038).
    async fn link_ended(&mut self) -> Result<RuntimeExit, RuntimeError> {
        let time = self.services.time.clone();
        let joined = time
            .wait_named(
                self.services.limits.io_timeout,
                LINK_END_WAIT,
                self.link.join_next(),
            )
            .await?;
        joined
            .ok_or(RuntimeError::Worker)?
            .map_err(|_| RuntimeError::Worker)??;
        Ok(RuntimeExit::Shutdown)
    }
    fn start(
        &mut self,
        message: Option<UserTurn>,
        host_keys: Vec<tqa_types::HostKeyGrant>,
    ) -> Result<(), RuntimeError> {
        let session = self.session.clone().ok_or(RuntimeError::Worker)?;
        let services = self.services.clone();
        self.cancel = CancellationToken::new();
        let input = Invocation {
            session,
            history: self.history.clone(),
            message,
            host_keys,
            cancel: self.cancel.clone(),
            shutdown: self.run_shutdown.clone(),
        };
        self.runs.spawn(async move { services.invoke(input).await });
        Ok(())
    }
    fn record(&mut self, frame: ServerFrame) -> Result<(), RuntimeError> {
        let bytes = serde_json::to_vec(&frame)
            .map_err(|_| RuntimeError::Frame)?
            .len();
        if self.history.len() >= self.services.limits.history_frames
            || bytes
                > self
                    .services
                    .limits
                    .history_bytes
                    .saturating_sub(self.history_bytes)
        {
            return Err(RuntimeError::Limit);
        }
        history::advance(&mut self.last, &frame)?;
        if let ServerFrame::Event { event, .. } = &frame {
            match event {
                Event::RunStarted { run_id } | Event::RunResumed { run_id, .. } => {
                    self.active = Some(run_id.clone());
                    self.heartbeat = self
                        .services
                        .time
                        .clock
                        .now()
                        .checked_add(self.services.limits.heartbeat_interval);
                }
                Event::RunCompleted { .. }
                | Event::RunFailed { .. }
                | Event::RunCancelled { .. } => {
                    self.active = None;
                    self.heartbeat = None;
                }
                _ => {}
            }
        }
        self.history_bytes += bytes;
        self.history.push(frame);
        Ok(())
    }
    fn heartbeat(&mut self) {
        if let Some(run_id) = &self.active {
            self.control(ServerFrame::Heartbeat {
                run_id: run_id.clone(),
            });
        }
        self.heartbeat = self
            .services
            .time
            .clock
            .now()
            .checked_add(self.services.limits.heartbeat_interval);
    }
    fn control(&self, frame: ServerFrame) {
        if self
            .controls
            .try_send(Control { frame, sent: None })
            .is_err()
        {
            tracing::warn!("ephemeral runtime control frame could not be queued");
        }
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.run_shutdown.cancel();
        self.link_shutdown.cancel();
    }
}

/// The session's MCP supervisor: spawned groups are recorded for the reaper, and connectors use this runtime's own
/// gateway origin and token (ADR-0031: never a URL from the wire).
fn mcp_supervisor(deps: &RuntimeDeps) -> Arc<tokio::sync::Mutex<crate::mcp::McpSupervisor>> {
    Arc::new(tokio::sync::Mutex::new(crate::mcp::McpSupervisor::new(
        deps.cleanup.supervised(),
        crate::mcp::HANDSHAKE,
        Some((deps.config.origin.clone(), deps.config.token.clone())),
    )))
}
