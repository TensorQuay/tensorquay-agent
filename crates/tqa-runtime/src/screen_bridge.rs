//! The runtime's side of the screen bridge (ADR-0039 section 5, RT-I-045): for a session whose environment declares
//! `desktop`, a second outbound WebSocket beside the link, `GET /runtime/screen` with the runtime token. The gateway
//! holds each dial as a long poll and upgrades it (`101`) for a viewer, or answers `204` at 30 s and the runtime dials
//! again at once. An upgraded bridge copies bytes both ways between its socket and the VNC server on loopback, without
//! reading or keeping them, on its own task: a slow model call never stalls the picture. One bridge per viewer, at most
//! four, with one spare dial held; while four are up the gateway refuses the spare (`409`) and the runtime waits for one
//! to close. A failed dial backs off (1 s doubling to 30 s, with jitter); a refused token or a conversation without a
//! desktop ends the bridge; the session's end closes every bridge. Nothing in the sandbox listens on a network address.
use crate::{RuntimeConfig, RuntimeError};
use futures::{SinkExt as _, StreamExt as _};
use secrecy::ExposeSecret as _;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
    task::JoinSet,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Message,
        client::IntoClientRequest as _,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};
use tokio_util::sync::CancellationToken;
use tqa_llm::Jitter;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Upgraded bridges at once, as there are at most four viewers.
pub(crate) const MAX_BRIDGES: usize = 4;
/// The most bytes one frame carries towards the gateway.
pub(crate) const CHUNK_BYTES: usize = 64 * 1024;
/// The image's VNC server (`x11vnc -localhost -rfbport 5900`).
pub(crate) const VNC: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 5900);
/// The reason a bridge closes with when the VNC server isn't there; the panel says "The screen isn't available".
pub(crate) const UNAVAILABLE: &str = "screen_unavailable";

/// The bridge's timings, the ADR's by default; a test shortens them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pace {
    /// How long one dial may wait for the gateway's answer: its 30 s hold, with room.
    pub dial: Duration,
    /// Reconnect backoff: the first delay and its cap, doubling between.
    pub backoff_base: Duration,
    pub backoff_cap: Duration,
    /// An upgraded bridge silent this long is pinged, and closed if it is still silent as long again.
    pub idle: Duration,
    /// How long the VNC server may take to accept.
    pub connect: Duration,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            dial: Duration::from_secs(40),
            backoff_base: Duration::from_secs(1),
            backoff_cap: Duration::from_secs(30),
            idle: Duration::from_mins(1),
            connect: Duration::from_secs(5),
        }
    }
}

/// Whether a session's environment wants the bridge: it declares `desktop` (capabilities are declared, never detected).
pub(crate) fn wanted(environment: &tqa_types::Environment) -> bool {
    environment
        .capabilities
        .contains(&tqa_types::Capability::Desktop)
}

/// The screen bridge of one session.
pub(crate) struct ScreenBridge {
    pub config: RuntimeConfig,
    pub pace: Pace,
    pub vnc: SocketAddr,
    pub jitter: Box<dyn Jitter>,
    pub shutdown: CancellationToken,
}

/// How a dial ended.
enum Dialled {
    /// `101`: a viewer's bridge.
    Bridge(Box<Socket>),
    /// `204`: no viewer within the hold; dial again at once.
    NoViewer,
    /// `409`: four bridges are up.
    Full,
    /// `401` or `403`: the token is no longer valid, or the conversation has no desktop. The bridge ends.
    Refused,
    /// Anything else: back off and try again.
    Failed,
}

impl ScreenBridge {
    /// Dials and bridges until the session ends or the gateway refuses the bridge for good.
    pub(crate) async fn run(mut self) {
        let mut bridges = JoinSet::new();
        let mut failures = 0_u32;
        loop {
            while bridges.try_join_next().is_some() {}
            if self.shutdown.is_cancelled() {
                break;
            }
            if bridges.len() >= MAX_BRIDGES {
                tokio::select! {biased;
                    () = self.shutdown.cancelled() => break,
                    _ = bridges.join_next() => continue,
                }
            }
            let dialled = self.dial().await;
            if !self.after(dialled, &mut bridges, &mut failures).await {
                break;
            }
        }
        // The session ended: each bridge's token is a child of the shutdown, so each closes its socket; then wait.
        while bridges.join_next().await.is_some() {}
    }

    /// Acts on a dial's answer; `false` when the bridge ends (refused, or the session's end).
    async fn after(
        &mut self,
        dialled: Dialled,
        bridges: &mut JoinSet<()>,
        failures: &mut u32,
    ) -> bool {
        match dialled {
            Dialled::Bridge(socket) => {
                *failures = 0;
                bridges.spawn(relay(
                    *socket,
                    self.vnc,
                    self.pace,
                    self.shutdown.child_token(),
                ));
                true
            }
            Dialled::NoViewer => {
                *failures = 0;
                true
            }
            Dialled::Refused => false,
            // Four are up as the gateway counts them: the next dial waits for one of ours to close.
            Dialled::Full if !bridges.is_empty() => tokio::select! {biased;
                () = self.shutdown.cancelled() => false,
                _ = bridges.join_next() => true,
            },
            Dialled::Full | Dialled::Failed => {
                *failures = failures.saturating_add(1);
                let Ok(delay) = self.backoff(*failures) else {
                    return false;
                };
                tokio::select! {biased;
                    () = self.shutdown.cancelled() => false,
                    () = tokio::time::sleep(delay) => true,
                }
            }
        }
    }

    /// The delay before the `attempt`th retry: the base doubled per attempt up to the cap, times a jitter in [0, 1).
    fn backoff(&mut self, attempt: u32) -> Result<Duration, RuntimeError> {
        let fraction = self.jitter.next_fraction();
        if !fraction.is_finite() || !(0.0..1.0).contains(&fraction) {
            return Err(RuntimeError::Configuration("screen bridge jitter"));
        }
        let mut delay = self.pace.backoff_base;
        for _ in 1..attempt.min(32) {
            delay = delay.saturating_mul(2).min(self.pace.backoff_cap);
        }
        Ok(delay.mul_f64(fraction))
    }

    async fn dial(&mut self) -> Dialled {
        let Ok(mut request) = self.config.screen_url().as_str().into_client_request() else {
            return Dialled::Failed;
        };
        let Ok(mut auth) = format!("Bearer {}", self.config.token.expose_secret())
            .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
        else {
            return Dialled::Failed;
        };
        auth.set_sensitive(true);
        request.headers_mut().insert("authorization", auth);
        // connect_async connects directly and does not consult proxy variables or follow HTTP redirects.
        let dialled = tokio::select! {biased;
            () = self.shutdown.cancelled() => return Dialled::Failed,
            dialled = tokio::time::timeout(self.pace.dial, tokio_tungstenite::connect_async(request)) => dialled,
        };
        match dialled {
            Ok(Ok((socket, _))) => Dialled::Bridge(Box::new(socket)),
            Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) => {
                match response.status().as_u16() {
                    204 => Dialled::NoViewer,
                    409 => Dialled::Full,
                    401 | 403 => Dialled::Refused,
                    _ => Dialled::Failed,
                }
            }
            _ => Dialled::Failed,
        }
    }
}

/// One viewer's bridge: copies bytes both ways between `socket` and the VNC server until either side ends.
async fn relay(mut socket: Socket, vnc: SocketAddr, pace: Pace, shutdown: CancellationToken) {
    let connected = tokio::time::timeout(pace.connect, TcpStream::connect(vnc)).await;
    let Ok(Ok(stream)) = connected else {
        close(&mut socket, CloseCode::Error, UNAVAILABLE).await;
        return;
    };
    let (mut from_vnc, mut to_vnc) = stream.into_split();
    let mut buffer = vec![0_u8; CHUNK_BYTES];
    let mut pinged = false;
    loop {
        tokio::select! {biased;
            () = shutdown.cancelled() => {
                close(&mut socket, CloseCode::Away, "session_ended").await;
                return;
            }
            read = from_vnc.read(&mut buffer) => match read {
                Ok(0) | Err(_) => {
                    close(&mut socket, CloseCode::Error, UNAVAILABLE).await;
                    return;
                }
                Ok(count) => {
                    let chunk = buffer.get(..count).unwrap_or_default().to_vec();
                    if socket.send(Message::Binary(chunk.into())).await.is_err() {
                        return;
                    }
                }
            },
            message = tokio::time::timeout(pace.idle, socket.next()) => match message {
                Ok(Some(Ok(Message::Binary(bytes)))) => {
                    pinged = false;
                    if to_vnc.write_all(&bytes).await.is_err() {
                        close(&mut socket, CloseCode::Error, UNAVAILABLE).await;
                        return;
                    }
                }
                Ok(Some(Ok(Message::Close(_)) | Err(_)) | None) => return,
                Ok(Some(Ok(_))) => pinged = false,
                // Silent for `idle`: ping once; silent again, the gateway is gone.
                Err(_) if pinged => {
                    close(&mut socket, CloseCode::Away, "unanswered").await;
                    return;
                }
                Err(_) => {
                    pinged = true;
                    if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                        return;
                    }
                }
            },
        }
    }
}

async fn close(socket: &mut Socket, code: CloseCode, reason: &'static str) {
    let _ = socket
        .close(Some(CloseFrame {
            code,
            reason: reason.into(),
        }))
        .await;
}

#[cfg(test)]
#[path = "screen_bridge_tests.rs"]
mod tests;
