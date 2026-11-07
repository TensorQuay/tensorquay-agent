//! RT-I-045 (ADR-0039 section 5): the runtime's screen bridge against a fixture gateway answering `/runtime/screen`
//! with `204`, then `101`; a loopback VNC fixture; the gateway closing the socket; a dropped connection; the VNC
//! fixture gone; four bridges up and the spare refused `409`; a refused token; the session's end.
use super::{MAX_BRIDGES, Pace, ScreenBridge, UNAVAILABLE};
use crate::RuntimeConfig;
use futures::{SinkExt as _, StreamExt as _};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        handshake::server::{Callback, ErrorResponse, Request, Response},
        http::StatusCode,
    },
};
use tokio_util::sync::CancellationToken;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Served = WebSocketStream<TcpStream>;

const TOKEN: &str = "rt-test-token-0123456789";
const WAIT: Duration = Duration::from_secs(10);

/// A jitter of zero: every backoff is immediate, so a test never waits on one.
struct NoJitter;
impl tqa_llm::Jitter for NoJitter {
    fn next_fraction(&mut self) -> f64 {
        0.0
    }
}

/// What the fixture gateway answers each dial with, in order; past the script, `204`.
#[derive(Clone, Copy, Debug)]
enum Answer {
    Status(u16),
    Upgrade,
    /// Accept the TCP connection and drop it before answering.
    Drop,
}

/// One dial the fixture saw: its path and its `Authorization` header.
#[derive(Debug)]
struct Dial {
    path: String,
    authorization: Option<String>,
}

struct Gateway {
    address: SocketAddr,
    dials: mpsc::UnboundedReceiver<Dial>,
    bridges: mpsc::UnboundedReceiver<Served>,
}

/// The fixture gateway's answer to one dial's handshake: it records the dial, then refuses with a status or upgrades.
struct Answering {
    answer: Answer,
    dials: mpsc::UnboundedSender<Dial>,
}

impl Callback for Answering {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let _ = self.dials.send(Dial {
            path: request.uri().path().to_owned(),
            authorization: request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        });
        let Answer::Status(code) = self.answer else {
            return Ok(response);
        };
        let mut refusal = ErrorResponse::new(None);
        *refusal.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::NO_CONTENT);
        Err(refusal)
    }
}

/// Serves one dial with `answer`; an upgraded socket goes to the test.
async fn serve(stream: TcpStream, answering: Answering, bridges: mpsc::UnboundedSender<Served>) {
    if let Ok(socket) = tokio_tungstenite::accept_hdr_async(stream, answering).await {
        let _ = bridges.send(socket);
    }
}

/// A fixture gateway on loopback answering each dial from `script`.
async fn gateway(script: Vec<Answer>) -> Result<Gateway, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (dial_sender, dials) = mpsc::unbounded_channel();
    let (bridge_sender, bridges) = mpsc::unbounded_channel();
    let script = Arc::new(Mutex::new(script.into_iter()));
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let answer = script
                .lock()
                .ok()
                .and_then(|mut answers| answers.next())
                .unwrap_or(Answer::Status(204));
            if matches!(answer, Answer::Drop) {
                drop(stream);
                continue;
            }
            let answering = Answering {
                answer,
                dials: dial_sender.clone(),
            };
            tokio::spawn(serve(stream, answering, bridge_sender.clone()));
        }
    });
    Ok(Gateway {
        address,
        dials,
        bridges,
    })
}

/// A fixture VNC server on loopback: each connection it accepts is handed to the test.
async fn vnc() -> Result<(SocketAddr, mpsc::UnboundedReceiver<TcpStream>), std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let _ = sender.send(stream);
        }
    });
    Ok((address, receiver))
}

/// An address nothing listens on: a listener bound and closed.
async fn nothing() -> Result<SocketAddr, std::io::Error> {
    TcpListener::bind("127.0.0.1:0").await?.local_addr()
}

fn bridge(
    gateway: SocketAddr,
    vnc: SocketAddr,
    shutdown: &CancellationToken,
) -> Result<ScreenBridge, url::ParseError> {
    let directory = std::env::temp_dir();
    Ok(ScreenBridge {
        config: RuntimeConfig {
            origin: url::Url::parse(&format!("http://{gateway}"))?,
            proxy: None,
            token: TOKEN.to_owned().into(),
            state_dir: directory.clone(),
            workspace: directory.clone(),
            skills: directory,
            sync_before_result: false,
        },
        pace: Pace {
            dial: Duration::from_secs(5),
            backoff_base: Duration::from_millis(1),
            backoff_cap: Duration::from_millis(4),
            idle: Duration::from_millis(300),
            connect: Duration::from_secs(1),
        },
        vnc,
        jitter: Box::new(NoJitter),
        shutdown: shutdown.clone(),
    })
}

async fn next<T>(receiver: &mut mpsc::UnboundedReceiver<T>) -> Result<T, &'static str> {
    tokio::time::timeout(WAIT, receiver.recv())
        .await
        .map_err(|_| "timed out")?
        .ok_or("closed")
}

/// The next binary payload on a served bridge, skipping pings.
async fn binary(socket: &mut Served) -> Result<Vec<u8>, &'static str> {
    loop {
        match tokio::time::timeout(WAIT, socket.next())
            .await
            .map_err(|_| "timed out")?
        {
            Some(Ok(Message::Binary(bytes))) => return Ok(bytes.to_vec()),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            other => {
                return Err(if matches!(other, Some(Ok(Message::Close(_)))) {
                    "closed"
                } else {
                    "ended"
                });
            }
        }
    }
}

// TEST: RT-I-045
#[tokio::test]
async fn rt_i_045_after_a_204_the_bridge_dials_again_and_relays_both_ways_once_upgraded()
-> TestResult {
    let mut gateway = gateway(vec![Answer::Status(204), Answer::Upgrade]).await?;
    let (vnc_address, mut vnc_connections) = vnc().await?;
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(bridge(gateway.address, vnc_address, &shutdown)?.run());
    for _ in 0..2 {
        let dial = next(&mut gateway.dials).await?;
        assert_eq!(dial.path, "/runtime/screen");
        assert_eq!(
            dial.authorization.as_deref(),
            Some(format!("Bearer {TOKEN}").as_str())
        );
    }
    let mut socket = next(&mut gateway.bridges).await?;
    let mut server = next(&mut vnc_connections).await?;
    // The VNC server's bytes reach the gateway, in chunks no larger than 64 KiB.
    let picture = vec![7_u8; 100 * 1024];
    server.write_all(&picture).await?;
    let mut received = Vec::new();
    while received.len() < picture.len() {
        let chunk = binary(&mut socket).await?;
        assert!(chunk.len() <= super::CHUNK_BYTES, "{}", chunk.len());
        received.extend(chunk);
    }
    assert_eq!(received, picture);
    // The viewer's bytes reach the VNC server unchanged.
    socket
        .send(Message::Binary(b"RFB 003.008\n".to_vec().into()))
        .await?;
    let mut hello = [0_u8; 12];
    tokio::time::timeout(WAIT, server.read_exact(&mut hello)).await??;
    assert_eq!(&hello, b"RFB 003.008\n");
    // With one bridge up, the spare dial is already held.
    let spare = next(&mut gateway.dials).await?;
    assert_eq!(spare.path, "/runtime/screen");
    shutdown.cancel();
    tokio::time::timeout(WAIT, task).await??;
    Ok(())
}

// TEST: RT-I-045
#[tokio::test]
async fn rt_i_045_the_gateway_closing_a_bridge_closes_its_vnc_connection() -> TestResult {
    let mut gateway = gateway(vec![Answer::Upgrade]).await?;
    let (vnc_address, mut vnc_connections) = vnc().await?;
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(bridge(gateway.address, vnc_address, &shutdown)?.run());
    let mut socket = next(&mut gateway.bridges).await?;
    let mut server = next(&mut vnc_connections).await?;
    socket.close(None).await?;
    let mut rest = Vec::new();
    let read = tokio::time::timeout(WAIT, server.read_to_end(&mut rest)).await??;
    assert_eq!(
        read, 0,
        "the VNC connection ends with the bridge, and nothing more is sent"
    );
    shutdown.cancel();
    tokio::time::timeout(WAIT, task).await??;
    Ok(())
}

// TEST: RT-I-045
#[tokio::test]
async fn rt_i_045_without_a_vnc_server_the_bridge_closes_with_its_reason_and_dials_again()
-> TestResult {
    let mut gateway = gateway(vec![Answer::Upgrade]).await?;
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(bridge(gateway.address, nothing().await?, &shutdown)?.run());
    let mut socket = next(&mut gateway.bridges).await?;
    match tokio::time::timeout(WAIT, socket.next()).await? {
        Some(Ok(Message::Close(Some(frame)))) => assert_eq!(frame.reason.as_str(), UNAVAILABLE),
        other => return Err(format!("expected a close with its reason, got {other:?}").into()),
    }
    let _first = next(&mut gateway.dials).await?;
    let again = next(&mut gateway.dials).await?;
    assert_eq!(again.path, "/runtime/screen");
    shutdown.cancel();
    tokio::time::timeout(WAIT, task).await??;
    Ok(())
}

// TEST: RT-I-045
#[tokio::test]
async fn rt_i_045_a_dropped_dial_is_dialled_again() -> TestResult {
    let mut gateway = gateway(vec![
        Answer::Drop,
        Answer::Drop,
        Answer::Status(500),
        Answer::Upgrade,
    ])
    .await?;
    let (vnc_address, _vnc) = vnc().await?;
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(bridge(gateway.address, vnc_address, &shutdown)?.run());
    let _bridge = next(&mut gateway.bridges).await?;
    shutdown.cancel();
    tokio::time::timeout(WAIT, task).await??;
    Ok(())
}

// TEST: RT-I-045
#[tokio::test]
async fn rt_i_045_at_four_bridges_no_spare_is_dialled_until_one_closes() -> TestResult {
    let mut gateway = gateway(vec![Answer::Upgrade; MAX_BRIDGES + 1]).await?;
    let (vnc_address, _vnc) = vnc().await?;
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(bridge(gateway.address, vnc_address, &shutdown)?.run());
    let mut open = Vec::new();
    for _ in 0..MAX_BRIDGES {
        open.push(next(&mut gateway.bridges).await?);
        let _ = next(&mut gateway.dials).await?;
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(300), gateway.dials.recv())
            .await
            .is_err(),
        "no dial while four bridges are up"
    );
    if let Some(mut first) = open.pop() {
        first.close(None).await?;
    }
    let _fifth = next(&mut gateway.bridges).await?;
    shutdown.cancel();
    tokio::time::timeout(WAIT, task).await??;
    Ok(())
}

// TEST: RT-I-045
#[tokio::test]
async fn rt_i_045_a_spare_refused_409_waits_for_a_bridge_to_close() -> TestResult {
    let mut gateway = gateway(vec![Answer::Upgrade, Answer::Status(409), Answer::Upgrade]).await?;
    let (vnc_address, _vnc) = vnc().await?;
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(bridge(gateway.address, vnc_address, &shutdown)?.run());
    let mut first = next(&mut gateway.bridges).await?;
    let _upgraded = next(&mut gateway.dials).await?;
    let _refused = next(&mut gateway.dials).await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(300), gateway.dials.recv())
            .await
            .is_err(),
        "after a 409, no dial until a bridge closes"
    );
    first.close(None).await?;
    let _second = next(&mut gateway.bridges).await?;
    shutdown.cancel();
    tokio::time::timeout(WAIT, task).await??;
    Ok(())
}

// TEST: RT-I-045
#[tokio::test]
async fn rt_i_045_a_refused_token_or_no_desktop_ends_the_bridge() -> TestResult {
    for code in [401, 403] {
        let gateway = gateway(vec![Answer::Status(code)]).await?;
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(bridge(gateway.address, nothing().await?, &shutdown)?.run());
        tokio::time::timeout(WAIT, task)
            .await
            .map_err(|_| format!("{code}: the bridge kept dialling"))??;
    }
    Ok(())
}

// TEST: RT-I-045
#[tokio::test]
async fn rt_i_045_the_sessions_end_closes_every_bridge_and_a_silent_one_is_pinged() -> TestResult {
    let mut gateway = gateway(vec![Answer::Upgrade]).await?;
    let (vnc_address, _vnc) = vnc().await?;
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(bridge(gateway.address, vnc_address, &shutdown)?.run());
    let mut socket = next(&mut gateway.bridges).await?;
    // Silent past the idle bound: a ping arrives.
    match tokio::time::timeout(WAIT, socket.next()).await? {
        Some(Ok(Message::Ping(_))) => {}
        other => return Err(format!("expected a ping, got {other:?}").into()),
    }
    shutdown.cancel();
    match tokio::time::timeout(WAIT, socket.next()).await? {
        Some(Ok(Message::Close(_)) | Err(_)) | None => {}
        other => return Err(format!("expected the bridge closed, got {other:?}").into()),
    }
    tokio::time::timeout(WAIT, task).await??;
    Ok(())
}

#[test]
fn rt_i_045_only_a_desktop_environment_wants_the_bridge() {
    let mut environment = tqa_types::Environment::default();
    assert!(!super::wanted(&environment));
    environment
        .capabilities
        .push(tqa_types::Capability::Desktop);
    assert!(super::wanted(&environment));
}
