//! The scripted HTTP fake of the gateway's MCP proxy, shared by the TOOL-I-013 suites (`mcp_gateway.rs` and
//! `mcp_gateway_bounds.rs`): what it answers, what it saw, and the runtime client's side of a call.
#![allow(dead_code)]
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::Write as _,
    sync::{Arc, Mutex},
    time::Duration,
};

use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use tqa_runtime::testing::GatewayServer;
use tqa_tools::ToolError;
use tqa_types::PermissionContext;

pub type TestResult = Result<(), Box<dyn Error>>;
pub const HANDSHAKE: Duration = Duration::from_secs(10);

/// One request the fake proxy received.
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}
/// One scripted answer: status, extra headers, body.
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay: Duration,
    /// Write the body's first `n` bytes, pause, then the rest, so the client sees the split as separate chunks.
    pub split: Option<(usize, Duration)>,
}
/// A refusal the gateway answers itself: its code in `X-TQA-Gateway-Error` as well as the body (GW-I-148).
pub fn refusal(status: u16, code: &str) -> Answer {
    let mut answer = json_answer(status, &json!({"error":{"code":code}}));
    answer
        .headers
        .push((tqa_types::GATEWAY_ERROR_HEADER.into(), code.into()));
    answer
}
pub fn json_answer(status: u16, body: &Value) -> Answer {
    Answer {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.to_string().into_bytes(),
        delay: Duration::ZERO,
        split: None,
    }
}
pub type Script = Arc<dyn Fn(&Seen, usize) -> Answer + Send + Sync>;
/// A scripted answer to a call, given the call's JSON-RPC id.
pub type Answerer = Box<dyn Fn(Value) -> Answer + Send + Sync>;

/// Serves the script until the test ends; returns the origin and the requests seen.
pub async fn proxy(script: Script) -> Result<(url::Url, Arc<Mutex<Vec<Seen>>>), Box<dyn Error>> {
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
/// Answers one connection's request from the script.
pub async fn serve(mut socket: tokio::net::TcpStream, script: Script, log: Arc<Mutex<Vec<Seen>>>) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    let index = {
        let mut log = log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        log.push(request.clone());
        log.len() - 1
    };
    let answer = script(&request, index);
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
    if let Some((first, pause)) = answer.split {
        let first = first.min(answer.body.len());
        let _ = socket.write_all(&answer.body[..first]).await;
        let _ = socket.flush().await;
        tokio::time::sleep(pause).await;
        let _ = socket.write_all(&answer.body[first..]).await;
    } else {
        let _ = socket.write_all(&answer.body).await;
    }
}
pub async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Seen> {
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
    let method = lines.next()?.split_whitespace().next()?.to_owned();
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
        headers,
        body,
    })
}

/// The proxy's handshake answers, then `on_call` for everything else.
pub fn handshake_then(on_call: impl Fn(&Seen, usize) -> Answer + Send + Sync + 'static) -> Script {
    Arc::new(move |seen: &Seen, index| {
        let id = seen.body["id"].clone();
        match seen.body["method"].as_str() {
            Some("initialize") => {
                let mut answer = json_answer(
                    200,
                    &json!({"jsonrpc":"2.0","id":id,"result":{
                    "protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"proxy","version":"1"}}}),
                );
                answer
                    .headers
                    .push(("mcp-session-id".into(), format!("session-{index}")));
                answer
            }
            Some("notifications/initialized" | "notifications/cancelled") => Answer {
                status: 202,
                headers: vec![],
                body: vec![],
                delay: Duration::ZERO,
                split: None,
            },
            Some("tools/list") => json_answer(
                200,
                &json!({"jsonrpc":"2.0","id":id,"result":{"tools":[
                    {"name":"search","description":"Search","inputSchema":{"type":"object"}}]}}),
            ),
            _ => on_call(seen, index),
        }
    })
}
pub fn context() -> Result<PermissionContext, Box<dyn Error>> {
    Ok(serde_json::from_value(
        json!({"conversation_id":"conversation",
        "run_id":"r-7","step":3,"attempt":2,"call_id":"call-9"}),
    )?)
}
pub fn token() -> SecretString {
    SecretString::from("runtime-token")
}
pub async fn call(server: &GatewayServer) -> Result<tqa_runtime::testing::CallOutput, ToolError> {
    server
        .call("search", json!({"q":"x"}), &CancellationToken::new())
        .await
        .map_err(|failure| failure.into_tool_error("tracker", "search"))
}
/// A connector's own text as the model gets it: one untrusted-content block naming the server and the tool (SEC-040).
pub fn marked(body: &str) -> String {
    tqa_tools::untrusted::wrap("mcp", &[("server", "tracker"), ("tool", "search")], body)
        .unwrap_or_default()
}
