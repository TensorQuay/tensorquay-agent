//! One proxied connection: identify the sandbox, decide, resolve once, check every address, then tunnel or forward.
use super::{
    Decision, EgressResolver,
    head::{self, MAX_HEAD_BYTES, Target},
    notice::Notices,
};
use crate::app::State;
use std::{
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::TcpStream,
};
use tqa_types::{ConversationId, ErrorCode, ErrorTarget, ServerFrame};

pub(super) struct Proxy {
    pub state: Arc<State>,
    pub resolver: Arc<dyn EgressResolver>,
    pub notices: Notices,
    /// The bounds on keyed connections (ADR-0037 §4).
    pub keyed: super::keyed::Limits,
}

/// An open tunnel: its upstream, and for a CONNECT the caller, host and port.
type Opened = (TcpStream, Option<(ConversationId, String, u16)>);

/// Why a request did not go through; each maps to one fixed response.
enum Refusal {
    Unidentified,
    Denied {
        host: String,
        port: u16,
        reason: &'static str,
    },
    Malformed,
    Unreachable,
}

impl Proxy {
    /// Serves one client connection to completion; failures are answered, never propagated.
    pub(super) async fn connection(&self, mut client: TcpStream, peer: SocketAddr) {
        let deadline = self.state.config.io_timeout();
        let head = match tokio::time::timeout(deadline, read_head(&mut client)).await {
            Ok(Some(head)) => Ok(head),
            Ok(None) => Err(Refusal::Malformed),
            Err(_) => Err(Refusal::Unreachable),
        };
        let outcome = match head {
            Ok(head) => {
                match tokio::time::timeout(deadline, self.open(&mut client, peer.ip(), &head)).await
                {
                    Ok(outcome) => outcome,
                    Err(_) => Err(Refusal::Unreachable),
                }
            }
            Err(refusal) => Err(refusal),
        };
        match outcome {
            Ok((mut upstream, tunnel)) => {
                // A host the caller's running run holds a key for is opened (ADR-0037 §4); any other stays unseen.
                if let Some((conversation, host, port)) = tunnel
                    && let Some(keyed) =
                        super::keyed::keyed(&self.state, &conversation, &host, port).await
                {
                    super::keyed::intercept(
                        self.state.clone(),
                        &self.keyed,
                        client,
                        upstream,
                        keyed,
                    )
                    .await;
                    return;
                }
                // Data flows until either side closes or the tunnel idles out.
                let _ = copy_until_idle(&mut client, &mut upstream, self.state.config.egress.idle)
                    .await;
            }
            Err(refusal) => {
                let status = match refusal {
                    Refusal::Unidentified => "407 Proxy Authentication Required",
                    Refusal::Denied { host, port, reason } => {
                        let response = super::refusal::denied(&host, port, reason);
                        let _ =
                            tokio::time::timeout(deadline, client.write_all(response.as_bytes()))
                                .await;
                        return;
                    }
                    Refusal::Malformed => "400 Bad Request",
                    Refusal::Unreachable => "502 Bad Gateway",
                };
                let response =
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = tokio::time::timeout(deadline, client.write_all(response.as_bytes())).await;
            }
        }
    }

    async fn open(
        &self,
        client: &mut TcpStream,
        peer: IpAddr,
        bytes: &[u8],
    ) -> Result<Opened, Refusal> {
        let head = head::parse(bytes).map_err(|_| Refusal::Malformed)?;
        let conversation = self.identify(peer, head.bearer.as_deref()).await?;
        let (host, port, forward) = match head.target {
            Target::Tunnel { host, port } => (host, port, None),
            Target::Forward { host, port, head } => (host, port, Some(head)),
        };
        // A key never goes over plain http: a placeholder in a forwarded request is refused (ADR-0037 §4).
        if forward.is_some() && holds_placeholder(bytes) {
            return Err(Refusal::Denied {
                host,
                port,
                reason: "a key never goes over plain http",
            });
        }
        let tunnelled = forward.is_none();
        match self.connect(&host, port).await {
            Ok(mut upstream) => {
                if let Some(head) = forward {
                    upstream
                        .write_all(&head)
                        .await
                        .map_err(|_| Refusal::Unreachable)?;
                } else {
                    client
                        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                        .await
                        .map_err(|_| Refusal::Unreachable)?;
                }
                let tunnel = tunnelled.then_some((conversation, host, port));
                Ok((upstream, tunnel))
            }
            Err(Refusal::Denied { host, port, reason }) => {
                self.denied(&conversation, &host, port, reason);
                Err(Refusal::Denied { host, port, reason })
            }
            Err(other) => Err(other),
        }
    }

    async fn identify(
        &self,
        peer: IpAddr,
        bearer: Option<&str>,
    ) -> Result<ConversationId, Refusal> {
        identify(&self.state, peer, bearer)
            .await
            .ok_or(Refusal::Unidentified)
    }

    /// Resolves once, refuses if any answer is guarded, and connects only to the checked addresses (no re-resolution).
    async fn connect(&self, host: &str, port: u16) -> Result<TcpStream, Refusal> {
        let addresses = pinned(&self.state, self.resolver.as_ref(), host, port)
            .await
            .map_err(|refused| match refused {
                Pinning::Denied(reason) => denied(host, port, reason),
                Pinning::Unreachable => Refusal::Unreachable,
            })?;
        for address in addresses {
            // Through the resolver, which tests replace, then the gateway's one socket rule (`net`, GW-U-119).
            if let Ok(stream) = self
                .resolver
                .connect(address)
                .await
                .inspect(crate::net::mark)
            {
                return Ok(stream);
            }
        }
        Err(Refusal::Unreachable)
    }

    fn denied(&self, conversation: &ConversationId, host: &str, port: u16, reason: &'static str) {
        tracing::warn!(conversation_id = %conversation, host, port, decision = "denied", reason, "egress denied");
        if !self
            .notices
            .due(conversation, host, port, std::time::Instant::now())
        {
            return;
        }
        let target = ErrorTarget::new(host, port).ok();
        let notice = ServerFrame::Error {
            seq: None,
            code: ErrorCode::EgressDenied,
            message: format!("Blocked network access to {host}:{port}; {reason}."),
            retryable: false,
            target,
        };
        if let Err(error) = self.state.events.control(conversation, notice) {
            tracing::debug!(code = error.code(), "egress notice could not be published");
        }
    }
}

/// Why a destination may not be used.
pub(crate) enum Pinning {
    Denied(&'static str),
    Unreachable,
}

/// The caller is the sandbox bound to this transport address; a runtime bearer must name the same binding.
/// `None` when it cannot be identified.
pub(crate) async fn identify(
    state: &State,
    peer: IpAddr,
    bearer: Option<&str>,
) -> Option<ConversationId> {
    if let Some(token) = bearer {
        let token = secrecy::SecretString::from(token.to_owned());
        return match state.runtime_tokens.resolve(&token, peer).await {
            Ok(Some((conversation, _))) => Some(conversation),
            _ => None,
        };
    }
    let peer = peer.to_string();
    let bound = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!("SELECT DISTINCT t.conversation_id FROM runtime_tokens t JOIN conversations c ON c.id=t.conversation_id WHERE t.peer_address=$1::text::inet AND t.sandbox_id IS NOT NULL AND (t.expires_at IS NULL OR t.expires_at>CURRENT_TIMESTAMP) AND c.state='active' LIMIT 2", peer)
            .fetch_all(&state.pool)
            .await?)
    })
    .await
    .ok()?;
    // Anything but exactly one live binding is ambiguous: fail closed.
    match bound.as_slice() {
        [conversation] => conversation.to_string().parse().ok(),
        _ => None,
    }
}

/// Decides `host:port` against the allow-list, resolves it once and refuses if any answer is guarded. The caller
/// connects only to the returned, checked addresses (no re-resolution).
pub(crate) async fn pinned(
    state: &State,
    resolver: &dyn EgressResolver,
    host: &str,
    port: u16,
) -> Result<Vec<SocketAddr>, Pinning> {
    if state.config.egress.disabled {
        return Err(Pinning::Denied(super::refusal::DISABLED));
    }
    // Each new connection is decided against the live allow-list (ADR-0028); an open tunnel keeps its own bounds.
    let settings = state.settings.current();
    let Decision::Allowed { ip_literal } = settings.egress.decide(host, port) else {
        return Err(Pinning::Denied(super::refusal::NOT_LISTED));
    };
    // An administrator's internal rule for this exact name (ADR-0035 amendment): its own guard, below.
    let internal = !ip_literal && settings.egress.internal(host, port);
    let addresses = match host.parse::<IpAddr>() {
        Ok(address) => vec![SocketAddr::new(address, port)],
        Err(_) => resolver
            .resolve(host, port)
            .await
            .map_err(|_| Pinning::Unreachable)?,
    };
    if addresses.is_empty() {
        return Err(Pinning::Unreachable);
    }
    if internal {
        // Every address this one resolution gave, and only these are connected to: a re-pointed name can't move it.
        let answer: Vec<IpAddr> = addresses.iter().map(SocketAddr::ip).collect();
        state
            .internal
            .verdict(&answer, std::time::Instant::now())
            .map_err(Pinning::Denied)?;
    } else if addresses
        .iter()
        .any(|address| super::guarded(address.ip(), ip_literal))
    {
        return Err(Pinning::Denied(super::refusal::PRIVATE));
    }
    Ok(addresses)
}

fn denied(host: &str, port: u16, reason: &'static str) -> Refusal {
    Refusal::Denied {
        host: host.to_owned(),
        port,
        reason,
    }
}

/// Reads up to the blank line that ends the head, bounded by `MAX_HEAD_BYTES`. Bytes after it are not expected:
/// a tunnel waits for our 200, and a forwarded request carries no body the proxy must read first.
async fn read_head(client: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::with_capacity(1024);
    let mut byte = [0_u8; 1];
    while head.len() < MAX_HEAD_BYTES {
        if client.read(&mut byte).await.ok()? == 0 {
            return None;
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            return Some(head);
        }
    }
    None
}

/// Copies both ways until either side closes, or until nothing has moved in either direction for `idle`
/// (`[egress] idle_seconds`), which ends it with `TimedOut` (GW-U-206). Every byte read or written moves the
/// deadline, so a long download stays open for as long as it takes; only a silent tunnel is closed.
async fn copy_until_idle<A, B>(a: &mut A, b: &mut B, idle: Duration) -> std::io::Result<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin + ?Sized,
    B: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    let moved = Moved::new();
    let mut a = Active {
        inner: a,
        moved: &moved,
    };
    let mut b = Active {
        inner: b,
        moved: &moved,
    };
    let copy = tokio::io::copy_bidirectional(&mut a, &mut b);
    tokio::pin!(copy);
    loop {
        let deadline = moved.last() + idle;
        tokio::select! {
            // An ended copy wins over a deadline that falls at the same moment.
            biased;
            ended = &mut copy => return ended,
            () = tokio::time::sleep_until(deadline) => {}
        }
        // Bytes that moved while it slept set a later deadline; the loop waits for that one.
        if moved.last() + idle <= tokio::time::Instant::now() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
    }
}

/// When a tunnel last moved a byte, either way: time since its start, shared by its two sides. Atomic so the
/// connection's future stays `Send`; both sides are polled by that one task.
struct Moved {
    start: tokio::time::Instant,
    since: AtomicU64,
}

impl Moved {
    fn new() -> Self {
        Self {
            start: tokio::time::Instant::now(),
            since: AtomicU64::new(0),
        }
    }

    fn touch(&self) {
        let nanos = u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.since.fetch_max(nanos, Ordering::Relaxed);
    }

    fn last(&self) -> tokio::time::Instant {
        self.start + Duration::from_nanos(self.since.load(Ordering::Relaxed))
    }
}

/// One side of a tunnel, noting in the tunnel's [`Moved`] every read or write that moves at least one byte.
struct Active<'a, T: ?Sized> {
    inner: &'a mut T,
    moved: &'a Moved,
}

impl<T: AsyncRead + Unpin + ?Sized> AsyncRead for Active<'_, T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let polled = Pin::new(&mut *this.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            this.moved.touch();
        }
        polled
    }
}

impl<T: AsyncWrite + Unpin + ?Sized> AsyncWrite for Active<'_, T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut *this.inner).poll_write(cx, buf);
        if matches!(polled, Poll::Ready(Ok(written)) if written > 0) {
            this.moved.touch();
        }
        polled
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut *this.inner).poll_write_vectored(cx, bufs);
        if matches!(polled, Poll::Ready(Ok(written)) if written > 0) {
            this.moved.touch();
        }
        polled
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Whether a plain-http request's head holds a placeholder's mark, raw or percent-encoded (ADR-0037 §4). A
/// body after the head streams unread here: only an inert placeholder could leave that way, never a value (ADR-0037
/// §4).
fn holds_placeholder(bytes: &[u8]) -> bool {
    crate::host_keys::marked(bytes)
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod tests;
