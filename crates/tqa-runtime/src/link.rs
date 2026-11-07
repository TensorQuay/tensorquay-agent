//! An outbound-only WebSocket driver; its single task owns all socket writes.
mod gateway_link;
pub(crate) mod history;
mod registration;
mod writer;
use crate::{Journal, RuntimeConfig, RuntimeError, RuntimeLimits, session::Session, time::Time};
use futures::{SinkExt as _, StreamExt as _};
use secrecy::ExposeSecret as _;
use std::{sync::Arc, time::Duration};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest as _, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;
use tqa_llm::Jitter;
use tqa_types::{ClientFrame, Seq, ServerFrame};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub(crate) use gateway_link::LinkHandle;
#[cfg(feature = "testing")]
pub use gateway_link::SETTLE_WAIT;
/// The link as the runner's `GatewayLink` sees it (CORE-U-062): whether it is registered now, and how many times it
/// has registered, so a wait can tell a new registration from the one it started with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LinkState {
    pub up: bool,
    pub registrations: u64,
}
/// A request to prove the registered link with a ping; answered `true` on its pong within `io_timeout`.
pub(crate) type LinkProbe = oneshot::Sender<bool>;
/// The name of a probe's wait for its pong, so a test's fake clock can tell it from the `io_timeout` bounds of real
/// I/O, which last as long (RT-I-033).
pub const PROBE_WAIT: &str = "link.probe";
pub(crate) struct Control {
    pub frame: ServerFrame,
    pub sent: Option<oneshot::Sender<Result<(), RuntimeError>>>,
}
pub(crate) enum LinkEvent {
    Register {
        session: Session,
        watermark: Seq,
        ready: oneshot::Sender<Result<(), RuntimeError>>,
    },
    Action(ClientFrame),
}
pub(crate) struct Link {
    pub config: RuntimeConfig,
    pub limits: RuntimeLimits,
    pub time: Time,
    pub jitter: Box<dyn Jitter>,
    pub journal: Arc<Journal>,
    pub changes: watch::Receiver<Option<Arc<ServerFrame>>>,
    pub controls: mpsc::Receiver<Control>,
    pub actions: mpsc::Sender<LinkEvent>,
    pub shutdown: CancellationToken,
    /// Published for the runner's `GatewayLink`, and the probes it asks for (CORE-U-062).
    pub state: watch::Sender<LinkState>,
    pub probes: mpsc::Receiver<LinkProbe>,
    #[cfg(feature = "testing")]
    pub testing: crate::testing::TestingHooks,
}
enum Dial {
    Socket(Box<Socket>),
    Unauthorized,
    Failed,
}
impl Link {
    pub async fn run(mut self) -> Result<(), RuntimeError> {
        let result = self.connections().await;
        #[cfg(feature = "testing")]
        {
            let finished = self.testing.link_finished.clone();
            drop(self);
            finished.checkpoint(&CancellationToken::new()).await;
        }
        result
    }
    async fn connections(&mut self) -> Result<(), RuntimeError> {
        let started = self.time.clock.now();
        let mut registered = false;
        let mut failures = 0_u32;
        loop {
            if self.shutdown.is_cancelled() {
                return Ok(());
            }
            let remaining = self
                .limits
                .start_timeout
                .saturating_sub(self.time.clock.now().saturating_duration_since(started));
            if !registered && remaining.is_zero() {
                return Err(RuntimeError::StartDeadline);
            }
            let timeout = if registered {
                self.limits.io_timeout
            } else {
                self.limits.io_timeout.min(remaining)
            };
            let Some(accepted) = self.connection(timeout, registered).await? else {
                return Ok(());
            };
            registered |= accepted;
            if accepted {
                failures = 0;
            }
            failures = failures.saturating_add(1);
            let mut delay = self.backoff(failures)?;
            if !registered {
                delay = delay.min(
                    self.limits
                        .start_timeout
                        .saturating_sub(self.time.clock.now().saturating_duration_since(started)),
                );
            }
            tokio::select! {biased;()=self.shutdown.cancelled()=>return Ok(()),()=self.time.sleeper.sleep(delay)=>{}}
            // Offline control frames are ephemeral; durable frames remain exclusively in the WAL.
            while self.controls.try_recv().is_ok() {}
        }
    }
    async fn connection(
        &mut self,
        timeout: Duration,
        registered: bool,
    ) -> Result<Option<bool>, RuntimeError> {
        match self.dial(timeout).await {
            Dial::Unauthorized if registered => Ok(None),
            Dial::Socket(socket) => {
                let result = self.connected(*socket).await;
                self.state.send_modify(|state| state.up = false);
                disconnection(result)
            }
            _ => Ok(Some(false)),
        }
    }
    fn backoff(&mut self, attempt: u32) -> Result<Duration, RuntimeError> {
        let fraction = self.jitter.next_fraction();
        if !fraction.is_finite() || !(0.0..1.0).contains(&fraction) {
            return Err(RuntimeError::Configuration("reconnect jitter"));
        }
        let mut delay = self.limits.reconnect_base;
        for _ in 1..attempt.min(32) {
            delay = delay.saturating_mul(2).min(self.limits.reconnect_cap);
        }
        Ok(delay.mul_f64(fraction))
    }
    async fn dial(&mut self, timeout: Duration) -> Dial {
        let Ok(mut request) = self.config.link_url().as_str().into_client_request() else {
            return Dial::Failed;
        };
        let Ok(mut auth) = format!("Bearer {}", self.config.token.expose_secret())
            .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
        else {
            return Dial::Failed;
        };
        auth.set_sensitive(true);
        request.headers_mut().insert("authorization", auth);
        request.headers_mut().insert(
            tqa_types::PROTOCOL_REVISION_HEADER,
            tqa_types::PROTOCOL_REVISION.into(),
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(tqa_types::MAX_FRAME_BYTES))
            .max_frame_size(Some(tqa_types::MAX_FRAME_BYTES));
        // connect_async connects directly and does not consult proxy variables or follow HTTP redirects.
        match self
            .time
            .wait(
                timeout,
                &self.shutdown,
                Box::pin(tokio_tungstenite::connect_async_with_config(
                    request,
                    Some(config),
                    true,
                )),
            )
            .await
        {
            Ok(Ok((socket, _))) => Dial::Socket(Box::new(socket)),
            Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response)))
                if response.status().as_u16() == 401 =>
            {
                Dial::Unauthorized
            }
            _ => Dial::Failed,
        }
    }
    /// Reads the session, hands it to the worker and waits for its registration; returns the gateway's watermark.
    async fn register(&mut self, socket: &mut Socket) -> Result<Seq, (bool, RuntimeError)> {
        let (session, watermark) = self.configuration(socket).await.map_err(|e| (false, e))?;
        let (ready, receive) = oneshot::channel();
        self.time
            .wait(
                self.limits.io_timeout,
                &self.shutdown,
                self.actions.send(LinkEvent::Register {
                    session,
                    watermark,
                    ready,
                }),
            )
            .await
            .map_err(|e| (false, e))?
            .map_err(|_| (false, RuntimeError::Worker))?;
        tokio::select! {biased;
            ()=self.shutdown.cancelled()=>return Err((false,RuntimeError::Stopped)),
            result=receive=>result.map_err(|_|(true,RuntimeError::Worker))?.map_err(|e|(true,e))?
        };
        Ok(watermark)
    }
    async fn connected(&mut self, mut socket: Socket) -> Result<(), (bool, RuntimeError)> {
        let watermark = self.register(&mut socket).await?;
        self.state.send_modify(|state| {
            state.up = true;
            state.registrations += 1;
        });
        let (mut write, mut read) = socket.split();
        let mut cursor = watermark;
        // A probe in flight: its reply, and the deadline for the pong (CORE-U-062).
        let sleeper = self.time.sleeper.clone();
        let (mut probing, mut deadline): (Option<LinkProbe>, Option<Pending<'_>>) = (None, None);
        let mut probes_closed = false;
        self.replay(&mut write, &mut cursor)
            .await
            .map_err(|e| (true, e))?;
        loop {
            tokio::select! {biased;
                ()=self.shutdown.cancelled()=>{
                    // What the journal published before the stop goes out before the close: the gateway reads the
                    // link in order, so a run's last frames reach it.
                    // Writes wait on this link's stop, which has fired: the flush swaps in a fresh token for its
                    // duration, and one io_timeout bounds the whole flush, so a long backlog can't stretch the stop.
                    let latest=self.changes.borrow_and_update().clone();
                    if let Some(frame)=latest {
                        let stop=std::mem::replace(&mut self.shutdown,CancellationToken::new());
                        let (time,bound)=(self.time.clone(),self.limits.io_timeout);
                        let flushed=time.wait(bound,&CancellationToken::new(),self.deliver(&mut write,&frame,&mut cursor)).await;
                        self.shutdown=stop;
                        if !matches!(flushed,Ok(Ok(()))) {tracing::debug!("durable frames are left for the next session's replay");}
                    }
                    let close=write.send(Message::Close(None));
                    let _=self.time.wait(self.limits.io_timeout,&CancellationToken::new(),close).await;
                    // Then wait (bounded) for the gateway's Close, which it sends only after reading everything before
                    // ours: leaving at once can reset the connection and lose frames the gateway had not read yet.
                    let answered=async {while let Some(Ok(message))=read.next().await {if matches!(message,Message::Close(_)) {break;}}};
                    let _=self.time.wait(self.limits.io_timeout,&CancellationToken::new(),answered).await;
                    return Ok(());
                }
                changed=self.changes.changed()=>{
                    changed.map_err(|_|(true,RuntimeError::Worker))?;
                    let latest=self.changes.borrow_and_update().clone();
                    if let Some(frame)=latest {self.deliver(&mut write,&frame,&mut cursor).await.map_err(|e|(true,e))?;}
                }
                control=self.controls.recv()=>{
                    let Some(control)=control else {return Err((true,RuntimeError::Worker));};
                    let result=self.write_control(&mut write,&control.frame).await;
                    if let Some(sent)=control.sent {let _=sent.send(result.clone());}
                    result.map_err(|e|(true,e))?;
                }
                probe=self.probes.recv(), if probing.is_none() && !probes_closed=>{
                    // No handle left (the runtime is stopping): nothing asks again, so the branch stays off.
                    let Some(reply)=probe else {probes_closed=true; continue;};
                    self.write_message(&mut write,Message::Ping(Vec::new().into())).await.map_err(|e|(true,e))?;
                    probing=Some(reply);
                    deadline=Some(sleeper.sleep_named(self.limits.io_timeout,PROBE_WAIT));
                }
                ()=expiry(&mut deadline)=>{
                    // No pong in time: the path is gone although no read failed yet. The link redials.
                    if let Some(reply)=probing.take() {let _=reply.send(false);}
                    return Err((true,RuntimeError::Link));
                }
                message=read.next()=>match message {
                    Some(Ok(Message::Ping(bytes)))=>{self.write_message(&mut write,Message::Pong(bytes)).await.map_err(|e|(true,e))?;},
                    Some(Ok(Message::Pong(_)))=>{
                        if let Some(reply)=probing.take() {let _=reply.send(true);}
                        deadline=None;
                    },
                    Some(Ok(Message::Text(text)))=>{
                        let frame=ClientFrame::parse(text.as_bytes()).map_err(|_|(true,RuntimeError::Frame))?;
                        self.time.wait(self.limits.io_timeout,&self.shutdown,self.actions.send(LinkEvent::Action(frame))).await
                            .map_err(|e|(true,e))?.map_err(|_|(true,RuntimeError::Worker))?;
                        #[cfg(feature = "testing")]
                        self.testing.after_action.checkpoint(&CancellationToken::new()).await;
                    }
                    _=>return Err((true,RuntimeError::Link)),
                }
            }
        }
    }
}

type Pending<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;

/// A probe's pong deadline, or never when no probe is in flight.
async fn expiry(deadline: &mut Option<Pending<'_>>) {
    match deadline {
        Some(deadline) => deadline.await,
        None => std::future::pending().await,
    }
}

fn disconnection(result: Result<(), (bool, RuntimeError)>) -> Result<Option<bool>, RuntimeError> {
    match result {
        Ok(()) | Err((_, RuntimeError::Stopped)) => Ok(None),
        Err((accepted, RuntimeError::Link | RuntimeError::Frame | RuntimeError::Timeout)) => {
            tracing::warn!("runtime link disconnected; durable frames await replay");
            Ok(Some(accepted))
        }
        Err((_, error)) => Err(error),
    }
}
