//! A bounded wire recorder that keeps synthetic credentials out of assertion diagnostics.

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use std::{
    io,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct Recorded {
    pub headers: HeaderMap,
    pub body: serde_json::Value,
    pub encoded_body: Vec<u8>,
}

struct Shared {
    requests: Mutex<Vec<Recorded>>,
    frames: Vec<Vec<u8>>,
    release: tokio::sync::Notify,
    cancel: CancellationToken,
}

pub struct Upstream {
    shared: Arc<Shared>,
    base_url: String,
    task: Option<tokio::task::JoinHandle<io::Result<()>>>,
}

impl Upstream {
    pub async fn start(text: &str, usage: &str) -> io::Result<Self> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let base_url = format!("http://{}/v1", listener.local_addr()?);
        let first = format!("data: {text}\n\n").into_bytes();
        // Split a multibyte character to exercise transparent byte forwarding.
        let split = first.iter().position(|byte| !byte.is_ascii()).unwrap_or(0) + 1;
        let shared = Arc::new(Shared {
            requests: Mutex::new(Vec::new()),
            frames: vec![
                first[..split].to_vec(),
                first[split..].to_vec(),
                format!("data: {usage}\n\ndata: [DONE]\n\n").into_bytes(),
            ],
            release: tokio::sync::Notify::new(),
            cancel: CancellationToken::new(),
        });
        let app = Router::new()
            .route("/v1/chat/completions", post(receive))
            .with_state(shared.clone());
        let cancel = shared.cancel.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(cancel.cancelled_owned())
                .await
        });
        Ok(Self {
            shared,
            base_url,
            task: Some(task),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn release(&self) {
        self.shared.release.notify_one();
    }

    pub async fn shutdown(mut self) -> io::Result<()> {
        self.shared.cancel.cancel();
        if let Some(task) = &mut self.task {
            tokio::time::timeout(DEADLINE, task)
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)??;
        }
        self.task.take();
        Ok(())
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.shared.cancel.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn receive(State(shared): State<Arc<Shared>>, request: Request) -> Response {
    let (mut parts, body) = request.into_parts();
    let Ok(Ok(bytes)) = tokio::time::timeout(DEADLINE, to_bytes(body, 65_536)).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(body) = serde_json::from_slice(&bytes) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    for value in parts.headers.values_mut() {
        value.set_sensitive(true);
    }
    {
        let mut requests = shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if requests.len() == 8 {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        requests.push(Recorded {
            headers: parts.headers,
            body,
            encoded_body: bytes.to_vec(),
        });
    }
    let stream = futures::stream::unfold((shared, 0), async |(shared, index)| {
        let frame = shared.frames.get(index)?.clone();
        if index == 2 {
            tokio::select! {
                () = shared.cancel.cancelled() => return None,
                result = tokio::time::timeout(DEADLINE, shared.release.notified()) => { result.ok()?; }
            }
        }
        Some((Ok::<_, io::Error>(frame), (shared, index + 1)))
    });
    (
        [("content-type", "text/event-stream")],
        Body::from_stream(stream),
    )
        .into_response()
}
