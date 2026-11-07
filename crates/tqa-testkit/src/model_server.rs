//! Owned loopback HTTP server with FIFO response scripts and sanitized request capture.

use crate::strict_line_breaks;
use crate::{
    FakeProfile, Script, Scripted, StrictLineBreaks,
    script_wire::{Frame, render, render_messages, render_responses},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Query, State},
    http::{Request, Response, StatusCode, header},
    routing::{get, post},
};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    net::Ipv4Addr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{net::TcpListener, sync::Notify, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tqa_types::Sleeper;

const MAX_SCRIPTS: usize = 128;
const MAX_RECORDS: usize = 1024;
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

/// One observed request; credentials are never retained in header or query values.
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    /// HTTP method.
    pub method: String,
    /// Path without its query string.
    pub path: String,
    /// Header names and sanitized values; unrecognized values are masked.
    pub headers: BTreeMap<String, String>,
    /// Query names with every value masked.
    pub query: BTreeMap<String, String>,
    /// Parsed request JSON, or null for an empty body.
    pub body: serde_json::Value,
    /// Exact JSON bytes for request-order assertions.
    pub encoded_body: Vec<u8>,
}

struct Data {
    scripts: VecDeque<Script>,
    requests: Vec<RecordedRequest>,
    models: Option<Vec<String>>,
    /// Ollama's own API, when set: `/api/version` and `/api/ps` listing each model with its context.
    ollama: Option<Vec<(String, u32)>>,
}
struct Shared {
    data: Mutex<Data>,
    changed: Notify,
    sleeper: Arc<dyn Sleeper>,
    cancel: CancellationToken,
    profile: Option<FakeProfile>,
    strict_line_breaks: Option<StrictLineBreaks>,
}

/// How a [`FakeModelServer`] departs from a plain scripted endpoint; the default is none.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeModelOptions {
    /// Enforce a provider's request rules (TK-U-016).
    pub profile: Option<FakeProfile>,
    /// Lose line breaks in strict tools' string arguments, as a constraining endpoint does (TK-U-019).
    pub strict_line_breaks: Option<StrictLineBreaks>,
}

/// A local fake model endpoint; dropping it cancels and aborts its owned server task.
pub struct FakeModelServer {
    base_url: String,
    shared: Arc<Shared>,
    task: Option<JoinHandle<io::Result<()>>>,
}

impl FakeModelServer {
    /// Binds an ephemeral loopback port and serves the supplied bounded script queue.
    ///
    /// # Errors
    /// Rejects excessive or invalid scripts and reports listener failures.
    pub async fn start(scripts: Vec<Script>, sleeper: Arc<dyn Sleeper>) -> io::Result<Self> {
        Self::start_with(FakeModelOptions::default(), scripts, sleeper).await
    }
    /// Like [`FakeModelServer::start`], but enforcing a provider's request rules: a request that breaks one is refused
    /// with that provider's status and error body, and consumes no script (TK-U-016).
    ///
    /// # Errors
    /// As [`FakeModelServer::start`].
    pub async fn start_as(
        profile: FakeProfile,
        scripts: Vec<Script>,
        sleeper: Arc<dyn Sleeper>,
    ) -> io::Result<Self> {
        let options = FakeModelOptions {
            profile: Some(profile),
            ..FakeModelOptions::default()
        };
        Self::start_with(options, scripts, sleeper).await
    }
    /// Like [`FakeModelServer::start`], with the given departures from a plain scripted endpoint.
    ///
    /// # Errors
    /// As [`FakeModelServer::start`].
    pub async fn start_with(
        options: FakeModelOptions,
        scripts: Vec<Script>,
        sleeper: Arc<dyn Sleeper>,
    ) -> io::Result<Self> {
        if scripts.len() > MAX_SCRIPTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "too many response scripts",
            ));
        }
        for script in &scripts {
            render(script)?;
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let base_url = format!("http://{}/v1", listener.local_addr()?);
        let shared = Arc::new(Shared {
            data: Mutex::new(Data {
                scripts: scripts.into(),
                requests: Vec::new(),
                models: Some(vec!["fake-model".into()]),
                ollama: None,
            }),
            changed: Notify::new(),
            sleeper,
            cancel: CancellationToken::new(),
            profile: options.profile,
            strict_line_breaks: options.strict_line_breaks,
        });
        let app = Router::new()
            .route("/v1/chat/completions", post(handle))
            .route("/v1/responses", post(handle))
            .route("/v1/messages", post(handle))
            .route("/v1/models", get(handle))
            .route("/api/version", get(ollama_version))
            .route("/api/ps", get(ollama_loaded))
            .with_state(shared.clone());
        let cancel = shared.cancel.clone();
        // TCP_NODELAY on every connection, as a real model endpoint answers: without it the small last write of a
        // streamed reply waits out the client's delayed ACK (40 ms on Linux) on a kept-alive connection (TK-I-007).
        // Best effort: a loopback socket doesn't refuse it, and TK-I-007 shows the stall if it ever did.
        let listener = axum::serve::ListenerExt::tap_io(listener, |stream| {
            stream.set_nodelay(true).ok();
        });
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(cancel.cancelled_owned())
                .await
        });
        Ok(Self {
            base_url,
            shared,
            task: Some(task),
        })
    }
    /// Returns the endpoint prefix, including its version path.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    /// Returns sanitized request snapshots in arrival order.
    #[must_use]
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .requests
            .clone()
    }
    /// Configures model discovery; None makes the endpoint return 404.
    pub fn set_models(&self, models: Option<Vec<String>>) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .models = models;
    }
    /// Answers as Ollama does besides its OpenAI-compatible API: `/api/version`, and `/api/ps` listing each `(name,
    /// context length)` as a loaded model. None (the default) makes both return 404, as any other server does.
    pub fn set_ollama(&self, loaded: Option<Vec<(String, u32)>>) {
        self.shared
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ollama = loaded;
    }
    /// Waits for a request count without polling or sleeping in real time.
    ///
    /// # Errors
    /// Reports a five-second control deadline if the expected request never arrives.
    pub async fn wait_for_requests(&self, count: usize) -> io::Result<()> {
        tokio::time::timeout(CONTROL_TIMEOUT, async {
            while self
                .shared
                .data
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .requests
                .len()
                < count
            {
                self.shared.changed.notified().await;
            }
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "request observation timed out"))
    }
    /// Cancels pending scripted delays and joins the server task.
    ///
    /// # Errors
    /// Reports listener/task failure or a bounded shutdown timeout.
    pub async fn shutdown(mut self) -> io::Result<()> {
        self.shared.cancel.cancel();
        if let Some(mut task) = self.task.take() {
            if let Ok(result) = tokio::time::timeout(CONTROL_TIMEOUT, &mut task).await {
                result.map_err(io::Error::other)??;
            } else {
                task.abort();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "server shutdown timed out",
                ));
            }
        }
        Ok(())
    }
}

impl Drop for FakeModelServer {
    fn drop(&mut self) {
        self.shared.cancel.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

fn response(status: StatusCode, body: impl Into<Body>) -> Response<Body> {
    let mut response = Response::new(body.into());
    *response.status_mut() = status;
    response
}

async fn record(
    shared: &Shared,
    request: Request<Body>,
) -> Result<serde_json::Value, Response<Body>> {
    let (parts, body) = request.into_parts();
    let bytes = tokio::select! { biased;
        () = shared.cancel.cancelled() => return Err(response(StatusCode::SERVICE_UNAVAILABLE, "server stopping")),
        bytes = tokio::time::timeout(CONTROL_TIMEOUT, to_bytes(body, MAX_REQUEST_BYTES)) => bytes.map_err(|_| response(StatusCode::REQUEST_TIMEOUT, "request deadline"))?.map_err(|_| response(StatusCode::PAYLOAD_TOO_LARGE, "request body limit"))?,
    };
    let body = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .map_err(|_| response(StatusCode::BAD_REQUEST, "invalid request JSON"))?
    };
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| {
            let text = if matches!(
                name.as_str(),
                // The protocol version header is a constant, never a credential.
                "accept" | "anthropic-version" | "content-type" | "host" | "user-agent"
            ) {
                tqa_llm::redact(value.to_str().unwrap_or("[redacted]"))
            } else {
                "[redacted]".into()
            };
            (name.to_string(), text)
        })
        .collect();
    let query = Query::<BTreeMap<String, String>>::try_from_uri(&parts.uri)
        .map_err(|_| response(StatusCode::BAD_REQUEST, "invalid query"))?
        .0
        .into_keys()
        .map(|key| (key, "[redacted]".into()))
        .collect();
    let mut data = shared.data.lock().unwrap_or_else(PoisonError::into_inner);
    if data.requests.len() >= MAX_RECORDS {
        return Err(response(
            StatusCode::TOO_MANY_REQUESTS,
            "request record limit",
        ));
    }
    data.requests.push(RecordedRequest {
        method: parts.method.to_string(),
        path: parts.uri.path().into(),
        headers,
        query,
        body: body.clone(),
        encoded_body: bytes.to_vec(),
    });
    drop(data);
    shared.changed.notify_one();
    Ok(body)
}

/// The scripted status response, with its `Retry-After` and any scripted headers; `None` streams the script.
fn status_response(script: &Script) -> Option<Response<Body>> {
    let (code, body, retry_after) = script.events.iter().find_map(|event| match event {
        Scripted::Status {
            code,
            body,
            retry_after,
        } => Some((*code, body, retry_after)),
        _ => None,
    })?;
    let mut result = response(
        StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        body.clone(),
    );
    if let Some(value) = retry_after {
        let Ok(value) = header::HeaderValue::from_str(value) else {
            return Some(response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid scripted header",
            ));
        };
        result.headers_mut().insert(header::RETRY_AFTER, value);
    }
    Some(with_scripted_headers(script, result))
}

/// `result` with the script's `Header` events set on it, over any header it has; an invalid one is a fixture error.
fn with_scripted_headers(script: &Script, mut result: Response<Body>) -> Response<Body> {
    let headers = script.events.iter().filter_map(|event| match event {
        Scripted::Header(name, value) => Some((name.as_str(), value.as_str())),
        _ => None,
    });
    for (name, value) in headers {
        let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(name.as_bytes()),
            header::HeaderValue::from_str(value),
        ) else {
            return response(StatusCode::INTERNAL_SERVER_ERROR, "invalid scripted header");
        };
        result.headers_mut().insert(name, value);
    }
    result
}

async fn handle(State(shared): State<Arc<Shared>>, request: Request<Body>) -> Response<Body> {
    let models = request.uri().path() == "/v1/models";
    let responses = request.uri().path() == "/v1/responses";
    let messages = request.uri().path() == "/v1/messages";
    let path = request.uri().path().to_owned();
    let body = match record(&shared, request).await {
        Ok(body) => body,
        Err(error) => return error,
    };
    if let Some(refusal) = shared
        .profile
        .filter(|_| !models)
        .and_then(|profile| profile.check(&path, &body))
    {
        let mut result = response(
            StatusCode::from_u16(refusal.status).unwrap_or(StatusCode::BAD_REQUEST),
            refusal.body,
        );
        result.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        return result;
    }
    if models {
        let data = shared.data.lock().unwrap_or_else(PoisonError::into_inner);
        return data.models.as_ref().map_or_else(|| response(StatusCode::NOT_FOUND, "models unavailable"), |models| response(StatusCode::OK, serde_json::json!({"data":models.iter().map(|id| serde_json::json!({"id":id})).collect::<Vec<_>>()} ).to_string()));
    }
    let script = shared
        .data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .scripts
        .pop_front();
    let Some(mut script) = script else {
        return response(StatusCode::SERVICE_UNAVAILABLE, "script queue exhausted");
    };
    if let Some(mode) = shared.strict_line_breaks.filter(|_| !messages) {
        script = strict_line_breaks::apply(mode, &strict_line_breaks::strict_tools(&body), &script);
    }
    if !script.header_delay.is_zero() {
        tokio::select! { biased; () = shared.cancel.cancelled() => return response(StatusCode::SERVICE_UNAVAILABLE, "server stopping"), () = shared.sleeper.sleep(script.header_delay) => {} }
    }
    if let Some(result) = status_response(&script) {
        return result;
    }
    let rendered = if responses {
        render_responses(&script)
    } else if messages {
        render_messages(&script)
    } else {
        render(&script)
    };
    let Ok(mut frames) = rendered else {
        return response(StatusCode::INTERNAL_SERVER_ERROR, "invalid response script");
    };
    if let Some(profile) = shared.profile.filter(|_| !responses && !messages) {
        for frame in &mut frames {
            frame.bytes = profile.restyle_chat_frame(&frame.bytes).into();
        }
    }
    let state = (VecDeque::<Frame>::from(frames), shared);
    let stream = futures::stream::unfold(state, |(mut frames, shared)| async move {
        let frame = frames.pop_front()?;
        if shared.cancel.is_cancelled() {
            return None;
        }
        if !frame.delay.is_zero() {
            tokio::select! { biased; () = shared.cancel.cancelled() => return None, () = shared.sleeper.sleep(frame.delay) => {} }
        }
        Some((Ok::<_, io::Error>(frame.bytes), (frames, shared)))
    });
    let mut result = response(StatusCode::OK, Body::from_stream(stream));
    result.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/event-stream"),
    );
    // A scripted header may relabel the stream, as some endpoints do (GW-U-062).
    with_scripted_headers(&script, result)
}

/// Ollama's `/api/version`, when the server answers as Ollama.
async fn ollama_version(State(shared): State<Arc<Shared>>) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let ollama = shared
        .data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .ollama
        .is_some();
    if ollama {
        axum::Json(serde_json::json!({"version": "0.13.0"})).into_response()
    } else {
        axum::http::StatusCode::NOT_FOUND.into_response()
    }
}

/// Ollama's `/api/ps`: each loaded model with its context, as `ollama ps` shows it.
async fn ollama_loaded(State(shared): State<Arc<Shared>>) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let loaded = shared
        .data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .ollama
        .clone();
    match loaded {
        Some(loaded) => axum::Json(serde_json::json!({"models": loaded.iter().map(|(name, context)| serde_json::json!({
            "name": name, "model": name, "size": 0, "digest": "", "expires_at": "2026-10-06T00:00:00Z",
            "size_vram": 0, "context_length": context,
        })).collect::<Vec<_>>()}))
        .into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}
