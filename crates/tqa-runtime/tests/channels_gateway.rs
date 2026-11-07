//! TOOL-I-014: the runtime's client for the gateway's channel routes (ADR-0014 point 6, the channels API contract).
//! Paths, query and headers as the gateway expects them; the runtime token as the bearer; the tool call's idempotency
//! key on posts and claims; a claim's 201 or 200; the gateway's error envelope and codes; an unreachable gateway; a
//! channel id that could leave the route; no redirects; a bounded body; a long poll that ends promptly on cancel. The
//! gateway is a scripted HTTP fake. TOOL-U-036: a page, a wait answer or the memberships list that carries what a
//! later gateway may send (a message type, a rule, a field this runtime doesn't know) still reads (ADR-0034 point 12a).
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::Write as _,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use tqa_runtime::testing::GatewayChannels;
use tqa_tools::builtin::channel::{ChannelClient, PageMessage, ReadRequest};
use tqa_types::{ChannelFilter, ChannelPost, MessageBody, MessageType};

type TestResult = Result<(), Box<dyn Error>>;
const TOKEN: &str = "runtime-token-marker";

/// One request the fake gateway received.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: Value,
}
/// One scripted answer.
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
}
fn answer(status: u16, body: &Value) -> Answer {
    Answer {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.to_string().into_bytes(),
        delay: Duration::ZERO,
    }
}
type Script = Arc<dyn Fn(&Seen) -> Answer + Send + Sync>;

/// Serves the script; returns the origin and the requests seen.
async fn gateway(script: Script) -> Result<(url::Url, Arc<Mutex<Vec<Seen>>>), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let origin = url::Url::parse(&format!("http://{}", listener.local_addr()?))?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve(socket, script.clone(), log.clone()));
        }
    });
    Ok((origin, seen))
}
async fn serve(mut socket: tokio::net::TcpStream, script: Script, log: Arc<Mutex<Vec<Seen>>>) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(request.clone());
    let answer = script(&request);
    tokio::time::sleep(answer.delay).await;
    let headers = answer
        .headers
        .iter()
        .fold(String::new(), |mut headers, (name, value)| {
            let _ = write!(headers, "{name}: {value}\r\n");
            headers
        });
    let head = format!(
        "HTTP/1.1 {} X\r\nconnection: close\r\ncontent-length: {}\r\n{headers}\r\n",
        answer.status,
        answer.body.len()
    );
    let _ = socket.write_all(head.as_bytes()).await;
    let _ = socket.write_all(&answer.body).await;
}
async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let (method, target) = (first.next()?.to_owned(), first.next()?.to_owned());
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + length {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = serde_json::from_slice(&buffer[header_end..]).unwrap_or(Value::Null);
    Some(Seen {
        method,
        target,
        headers,
        body,
    })
}

fn message(seq: u64, body: &Value) -> Value {
    json!({"seq":seq,"ts":"2026-09-27T20:00:00Z","author":{"kind":"conversation","id":"c-1","name":"Coder"},"body":body})
}

fn client(origin: &url::Url) -> Result<GatewayChannels, Box<dyn Error>> {
    Ok(GatewayChannels::new(
        origin,
        &SecretString::from(TOKEN.to_owned()),
    )?)
}

fn seen(log: &Arc<Mutex<Vec<Seen>>>) -> Vec<Seen> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tokio::test]
async fn tool_i_014_reads_and_waits_with_the_contracts_query() -> TestResult {
    let page = json!({"messages":[message(5, &json!({"type":"text","text":"hi"}))],"next_since":5,"more":true});
    let (origin, log) = gateway(Arc::new(move |_| answer(200, &page))).await?;
    let client = client(&origin)?;
    let request = ReadRequest {
        since: 4,
        filter: ChannelFilter {
            types: vec![MessageType::Task, MessageType::Report],
            parent: Some(2),
            task_seq: None,
            author_kind: None,
        },
        limit: 10,
        wait: None,
    };
    let page = client
        .read("ch-1", &request, &CancellationToken::new())
        .await
        .map_err(|failure| failure.code)?;
    assert_eq!(
        (page.messages.len(), page.next_since, page.more),
        (1, 5, true)
    );
    let waited = ReadRequest {
        wait: Some(Duration::from_mins(2)),
        ..ReadRequest::default()
    };
    client
        .read("ch-1", &waited, &CancellationToken::new())
        .await
        .map_err(|failure| failure.code)?;
    let seen = seen(&log);
    assert_eq!(
        (seen[0].method.as_str(), seen[0].target.as_str()),
        (
            "GET",
            "/runtime/channels/ch-1/messages?since=4&limit=10&types=task,report&parent=2"
        )
    );
    assert_eq!(seen[0].headers["authorization"], format!("Bearer {TOKEN}"));
    assert_eq!(
        seen[1].target,
        "/runtime/channels/ch-1/messages?since=0&limit=0&wait=120"
    );
    Ok(())
}

#[tokio::test]
async fn tool_i_014_posts_and_claims_carry_the_calls_key() -> TestResult {
    let (origin, log) = gateway(Arc::new(|seen: &Seen| {
        match (seen.method.as_str(), seen.target.as_str()) {
            ("POST", "/runtime/channels/ch-1/messages") => {
                answer(201, &message(16, &seen.body["body"]))
            }
            ("POST", "/runtime/channels/ch-1/tasks/7/claim")
                if seen
                    .headers
                    .get("idempotency-key")
                    .is_some_and(|key| key == "r-1:call-8") =>
            {
                answer(200, &message(17, &json!({"type":"claim","task_seq":7})))
            }
            ("POST", "/runtime/channels/ch-1/tasks/7/claim") => {
                answer(201, &message(17, &json!({"type":"claim","task_seq":7})))
            }
            _ => answer(
                404,
                &json!({"error":{"code":"not_found","message":"no such route"}}),
            ),
        }
    }))
    .await?;
    let client = client(&origin)?;
    let cancel = CancellationToken::new();
    let posted = client
        .post(
            "ch-1",
            &ChannelPost::new(MessageBody::Text {
                text: "ready".into(),
            }),
            "r-1:call-7",
            &cancel,
        )
        .await
        .map_err(|failure| failure.code)?;
    assert_eq!(posted.seq, 16);
    let fresh = client
        .claim("ch-1", 7, "r-1:call-7", &cancel)
        .await
        .map_err(|failure| failure.code)?;
    let again = client
        .claim("ch-1", 7, "r-1:call-8", &cancel)
        .await
        .map_err(|failure| failure.code)?;
    assert_eq!(
        (fresh.existing, again.existing, again.message.seq),
        (false, true, 17)
    );
    client
        .post(
            "ch-1",
            &ChannelPost::new(MessageBody::Done { summary: None }),
            "",
            &cancel,
        )
        .await
        .map_err(|failure| failure.code)?;
    let seen = seen(&log);
    assert_eq!(seen[0].headers["idempotency-key"], "r-1:call-7");
    assert_eq!(seen[0].body, json!({"body":{"type":"text","text":"ready"}}));
    assert!(
        !seen[3].headers.contains_key("idempotency-key"),
        "an empty key sends none"
    );
    Ok(())
}

#[tokio::test]
async fn tool_i_014_the_gateways_codes_and_the_clients_own() -> TestResult {
    let (origin, log) = gateway(Arc::new(|seen: &Seen| match seen.target.as_str() {
        "/runtime/channels" => answer(200, &json!({"channels":[{"id":"ch-1","name":"Refactor","purpose":"Split","role":"member","rule":"majority","state":"open"}]})),
        target if target.starts_with("/runtime/channels/ch-9/") => answer(403, &json!({"error":{"code":"not_member","message":"not a member of this channel"}})),
        target if target.starts_with("/runtime/channels/ch-8/") => answer(400, &json!({"error":{"code":"invalid_request","message":"expected 1 to 300","field":"wait"}})),
        target if target.starts_with("/runtime/channels/ch-7/") => Answer { status: 302, headers: vec![("location".into(), "http://example.invalid/".into())], body: vec![], delay: Duration::ZERO },
        _ => Answer { status: 502, headers: vec![], body: b"<html>bad gateway</html>".to_vec(), delay: Duration::ZERO },
    }))
    .await?;
    let client = client(&origin)?;
    let cancel = CancellationToken::new();
    let listed = client
        .channels(&cancel)
        .await
        .map_err(|failure| failure.code)?;
    assert_eq!((listed.len(), listed[0].role.as_str()), (1, "member"));
    let code = |channel: &'static str| {
        let client = &client;
        let cancel = &cancel;
        async move {
            client
                .read(channel, &ReadRequest::default(), cancel)
                .await
                .err()
                .map(|failure| (failure.code, failure.message))
        }
    };
    assert_eq!(
        code("ch-9").await,
        Some(("not_member".into(), "not a member of this channel".into()))
    );
    assert_eq!(
        code("ch-8").await,
        Some(("invalid_request".into(), "wait: expected 1 to 300".into()))
    );
    assert_eq!(
        code("ch-7").await.map(|(code, _)| code),
        Some("gateway_error".into()),
        "a redirect is not followed"
    );
    assert_eq!(
        code("ch-6").await.map(|(code, _)| code),
        Some("gateway_error".into())
    );
    // A channel id that could leave the route is refused before any request.
    let before = seen(&log).len();
    assert_eq!(
        code("../admin").await.map(|(code, _)| code),
        Some("invalid_request".into())
    );
    assert_eq!(seen(&log).len(), before);
    // Nothing listens: unreachable, which a model may retry.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")?;
    let origin = url::Url::parse(&format!("http://{}", closed.local_addr()?))?;
    drop(closed);
    let failure = GatewayChannels::new(&origin, &SecretString::from(TOKEN.to_owned()))?
        .channels(&cancel)
        .await
        .err()
        .ok_or("reached a closed port")?;
    assert_eq!(failure.code, "gateway_unreachable");
    Ok(())
}

#[tokio::test]
async fn tool_i_014_a_long_poll_ends_on_cancel_and_a_body_is_bounded() -> TestResult {
    let (origin, _) = gateway(Arc::new(|seen: &Seen| {
        if seen.target.contains("wait=") {
            Answer {
                delay: Duration::from_secs(30),
                ..answer(200, &json!({"messages":[],"next_since":0,"more":false}))
            }
        } else {
            let big = "x".repeat(5 * 1024 * 1024);
            answer(
                200,
                &json!({"messages":[],"next_since":0,"more":false,"pad":big}),
            )
        }
    }))
    .await?;
    let client = Arc::new(client(&origin)?);
    let cancel = CancellationToken::new();
    let waiting = {
        let (client, cancel) = (client.clone(), cancel.clone());
        tokio::spawn(async move {
            let request = ReadRequest {
                wait: Some(Duration::from_mins(5)),
                ..ReadRequest::default()
            };
            client.read("ch-1", &request, &cancel).await
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = Instant::now();
    cancel.cancel();
    // The gateway holds the wait's answer 30 s, so a call that ignored the cancel would take that long: a few seconds
    // tells the two apart on any host (200 ms measured a loaded one).
    let result = tokio::time::timeout(Duration::from_secs(10), waiting).await??;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        result.err().map(|failure| failure.code),
        Some("cancelled".into())
    );
    let failure = client
        .read("ch-1", &ReadRequest::default(), &CancellationToken::new())
        .await
        .err()
        .ok_or("accepted a 5 MiB body")?;
    assert_eq!(failure.code, "response_too_large");
    Ok(())
}

#[tokio::test]
async fn tool_u_036_what_a_later_gateway_adds_still_reads() -> TestResult {
    let unknown_type = message(6, &json!({"type":"artifact","name":"plan.md"}));
    let unknown_rule = message(
        7,
        &json!({"type":"decision","proposal_seq":2,"outcome":"accepted","rule":{"quorum":{"n":2}}}),
    );
    let unknown_field = message(8, &json!({"type":"text","text":"hi","lang":"en"}));
    let page = json!({
        "messages": [message(5, &json!({"type":"text","text":"known"})), unknown_type, unknown_rule, unknown_field],
        "next_since": 8, "more": false, "team_run": {"id": "t-1"},
    });
    let listed = json!({"channels": [{
        "id": "ch-1", "name": "Team", "purpose": "", "role": "lead", "rule": "quorum", "state": "open",
        "team_run": {"id": "t-1", "spawn": true},
    }], "revision_hint": 9});
    let (origin, _) = gateway(Arc::new(move |seen: &Seen| {
        if seen.target == "/runtime/channels" {
            answer(200, &listed)
        } else {
            answer(200, &page)
        }
    }))
    .await?;
    let client = client(&origin)?;
    let cancel = CancellationToken::new();
    for wait in [None, Some(Duration::from_secs(1))] {
        let request = ReadRequest {
            wait,
            ..ReadRequest::default()
        };
        let read = client
            .read("ch-1", &request, &cancel)
            .await
            .map_err(|failure| failure.code)?;
        let shapes: Vec<(bool, Option<u64>)> = read
            .messages
            .iter()
            .map(|message| (matches!(message, PageMessage::Known(_)), message.seq()))
            .collect();
        assert_eq!(
            shapes,
            [
                (true, Some(5)),
                (false, Some(6)),
                (false, Some(7)),
                (false, Some(8))
            ],
            "{wait:?}"
        );
        assert_eq!(read.next_since, 8);
    }
    let channels = client
        .channels(&cancel)
        .await
        .map_err(|failure| failure.code)?;
    assert_eq!(
        (channels[0].id.as_str(), channels[0].rule.as_str()),
        ("ch-1", "quorum")
    );
    Ok(())
}
