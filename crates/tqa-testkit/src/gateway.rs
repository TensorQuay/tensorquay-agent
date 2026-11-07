//! Scripted runtime gateway with bound-token authentication and an authoritative in-memory journal.
mod http;
mod path;
mod socket;
use crate::{FakeModelServer, RecordedRequest, Script};
use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};
use std::{
    collections::VecDeque,
    io,
    net::Ipv4Addr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Notify, mpsc},
    task::JoinHandle,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tqa_types::{ClientFrame, ServerFrame, Sleeper};

/// How long a test waits for the runtime to register, send or finish; scaled for loaded coverage runs.
pub(super) fn deadline() -> Duration {
    crate::deadline::scaled(Duration::from_secs(5))
}
const MAX_FRAMES: usize = 16_384;
const MAX_REPLAY_FRAMES: usize = 131_072;
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// Explicit scripts, credential and authoritative starting state for the fake gateway.
pub struct FakeGatewayOptions {
    /// Test-only credential, never recorded in observations or diagnostics.
    pub token: String,
    /// Valid session.config; its watermark is filled from the authoritative history on registration.
    pub session: ClientFrame,
    /// Initial gateway history; gaps can deliberately test runtime rejection.
    pub history: Vec<ServerFrame>,
    /// FIFO model responses served through the relay endpoint.
    pub scripts: Vec<Script>,
    /// Clock-driven model delays.
    pub sleeper: Arc<dyn Sleeper>,
}
/// An observed gateway request without credential material or received payloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayRequest {
    /// Request path, without query values.
    pub path: String,
    /// Whether its bearer matched the configured test credential.
    pub authenticated: bool,
    /// The protocol revision header, when present and at most 16 bytes of text.
    pub protocol_revision: Option<String>,
}
/// What the fake's `GET /runtime/teams/role` does.
#[derive(Clone)]
enum RoleRoute {
    /// 404, as a gateway without the team routes.
    Absent,
    /// Answers this role.
    Role(String),
    /// Never answers.
    Hangs,
}

struct Data {
    bound: bool,
    link_available: bool,
    frame_limit: usize,
    session: ClientFrame,
    stored: Vec<ServerFrame>,
    frames: Vec<ServerFrame>,
    requests: Vec<GatewayRequest>,
    first: VecDeque<String>,
    registrations: usize,
    closes: usize,
    /// Serialized actions for the current socket, sent verbatim.
    actions: Option<mpsc::Sender<String>>,
    connection: Option<CancellationToken>,
    bytes: usize,
    persist: bool,
    /// The conversation's channel memberships, or `None` for a gateway without the channel routes (404).
    channels: Option<Vec<serde_json::Value>>,
    team_role: RoleRoute,
    /// The answers `POST /relay/d/{engine}/v1/{point}` gives in order; with none left it answers 503.
    decisions: VecDeque<tqa_types::decision_v1::Response>,
    /// While the store is stalled, the runtime's frames read meanwhile, unrecorded and in order (`set_store_stalled`).
    stalled: Option<Vec<Vec<u8>>>,
}
struct Shared {
    data: Mutex<Data>,
    /// Holds the gateway's reply to the runtime's Close, to observe whether the runtime waits for it.
    hold_close: std::sync::atomic::AtomicBool,
    bearer: String,
    model_origin: String,
    client: reqwest::Client,
    cancel: CancellationToken,
    changed: Notify,
    connections: TaskTracker,
    /// The runtime's network path: cancelled while a test holds it down (`set_path_down`, TK-U-028).
    path: Mutex<CancellationToken>,
}
/// Owned loopback gateway. Tokens initially await binding; observations are bounded and credential-free.
pub struct FakeGateway {
    origin: String,
    shared: Arc<Shared>,
    model: Option<FakeModelServer>,
    task: Option<JoinHandle<io::Result<()>>>,
}
impl FakeGateway {
    /// Starts bounded link/history endpoints and a streaming relay over the existing fake model server.
    ///
    /// # Errors
    /// Rejects invalid session input, oversized fixtures and listener/client construction failures.
    pub async fn start(options: FakeGatewayOptions) -> io::Result<Self> {
        Self::start_on(options, Ipv4Addr::LOCALHOST).await
    }
    /// Binds a declared test interface, for a container reaching a host-side fake gateway.
    ///
    /// # Errors
    /// Has the same bounded fixture and listener checks as `start`.
    pub async fn start_on(options: FakeGatewayOptions, address: Ipv4Addr) -> io::Result<Self> {
        if !matches!(&options.session, ClientFrame::SessionConfig { .. })
            || options.token.is_empty()
            || options.token.len() > 4096
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid fake gateway settings",
            ));
        }
        ClientFrame::parse(&serde_json::to_vec(&options.session)?).map_err(io::Error::other)?;
        let bytes = serde_json::to_vec(&options.history)?.len();
        if options.history.len() > MAX_FRAMES || bytes > MAX_BYTES {
            return Err(io::Error::other("fake history limit"));
        }
        let model = FakeModelServer::start(options.scripts, options.sleeper).await?;
        let listener = TcpListener::bind((address, 0)).await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let shared = Arc::new(Shared {
            data: Mutex::new(Data {
                bound: false,
                link_available: true,
                frame_limit: MAX_FRAMES,
                session: options.session,
                stored: options.history,
                frames: vec![],
                requests: vec![],
                first: VecDeque::new(),
                registrations: 0,
                closes: 0,
                actions: None,
                connection: None,
                bytes,
                persist: true,
                channels: None,
                team_role: RoleRoute::Absent,
                decisions: VecDeque::new(),
                stalled: None,
            }),
            hold_close: std::sync::atomic::AtomicBool::new(false),
            bearer: format!("Bearer {}", options.token),
            model_origin: model.base_url().into(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(deadline())
                .build()
                .map_err(|_| io::Error::other("fake relay client"))?,
            cancel: CancellationToken::new(),
            changed: Notify::new(),
            connections: TaskTracker::new(),
            path: Mutex::new(CancellationToken::new()),
        });
        let app = Router::new()
            .route("/runtime/link", get(socket::upgrade))
            .route("/runtime/history", get(http::history))
            .route("/runtime/channels", get(http::channels))
            .route("/runtime/teams/role", get(http::team_role))
            .route("/relay/r/{route}/chat/completions", post(http::relay))
            .route("/relay/r/{route}/responses", post(http::relay))
            .route("/relay/r/{route}/messages", post(http::relay))
            .route("/relay/d/{engine}/v1/{point}", post(http::decision))
            .layer(from_fn_with_state(shared.clone(), path::guard))
            .with_state(shared.clone());
        let cancel = shared.cancel.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(cancel.cancelled_owned())
                .await
        });
        Ok(Self {
            origin,
            shared,
            model: Some(model),
            task: Some(task),
        })
    }
    /// Returns the HTTP origin used by runtime configuration.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }
    /// Returns the WebSocket endpoint for testing the fake's own contract.
    #[must_use]
    pub fn link_url(&self) -> String {
        format!("ws{}/runtime/link", &self.origin[4..])
    }
    /// The conversation's channel memberships that `GET /runtime/channels` lists (ADR-0014); `None` answers 404, as a
    /// gateway without the channel routes does.
    pub fn set_channels(&self, channels: Option<Vec<serde_json::Value>>) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .channels = channels;
    }
    /// The agent-team role `GET /runtime/teams/role` answers (ADR-0034 slice 3); `None` answers 404, as a gateway
    /// without the team routes does.
    pub fn set_team_role(&self, role: Option<&str>) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .team_role = role.map_or(RoleRoute::Absent, |role| RoleRoute::Role(role.to_owned()));
    }
    /// Makes `GET /runtime/teams/role` never answer, as a gateway that hangs.
    pub fn hang_team_role(&self) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .team_role = RoleRoute::Hangs;
    }
    /// The decisions the engine relay answers with, in order (ADR-0013); once they run out it answers 503.
    pub fn set_decisions(&self, decisions: Vec<tqa_types::decision_v1::Response>) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .decisions = decisions.into();
    }
    /// Cuts or restores the runtime's network path, the way a link outage does. While it is down: a model
    /// or engine response already streaming is cut mid-body; every new request fails before response headers; the
    /// registered link goes silent (no pongs and no close frame), so only the runtime's own probe can notice; and its
    /// redials are refused. Restoring it lets the link register again. Actions `send` queues while it is down go to the
    /// silent connection and are dropped with it (and `send` blocks after 64); a test sends nothing while down (TK-U-028).
    pub fn set_path_down(&self, down: bool) {
        {
            let mut path = self
                .shared
                .path
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if down {
                path.cancel();
            } else if path.is_cancelled() {
                *path = CancellationToken::new();
            }
        }
        self.shared.changed.notify_waiters();
    }
    /// Holds or releases the gateway's store, the way a slow journal does on the real gateway (RT-I-033, GW-I-127):
    /// while it is held the link still reads the socket and answers pings, but the runtime's frames wait unrecorded,
    /// in order (neither `frames` nor `stored` shows them); releasing it records them in the order they came.
    pub fn set_store_stalled(&self, stalled: bool) {
        socket::set_stalled(&self.shared, stalled);
    }
    /// Enables or disables token authentication without changing the credential.
    pub fn set_bound(&self, bound: bool) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bound = bound;
    }
    /// Revokes authentication and drops the current connection.
    pub fn revoke(&self) {
        self.set_bound(false);
        self.disconnect();
    }
    /// Drops only the current socket; later registrations still work.
    pub fn disconnect(&self) {
        if let Some(cancel) = &self
            .shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .connection
        {
            cancel.cancel();
        }
    }
    /// Enables link registration, or drops the current link and returns 503 on subsequent redials.
    /// Relay streams stay alive so link availability is independent of model execution.
    pub fn set_link_available(&self, available: bool) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .link_available = available;
        if !available {
            self.disconnect();
        }
    }
    /// Selects an explicit observation bound for large replay tests, up to 131,072 frames.
    ///
    /// # Errors
    /// Rejects zero, oversized limits and limits smaller than the already recorded frame count.
    pub fn set_frame_limit(&self, limit: usize) -> io::Result<()> {
        let mut data = self
            .shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !(1..=MAX_REPLAY_FRAMES).contains(&limit)
            || data.frames.len().max(data.stored.len()) > limit
        {
            return Err(io::Error::other("fake frame limit"));
        }
        data.frame_limit = limit;
        Ok(())
    }
    /// Counts received socket frames without cloning a large replay history.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .frames
            .len()
    }
    /// Changes whether observed sequenced frames are committed, to model a lost gateway acknowledgement.
    pub fn set_persist(&self, persist: bool) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .persist = persist;
    }
    /// Replaces the next registration's snapshot for changed-contract tests.
    ///
    /// # Errors
    /// Rejects anything other than a valid session.config.
    pub fn set_session(&self, session: ClientFrame) -> io::Result<()> {
        if !matches!(&session, ClientFrame::SessionConfig { .. }) {
            return Err(io::Error::other("session required"));
        }
        ClientFrame::parse(&serde_json::to_vec(&session)?).map_err(io::Error::other)?;
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .session = session;
        Ok(())
    }
    /// Injects a first-frame protocol fault for one registration.
    ///
    /// # Errors
    /// Rejects oversized frames or more than 64 queued faults.
    pub fn first_frame(&self, text: String) -> io::Result<()> {
        let mut data = self
            .shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if text.len() > tqa_types::MAX_FRAME_BYTES || data.first.len() >= 64 {
            return Err(io::Error::other("fake fault limit"));
        }
        data.first.push_back(text);
        Ok(())
    }
    /// Sends one typed action to the current registered socket.
    ///
    /// # Errors
    /// Fails when disconnected, stopped or backpressured beyond the control deadline.
    pub async fn send(&self, frame: ClientFrame) -> io::Result<()> {
        self.send_text(serde_json::to_string(&frame).map_err(io::Error::other)?)
            .await
    }
    /// Sends one frame's text verbatim, for frames a newer gateway could send that do not parse
    /// here, such as a route with an unknown protocol.
    ///
    /// # Errors
    /// Fails when disconnected, stopped or backpressured beyond the control deadline.
    pub async fn send_text(&self, text: String) -> io::Result<()> {
        if text.len() > tqa_types::MAX_FRAME_BYTES {
            return Err(io::Error::other("fake frame limit"));
        }
        let sender = self
            .shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .actions
            .clone()
            .ok_or_else(|| io::Error::other("no runtime link"))?;
        tokio::time::timeout(deadline(), sender.send(text))
            .await
            .map_err(io::Error::other)?
            .map_err(|_| io::Error::other("runtime link closed"))
    }
    /// Returns received frames in actual socket order, including controls and rejected sequences.
    #[must_use]
    pub fn frames(&self) -> Vec<ServerFrame> {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .frames
            .clone()
    }
    /// Returns only committed sequenced history.
    #[must_use]
    pub fn stored(&self) -> Vec<ServerFrame> {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stored
            .clone()
    }
    /// Returns credential-free link, history and relay request observations.
    #[must_use]
    pub fn requests(&self) -> Vec<GatewayRequest> {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .requests
            .clone()
    }
    /// Returns model request bodies captured by the reused fake model server.
    #[must_use]
    pub fn model_requests(&self) -> Vec<RecordedRequest> {
        self.model
            .as_ref()
            .map_or_else(Vec::new, FakeModelServer::requests)
    }
    /// Returns the count of authenticated WebSocket registrations.
    #[must_use]
    pub fn registrations(&self) -> usize {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .registrations
    }
    /// Returns the count of WebSocket close frames the runtime sent.
    #[must_use]
    pub fn closes(&self) -> usize {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closes
    }
    /// Holds (or releases) the gateway's Close reply to a runtime that closes the link.
    pub fn hold_close_reply(&self, hold: bool) {
        self.shared
            .hold_close
            .store(hold, std::sync::atomic::Ordering::SeqCst);
        self.shared.changed.notify_waiters();
    }
    /// Waits for at least the given number of received frames.
    ///
    /// # Errors
    /// Fails after the bounded real control deadline; model time remains injected.
    pub async fn wait_for_frames(&self, count: usize) -> io::Result<()> {
        self.wait_for(|data| data.frames.len() >= count).await
    }
    /// Waits for at least the given number of authenticated registrations.
    ///
    /// # Errors
    /// Fails after the bounded real control deadline.
    pub async fn wait_for_registrations(&self, count: usize) -> io::Result<()> {
        self.wait_for(|data| data.registrations >= count).await
    }
    async fn wait_for(&self, ready: impl Fn(&Data) -> bool) -> io::Result<()> {
        tokio::time::timeout(deadline(), self.wait_ready(ready))
            .await
            .map_err(io::Error::other)
    }
    async fn wait_ready(&self, ready: impl Fn(&Data) -> bool) {
        loop {
            let notified = self.shared.changed.notified();
            if self.is_ready(&ready) {
                return;
            }
            notified.await;
        }
    }
    fn is_ready(&self, ready: &impl Fn(&Data) -> bool) -> bool {
        ready(
            &self
                .shared
                .data
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }
    /// Cancels and joins both servers and every upgraded connection.
    ///
    /// # Errors
    /// Reports a server failure or bounded cleanup timeout.
    pub async fn shutdown(mut self) -> io::Result<()> {
        self.shared.cancel.cancel();
        self.disconnect();
        if let Some(model) = self.model.take() {
            model.shutdown().await?;
        }
        self.shared.connections.close();
        tokio::time::timeout(deadline(), self.shared.connections.wait())
            .await
            .map_err(io::Error::other)?;
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(deadline(), &mut task).await {
                Ok(result) => result.map_err(io::Error::other)??,
                Err(error) => {
                    task.abort();
                    let _ = task.await;
                    return Err(io::Error::other(error));
                }
            }
        }
        Ok(())
    }
}
impl Drop for FakeGateway {
    fn drop(&mut self) {
        self.shared.cancel.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn seq(frame: &ServerFrame) -> Option<tqa_types::Seq> {
    match frame {
        ServerFrame::Event { seq, .. } | ServerFrame::PermissionRequest { seq, .. } => Some(*seq),
        ServerFrame::Error { seq, .. } => *seq,
        _ => None,
    }
}
