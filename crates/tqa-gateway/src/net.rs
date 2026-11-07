//! One rule for the gateway's own TCP sockets (GW-U-119): `TCP_NODELAY` on every connection it accepts or opens. Its
//! traffic is small writes (relayed model stream chunks, client event records, runtime frames, tunnelled bytes), and
//! without it each waits for the peer's ACK of the one before; a peer that delays its ACK (40 ms on Linux) then holds
//! the end of a relayed stream that long (GW-I-181). reqwest clients set it themselves.
//!
//! Every connection it accepts also has TCP keepalive (GW-U-205): the public listener, the metrics listener and the
//! egress proxy. An idle keep-alive connection whose peer vanished without a FIN or RST (a sandbox cut off from its
//! network, then killed) is otherwise held for good, since the HTTP server has no idle or header timer and nothing is
//! ever sent on it (GW-I-314). Probes start after 60 s idle, every 10 s, and 3 unanswered close it: about 90 s.
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

/// A listener whose accepted streams have `TCP_NODELAY` and keepalive, for `axum::serve`.
pub type Listener = axum::serve::TapIo<TcpListener, fn(&mut TcpStream)>;

/// Keepalive on accepted streams: the first probe after 60 s idle, then every 10 s, the connection closed after 3.
const KEEPALIVE: TcpKeepalive = TcpKeepalive::new()
    .with_time(Duration::from_mins(1))
    .with_interval(Duration::from_secs(10))
    .with_retries(3);

/// Wraps a bound listener so every stream it accepts has `TCP_NODELAY` and keepalive.
pub fn listener(listener: TcpListener) -> Listener {
    let tap: fn(&mut TcpStream) = |stream| accepted(stream);
    axum::serve::ListenerExt::tap_io(listener, tap)
}

/// Accepts one connection by hand (the egress proxy), with `TCP_NODELAY` and keepalive. Cancel safe, as the
/// listener's own `accept` is.
pub(crate) async fn accept(
    listener: &TcpListener,
) -> std::io::Result<(TcpStream, std::net::SocketAddr)> {
    let (stream, peer) = listener.accept().await?;
    accepted(&stream);
    Ok((stream, peer))
}

/// The rule for a stream the gateway accepted: `TCP_NODELAY` and keepalive; a refusal is logged, the connection still
/// served.
fn accepted(stream: &TcpStream) {
    mark(stream);
    if let Err(error) = SockRef::from(stream).set_tcp_keepalive(&KEEPALIVE) {
        tracing::warn!(%error, "could not set TCP keepalive on an accepted connection");
    }
}

/// Sets `TCP_NODELAY` on a stream opened by hand; a refusal is logged, the connection still served.
pub(crate) fn mark(stream: &TcpStream) {
    if let Err(error) = stream.set_nodelay(true) {
        tracing::warn!(%error, "could not set TCP_NODELAY on a connection");
    }
}

/// Opens a TCP connection with `TCP_NODELAY`.
pub(crate) async fn connect(address: impl ToSocketAddrs) -> std::io::Result<TcpStream> {
    let stream = TcpStream::connect(address).await?;
    mark(&stream);
    Ok(stream)
}

#[cfg(test)]
#[path = "net_tests.rs"]
mod tests;
