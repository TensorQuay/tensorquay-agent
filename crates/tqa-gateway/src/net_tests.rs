//! GW-U-119: one rule for the gateway's own TCP sockets: `TCP_NODELAY` on every one it accepts or opens.
//! GW-U-205: keepalive on every one it accepts.
use super::{accept, connect, listener, mark};
use axum::serve::Listener as _;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// TEST: GW-U-119
#[tokio::test]
async fn gw_u_119_a_wrapped_listener_accepts_streams_with_nodelay() -> TestResult {
    let mut wrapped = listener(TcpListener::bind("127.0.0.1:0").await?);
    let address = wrapped.local_addr()?;
    let client = tokio::spawn(TcpStream::connect(address));
    let (accepted, _) = wrapped.accept().await;
    assert!(
        accepted.nodelay()?,
        "an accepted stream carries small writes at once"
    );
    drop(client.await??);
    Ok(())
}

// TEST: GW-U-119
#[tokio::test]
async fn gw_u_119_a_stream_accepted_by_hand_is_marked() -> TestResult {
    let plain = TcpListener::bind("127.0.0.1:0").await?;
    let client = tokio::spawn(TcpStream::connect(plain.local_addr()?));
    let (accepted, _) = plain.accept().await?;
    assert!(
        !accepted.nodelay()?,
        "the operating system's default, before the rule"
    );
    mark(&accepted);
    assert!(accepted.nodelay()?);
    drop(client.await??);
    Ok(())
}

// TEST: GW-U-119
#[tokio::test]
async fn gw_u_119_every_connection_the_gateway_opens_has_nodelay() -> TestResult {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let opened = connect(upstream.local_addr()?).await?;
    assert!(opened.nodelay()?);
    Ok(())
}

/// `SO_KEEPALIVE` and its three parameters, read off a stream.
fn keepalive(stream: &TcpStream) -> std::io::Result<(bool, Duration, Duration, u32)> {
    let socket = socket2::SockRef::from(stream);
    Ok((
        socket.keepalive()?,
        socket.tcp_keepalive_time()?,
        socket.tcp_keepalive_interval()?,
        socket.tcp_keepalive_retries()?,
    ))
}

/// What every accepted stream carries: probes from 60 s idle, every 10 s, 3 unanswered, so about 90 s.
const ACCEPTED: (bool, Duration, Duration, u32) =
    (true, Duration::from_mins(1), Duration::from_secs(10), 3);

// TEST: GW-U-205
#[tokio::test]
async fn gw_u_205_a_wrapped_listener_accepts_streams_with_keepalive() -> TestResult {
    let mut wrapped = listener(TcpListener::bind("127.0.0.1:0").await?);
    let address = wrapped.local_addr()?;
    let client = tokio::spawn(TcpStream::connect(address));
    let (accepted, _) = wrapped.accept().await;
    assert_eq!(
        keepalive(&accepted)?,
        ACCEPTED,
        "a peer that vanishes without a FIN is found and its connection freed"
    );
    drop(client.await??);
    Ok(())
}

// TEST: GW-U-205
#[tokio::test]
async fn gw_u_205_a_stream_accepted_by_hand_has_nodelay_and_keepalive() -> TestResult {
    let plain = TcpListener::bind("127.0.0.1:0").await?;
    let client = tokio::spawn(TcpStream::connect(plain.local_addr()?));
    let (accepted, _) = accept(&plain).await?;
    assert!(
        accepted.nodelay()?,
        "the egress proxy's tunnels carry small writes"
    );
    assert_eq!(
        keepalive(&accepted)?,
        ACCEPTED,
        "and a vanished sandbox's tunnel is freed"
    );
    drop(client.await??);
    Ok(())
}

// TEST: GW-U-205
#[tokio::test]
async fn gw_u_205_connections_the_gateway_opens_keep_the_system_keepalive() -> TestResult {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let opened = connect(upstream.local_addr()?).await?;
    assert!(
        !socket2::SockRef::from(&opened).keepalive()?,
        "the rule is for accepted connections; an opened one has its own client's timeouts"
    );
    Ok(())
}
