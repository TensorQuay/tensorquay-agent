//! RT-I-068 (18.2.2; the nightly real-stack suite of 25 Nov 2026, where a lead's `done` fell inside a gateway restart
//! and was lost: the scripted lead never called it again, so its team waited for the idle stop). The runtime's team
//! and channel calls go on through a gateway that is restarting: a call that can't reach the gateway, or that the
//! gateway answers 503 or with a retryable error, is sent again with the SAME idempotency key (the gateway answers a
//! repeated key with the first answer), with a growing pause, until the call's retry window ends. A refusal is never
//! retried, and a cancel ends the wait at once. The gateway is a scripted HTTP fake.
use std::{
    collections::BTreeMap,
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use tqa_runtime::testing::{GatewayChannels, GatewayTeams};
use tqa_tools::builtin::{channel::ChannelClient, team::TeamClient};
use tqa_types::{ChannelPost, MessageBody};

type TestResult = Result<(), Box<dyn Error>>;
const TOKEN: &str = "runtime-token-marker";

/// One request the fake gateway received: its method, path and headers.
#[derive(Clone, Debug)]
struct Seen {
    target: String,
    headers: BTreeMap<String, String>,
}
/// The scripted answer to the request numbered by the closure's argument (0 first): a status and a JSON body.
type Script = Arc<dyn Fn(usize) -> (u16, Value) + Send + Sync>;

async fn serve_on(listener: TcpListener, script: Script, log: Arc<Mutex<Vec<Seen>>>) {
    let count = Arc::new(AtomicUsize::new(0));
    while let Ok((mut socket, _)) = listener.accept().await {
        let (script, log, count) = (script.clone(), log.clone(), count.clone());
        tokio::spawn(async move {
            let Some(seen) = read_request(&mut socket).await else {
                return;
            };
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(seen);
            let (status, body) = script(count.fetch_add(1, Ordering::SeqCst));
            let body = body.to_string();
            let head = format!(
                "HTTP/1.1 {status} X\r\nconnection: close\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(body.as_bytes()).await;
        });
    }
}

async fn gateway(script: Script) -> Result<(url::Url, Arc<Mutex<Vec<Seen>>>), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let origin = url::Url::parse(&format!("http://{}", listener.local_addr()?))?;
    let log = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn(serve_on(listener, script, log.clone()));
    Ok((origin, log))
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let end = loop {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
    let mut lines = head.lines();
    let target = lines.next()?.split_whitespace().nth(1)?.to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    while buffer.len() < end + length {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Some(Seen { target, headers })
}

fn seen(log: &Arc<Mutex<Vec<Seen>>>) -> Vec<Seen> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn token() -> SecretString {
    SecretString::from(TOKEN.to_owned())
}

/// The gateway's error envelope.
fn error(code: &str, retryable: bool) -> Value {
    json!({"error": {"code": code, "message": "a scripted answer", "retryable": retryable}})
}

const DONE: (&str, &str) = ("call-key-1", "run-1");

// TEST: RT-I-068
#[tokio::test]
async fn rt_i_068_a_leads_done_goes_on_through_a_gateway_that_is_restarting() -> TestResult {
    // Twice unavailable (the gateway coming back from a restart), then the first answer to the key.
    let (origin, log) = gateway(Arc::new(|n| match n {
        0 => (503, error("store_unavailable", true)),
        1 => (503, json!({})),
        _ => (200, json!({"ended": true})),
    }))
    .await?;
    let teams = GatewayTeams::new(&origin, &token())?.with_retry_window(Duration::from_secs(20));
    let answer = teams
        .done(
            &json!({"summary": "the report is in"}),
            DONE,
            &CancellationToken::new(),
        )
        .await
        .map_err(|failure| failure.code)?;
    assert_eq!(answer, json!({"ended": true}));
    let calls = seen(&log);
    assert_eq!(calls.len(), 3, "sent until the gateway answered");
    for call in &calls {
        assert_eq!(call.target, "/runtime/teams/done");
        assert_eq!(
            call.headers.get("idempotency-key").map(String::as_str),
            Some(DONE.0),
            "the same key each time"
        );
        assert_eq!(
            call.headers.get("x-tqa-run-id").map(String::as_str),
            Some(DONE.1)
        );
    }
    Ok(())
}

// TEST: RT-I-068
#[tokio::test]
async fn rt_i_068_a_gateway_that_is_down_for_a_moment_is_reached_when_it_is_back() -> TestResult {
    // The gateway's port answers nothing (it is restarting), then the gateway listens there again.
    let probe = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = probe.local_addr()?;
    drop(probe);
    let origin = url::Url::parse(&format!("http://{address}"))?;
    let log = Arc::new(Mutex::new(Vec::new()));
    let back = log.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1200)).await;
        if let Ok(listener) = TcpListener::bind(address).await {
            serve_on(listener, Arc::new(|_| (200, json!({"ended": true}))), back).await;
        }
    });
    let teams = GatewayTeams::new(&origin, &token())?.with_retry_window(Duration::from_secs(20));
    let started = Instant::now();
    let answer = teams
        .done(
            &json!({"summary": "the report is in"}),
            DONE,
            &CancellationToken::new(),
        )
        .await
        .map_err(|failure| failure.code)?;
    assert_eq!(answer, json!({"ended": true}));
    assert!(
        started.elapsed() >= Duration::from_millis(1200),
        "it waited for the gateway"
    );
    assert_eq!(
        seen(&log).len(),
        1,
        "one call reached the gateway once it was back"
    );
    Ok(())
}

// TEST: RT-I-068
#[tokio::test]
async fn rt_i_068_a_refusal_is_answered_at_once_and_never_sent_again() -> TestResult {
    let (origin, log) = gateway(Arc::new(|_| (409, error("team_ended", false)))).await?;
    let teams = GatewayTeams::new(&origin, &token())?.with_retry_window(Duration::from_secs(20));
    let started = Instant::now();
    let failure = teams
        .done(&json!({"summary": "late"}), DONE, &CancellationToken::new())
        .await
        .err()
        .ok_or("a refusal was answered as a success")?;
    assert_eq!(failure.code, "team_ended");
    assert_eq!(seen(&log).len(), 1);
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

// TEST: RT-I-068
#[tokio::test]
async fn rt_i_068_a_cancel_ends_the_wait_for_the_gateway_at_once() -> TestResult {
    let (origin, _) = gateway(Arc::new(|_| (503, error("store_unavailable", true)))).await?;
    let teams = GatewayTeams::new(&origin, &token())?.with_retry_window(Duration::from_mins(1));
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        stop.cancel();
    });
    let started = Instant::now();
    let failure = teams
        .done(&json!({"summary": "the report is in"}), DONE, &cancel)
        .await
        .err()
        .ok_or("answered through a gateway that never came back")?;
    assert_eq!(failure.code, "cancelled");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the cancel ended the wait"
    );
    Ok(())
}

// TEST: RT-I-068
#[tokio::test]
async fn rt_i_068_past_its_window_the_call_says_what_the_gateway_said() -> TestResult {
    let (origin, log) = gateway(Arc::new(|_| (503, error("store_unavailable", true)))).await?;
    let teams =
        GatewayTeams::new(&origin, &token())?.with_retry_window(Duration::from_millis(1500));
    let failure = teams
        .done(
            &json!({"summary": "the report is in"}),
            DONE,
            &CancellationToken::new(),
        )
        .await
        .err()
        .ok_or("answered through a gateway that never came back")?;
    assert_eq!(failure.code, "store_unavailable");
    assert!(seen(&log).len() >= 2, "it was sent again within its window");
    Ok(())
}

// TEST: RT-I-068
#[tokio::test]
async fn rt_i_068_a_channel_post_goes_on_through_a_gateway_that_is_restarting() -> TestResult {
    let (origin, log) = gateway(Arc::new(|n| match n {
        0 => (503, error("store_unavailable", true)),
        _ => (
            201,
            json!({"seq": 7, "ts": "2026-11-27T10:00:00Z", "author": {"kind": "conversation", "id": "c-1", "name": "Coder"},
                   "body": {"type": "text", "text": "the totals are in"}}),
        ),
    }))
    .await?;
    let channels =
        GatewayChannels::new(&origin, &token())?.with_retry_window(Duration::from_secs(20));
    let post = ChannelPost::new(MessageBody::Text {
        text: "the totals are in".into(),
    });
    let posted = channels
        .post("ch-1", &post, "post-key-1", &CancellationToken::new())
        .await
        .map_err(|failure| failure.code)?;
    assert_eq!(posted.seq, 7);
    let calls = seen(&log);
    assert_eq!(calls.len(), 2);
    assert!(
        calls.iter().all(
            |call| call.headers.get("idempotency-key").map(String::as_str) == Some("post-key-1")
        )
    );
    Ok(())
}
