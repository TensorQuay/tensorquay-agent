//! A loopback HTTP server that replays recorded-shape SSE bodies verbatim and records requests.

use std::{
    error::Error,
    net::Ipv4Addr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

/// One observed request: its path and parsed JSON body.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub path: String,
    /// The request line and headers, lowercased.
    pub head: String,
    pub body: serde_json::Value,
    pub bytes: usize,
}

/// Serves one reply per connection, in order; each reply's body is written in the given pieces
/// and the connection then closes, so a truncated body is a cut stream.
pub struct SseServer {
    pub base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    task: JoinHandle<()>,
}

impl SseServer {
    pub async fn start(bodies: Vec<Vec<String>>) -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let base_url = format!("http://{}/v1", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let task = tokio::spawn(accept(listener, bodies, recorded));
        Ok(Self {
            base_url,
            requests,
            task,
        })
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for SseServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn accept(
    listener: TcpListener,
    bodies: Vec<Vec<String>>,
    recorded: Arc<Mutex<Vec<Recorded>>>,
) {
    for pieces in bodies {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        if serve(stream, &pieces, &recorded).await.is_err() {
            return;
        }
    }
}

async fn serve(
    mut stream: TcpStream,
    pieces: &[String],
    recorded: &Mutex<Vec<Recorded>>,
) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0; 8192];
    let (head_end, length) = loop {
        let count = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await??;
        if count == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..end]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break (end + 4, length);
        }
    };
    while buffer.len() < head_end + length {
        let count = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await??;
        if count == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..count]);
    }
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let path = head
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    let bytes = buffer.len() - head_end;
    let body = serde_json::from_slice(&buffer[head_end..]).unwrap_or(serde_json::Value::Null);
    recorded
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Recorded {
            path,
            head: head.to_ascii_lowercase(),
            body,
            bytes,
        });
    // A reply that starts with a status line is written verbatim, for error statuses.
    if !pieces
        .first()
        .is_some_and(|piece| piece.starts_with("HTTP/"))
    {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await?;
    }
    for piece in pieces {
        stream.write_all(piece.as_bytes()).await?;
        stream.flush().await?;
    }
    stream.shutdown().await
}

/// A recorded-shape fixture file from `tests/fixtures/responses`.
pub fn fixture(name: &str) -> Result<String, Box<dyn Error>> {
    fixture_in("responses", name)
}

/// A recorded-shape fixture file from `tests/fixtures/<protocol>`.
pub fn fixture_in(protocol: &str, name: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(protocol)
            .join(name),
    )?)
}

/// One SSE event with an `event:` line, as the Responses stream frames it.
pub fn event(value: &serde_json::Value) -> String {
    let kind = value["type"].as_str().unwrap_or_default();
    format!("event: {kind}\ndata: {value}\n\n")
}
