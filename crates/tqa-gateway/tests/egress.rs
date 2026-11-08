//! GW-I-012 and SEC-001 (gateway side): the policy proxy admits only allow-listed destinations. GW-I-315: a tunnel
//! closes after `[egress] idle_seconds` without traffic, never while bytes move.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use support::TestResult;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tqa_types::{ErrorCode, ServerFrame};

/// Maps names to scripted answers, one list per call, and counts calls.
struct Resolver {
    answers: Mutex<HashMap<String, Vec<Vec<SocketAddr>>>>,
    calls: Mutex<HashMap<String, usize>>,
}

#[async_trait::async_trait]
impl tqa_gateway::EgressResolver for Resolver {
    async fn resolve(&self, host: &str, _: u16) -> std::io::Result<Vec<SocketAddr>> {
        *self
            .calls
            .lock()
            .map_err(|_| std::io::Error::other("lock"))?
            .entry(host.into())
            .or_default() += 1;
        let mut answers = self
            .answers
            .lock()
            .map_err(|_| std::io::Error::other("lock"))?;
        let queue = answers
            .get_mut(host)
            .ok_or_else(|| std::io::Error::other("unknown name"))?;
        Ok(if queue.len() > 1 {
            queue.remove(0)
        } else {
            queue.first().cloned().unwrap_or_default()
        })
    }
}

/// A plain HTTP upstream that records request lines and answers `ok`.
async fn upstream() -> Result<(SocketAddr, Arc<Mutex<Vec<String>>>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(answer(socket, record.clone()));
        }
    });
    Ok((address, seen))
}

/// Records the request line and answers `ok`.
async fn answer(mut socket: tokio::net::TcpStream, record: Arc<Mutex<Vec<String>>>) {
    let mut buffer = vec![0; 4096];
    let Ok(length) = socket.read(&mut buffer).await else {
        return;
    };
    let text = String::from_utf8_lossy(&buffer[..length]).to_string();
    if let (Ok(mut seen), Some(line)) = (record.lock(), text.lines().next()) {
        seen.push(line.to_owned());
    }
    let _ = socket
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        .await;
}

/// Sends one raw request through the proxy and returns everything it answered.
async fn through(proxy: SocketAddr, request: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut socket = tokio::net::TcpStream::connect(proxy).await?;
    socket.write_all(request.as_bytes()).await?;
    let mut answer = Vec::new();
    tokio::time::timeout(support::DEADLINE, socket.read_to_end(&mut answer)).await??;
    Ok(String::from_utf8(answer)?)
}

fn config(port: u16) -> String {
    format!(
        "[limits]\nio_timeout=2\n[egress]\nlisten='127.0.0.1:3128'\n[[egress.allow]]\nhost='127.0.0.1'\nports=[{port}]\n[[egress.allow]]\nhost='internal.test'\nports=[{port}]\n[[egress.allow]]\nhost='*.wild.test'\nports=[{port}]\n"
    )
}

// TEST: GW-I-012, SEC-006
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_012_allowed_destinations_pass_and_denied_ones_get_403_a_log_and_a_notice(
    pool: PgPool,
) -> TestResult {
    let capture = log_capture::Capture::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .finish(),
    )?;
    let (target, seen) = upstream().await?;
    let port = target.port();
    support::run_configured(pool, None, &config(port), async |f| {
        let resolver = Arc::new(Resolver {
            answers: Mutex::new(HashMap::from([("internal.test".to_owned(), vec![vec![format!("10.0.0.5:{port}").parse()?]])])),
            calls: Mutex::new(HashMap::new()),
        });
        let (proxy, _task) = f.egress(resolver).await?;
        // An address with no live runtime binding cannot use the proxy.
        let unbound = through(proxy, &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\n")).await?;
        assert!(unbound.starts_with("HTTP/1.1 407"), "{unbound}");
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        assert_eq!(
            f.backend.proxy(&conversation.id).as_deref(),
            Some("http://gateway:3128/"),
            "the sandbox is pointed at the proxy"
        );
        let mut stream = f.stream(&conversation.id, &cookie, 0).await?;
        // CONNECT to an allowed host tunnels.
        let tunnel = through(
            proxy,
            &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\nGET /tunnel HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
        )
        .await?;
        assert!(tunnel.starts_with("HTTP/1.1 200 Connection Established\r\n\r\nHTTP/1.1 200 OK"), "{tunnel}");
        // An absolute-form request is forwarded in origin form.
        let forward = through(
            proxy,
            &format!("GET http://127.0.0.1:{port}/plain?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nProxy-Connection: keep-alive\r\n\r\n"),
        )
        .await?;
        assert!(forward.starts_with("HTTP/1.1 200 OK") && forward.ends_with("ok"), "{forward}");
        assert_eq!(
            seen.lock().map_err(|_| "lock")?.clone(),
            ["GET /tunnel HTTP/1.1", "GET /plain?x=1 HTTP/1.1"]
        );
        // A listed host on another port, an unlisted host, an unlisted IP literal and the database's port on the listed
        // host are denied (SEC-006).
        for denied in [
            format!("127.0.0.1:{}", port.wrapping_add(1).max(1)),
            format!("evil.example:{port}"),
            format!("192.0.2.10:{port}"),
            "127.0.0.1:5432".to_owned(),
        ] {
            let answer = through(proxy, &format!("CONNECT {denied} HTTP/1.1\r\n\r\n")).await?;
            assert!(answer.starts_with("HTTP/1.1 403"), "{denied}: {answer}");
        }
        // SEC-001: the denial reaches the transcript as a structured, unsequenced notice. Under load the sandbox's
        // state notice may still come first.
        let mut target = None;
        for _ in 0..8 {
            let (_, frame) = stream.next().await?;
            target = egress_notice(frame)?;
            if target.is_some() {
                break;
            }
        }
        assert_eq!(target, Some(("127.0.0.1".to_owned(), port.wrapping_add(1).max(1))));
        let logs = capture.text()?;
        assert!(
            logs.lines().any(|line| line.contains("WARN")
                && line.contains(conversation.id.as_str())
                && line.contains("evil.example")
                && line.contains("denied")),
            "{logs}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-012, SEC-006
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_012_private_answers_are_refused_and_names_are_resolved_once(
    pool: PgPool,
) -> TestResult {
    let (target, _) = upstream().await?;
    let port = target.port();
    support::run_configured(pool, None, &config(port), async |f| {
        let documentation: SocketAddr = format!("192.0.2.1:{port}").parse()?;
        let resolver = Arc::new(Resolver {
            answers: Mutex::new(HashMap::from([
                // A wildcard-matched name that resolves to loopback or a private address is refused.
                ("loop.wild.test".to_owned(), vec![vec![target]]),
                (
                    "ten.wild.test".to_owned(),
                    vec![vec![format!("10.0.0.7:{port}").parse()?]],
                ),
                // DNS rebinding: a public answer first, loopback on any later lookup.
                (
                    "rebind.wild.test".to_owned(),
                    vec![vec![documentation], vec![target]],
                ),
                (
                    "internal.test".to_owned(),
                    vec![vec![format!("10.0.0.5:{port}").parse()?]],
                ),
            ])),
            calls: Mutex::new(HashMap::new()),
        });
        let (proxy, _task) = f.egress(resolver.clone()).await?;
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        // An exact host-name rule never exempts a reserved answer: DNS may be poisoned (review-045 m1).
        for name in ["loop.wild.test", "ten.wild.test", "internal.test"] {
            let answer = through(proxy, &format!("CONNECT {name}:{port} HTTP/1.1\r\n\r\n")).await?;
            assert!(answer.starts_with("HTTP/1.1 403"), "{name}: {answer}");
        }
        // The checked public answer is used; the proxy never looks the name up again and never reaches loopback.
        let answer = through(
            proxy,
            &format!("CONNECT rebind.wild.test:{port} HTTP/1.1\r\n\r\n"),
        )
        .await?;
        assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
        assert_eq!(
            resolver
                .calls
                .lock()
                .map_err(|_| "lock")?
                .get("rebind.wild.test"),
            Some(&1)
        );
        Ok(())
    })
    .await
}

/// An upstream that sends every byte back as it comes, until the other side closes.
async fn echo() -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut read, mut write) = socket.split();
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
        }
    });
    Ok(address)
}

// TEST: GW-I-315
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_315_a_tunnel_with_traffic_outlives_the_idle_bound_and_an_idle_one_closes(
    pool: PgPool,
) -> TestResult {
    let port = echo().await?.port();
    let config = format!(
        "[limits]\nio_timeout=2\n[egress]\nlisten='127.0.0.1:3128'\nidle_seconds=1\n[[egress.allow]]\nhost='127.0.0.1'\nports=[{port}]\n"
    );
    support::run_configured(pool, None, &config, async |f| {
        let resolver = Arc::new(Resolver {
            answers: Mutex::new(HashMap::new()),
            calls: Mutex::new(HashMap::new()),
        });
        let (proxy, _task) = f.egress(resolver).await?;
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        let mut tunnel = tokio::net::TcpStream::connect(proxy).await?;
        tunnel
            .write_all(format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\n").as_bytes())
            .await?;
        let established = b"HTTP/1.1 200 Connection Established\r\n\r\n";
        let mut answer = vec![0_u8; established.len()];
        tokio::time::timeout(support::DEADLINE, tunnel.read_exact(&mut answer)).await??;
        assert_eq!(answer, established);
        // Three seconds of a byte every quarter second, each echoed: three idle bounds, and the tunnel stays open.
        // Before the fix it was cut one second in, whatever moved (a review before 17.0).
        let mut byte = [0_u8; 1];
        for count in 0..12_u8 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            tunnel.write_all(&[count]).await?;
            tokio::time::timeout(support::DEADLINE, tunnel.read_exact(&mut byte)).await??;
            assert_eq!(byte, [count], "byte {count}");
        }
        // Quiet: the proxy closes the tunnel about one bound later, and not before.
        let quiet = tokio::time::Instant::now();
        let mut rest = Vec::new();
        tokio::time::timeout(support::DEADLINE, tunnel.read_to_end(&mut rest)).await??;
        let waited = quiet.elapsed();
        assert!(rest.is_empty(), "{rest:?}");
        assert!(
            waited >= Duration::from_millis(900),
            "closed {waited:?} after the last byte, before the idle bound"
        );
        Ok(())
    })
    .await
}

/// A frame read while waiting for the egress notice: a sandbox state notice is skipped (`None`), the unsequenced egress
/// notice gives its target, and anything else fails.
fn egress_notice(frame: ServerFrame) -> Result<Option<(String, u16)>, String> {
    match frame {
        ServerFrame::SessionReady { .. } => Ok(None),
        ServerFrame::Error {
            seq: None,
            code: ErrorCode::EgressDenied,
            target: Some(target),
            ..
        } => Ok(Some((target.host().to_owned(), target.port()))),
        other => Err(format!("expected an egress notice, received {other:?}")),
    }
}

// TEST: GW-I-012
#[test]
fn gw_i_012_waiting_for_the_egress_notice_skips_only_a_state_notice()
-> Result<(), Box<dyn std::error::Error>> {
    let ready = ServerFrame::SessionReady {
        conversation_id: "conversation_1".parse()?,
        resumed: false,
        sandbox: tqa_types::SandboxSummary::new(
            tqa_types::SandboxBackend::Docker,
            tqa_types::SandboxState::Ready,
        ),
        queue: None,
    };
    assert_eq!(egress_notice(ready), Ok(None));
    let error = |seq: Option<u64>, code: ErrorCode, target: Option<tqa_types::ErrorTarget>| {
        ServerFrame::Error {
            seq: seq.map(Into::into),
            code,
            message: "denied".into(),
            retryable: false,
            target,
        }
    };
    let target = || tqa_types::ErrorTarget::new("127.0.0.1", 8080);
    assert_eq!(
        egress_notice(error(None, ErrorCode::EgressDenied, Some(target()?))),
        Ok(Some(("127.0.0.1".to_owned(), 8080)))
    );
    // Still refused: another error, a sequenced one, or a notice without its target.
    for frame in [
        error(None, ErrorCode::Internal, Some(target()?)),
        error(Some(3), ErrorCode::EgressDenied, Some(target()?)),
        error(None, ErrorCode::EgressDenied, None),
    ] {
        assert!(egress_notice(frame).is_err());
    }
    Ok(())
}
