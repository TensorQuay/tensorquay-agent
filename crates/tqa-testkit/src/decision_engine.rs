//! A scripted `decision/v1` engine over HTTP (ADR-0013 point 6, TK-U-021), for the runtime's relay adapter and the
//! conformance suite: each consultation takes the script's next answer, and every request is recorded as parsed.
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::post,
};
use std::{
    collections::VecDeque,
    io,
    net::Ipv4Addr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tqa_types::decision_v1;

/// One scripted answer.
#[derive(Clone, Debug)]
pub enum EngineAnswer {
    /// A decision, as JSON with status 200.
    Decide(decision_v1::Response),
    /// Any status and body, for malformed or failed answers.
    Raw {
        /// The HTTP status.
        status: u16,
        /// The body, sent as is.
        body: Vec<u8>,
    },
    /// The inner answer after a delay.
    Delay(Duration, Box<EngineAnswer>),
    /// No answer until the server stops.
    Hang,
}

/// A consultation the fake engine received.
#[derive(Clone, Debug)]
pub struct EngineRequest {
    /// `/v1/before_step` or `/v1/after_response`.
    pub path: String,
    /// The request, parsed as `decision/v1`.
    pub request: decision_v1::Request,
    /// The `Authorization` header, if any.
    pub authorization: Option<String>,
}

struct Shared {
    script: Mutex<VecDeque<EngineAnswer>>,
    requests: Mutex<Vec<EngineRequest>>,
    cancel: CancellationToken,
}

/// The running fake engine.
pub struct FakeEngineServer {
    base_url: String,
    shared: Arc<Shared>,
    task: JoinHandle<io::Result<()>>,
}

impl FakeEngineServer {
    /// Serves `script` on a loopback port; past the script every consultation gets 503.
    ///
    /// # Errors
    /// A failure to bind the port.
    pub async fn start(script: Vec<EngineAnswer>) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let shared = Arc::new(Shared {
            script: Mutex::new(script.into()),
            requests: Mutex::new(Vec::new()),
            cancel: CancellationToken::new(),
        });
        let app = Router::new()
            .route("/v1/before_step", post(consult))
            .route("/v1/after_response", post(consult))
            .with_state(shared.clone());
        let cancel = shared.cancel.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(cancel.cancelled_owned())
                .await
        });
        Ok(Self {
            base_url,
            shared,
            task,
        })
    }
    /// The engine's origin, without a path.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    /// The consultations received so far, in arrival order.
    #[must_use]
    pub fn requests(&self) -> Vec<EngineRequest> {
        self.shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// Stops the server; a hanging consultation is answered 503.
    pub async fn stop(self) {
        self.shared.cancel.cancel();
        let _ = self.task.await;
    }
}

async fn consult(
    State(shared): State<Arc<Shared>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Ok(request) = serde_json::from_slice::<decision_v1::Request>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    shared
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(EngineRequest {
            path: uri.path().to_owned(),
            request,
            authorization: headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        });
    let next = shared
        .script
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pop_front();
    match next {
        Some(answer) => answer_with(&shared.cancel, answer).await,
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn answer_with(cancel: &CancellationToken, mut answer: EngineAnswer) -> Response {
    loop {
        match answer {
            EngineAnswer::Decide(decision) => {
                return (StatusCode::OK, axum::Json(decision)).into_response();
            }
            EngineAnswer::Raw { status, body } => {
                let status =
                    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                return (status, body).into_response();
            }
            EngineAnswer::Delay(delay, inner) => {
                tokio::select! {
                    () = cancel.cancelled() => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
                    () = tokio::time::sleep(delay) => answer = *inner,
                }
            }
            EngineAnswer::Hang => {
                cancel.cancelled().await;
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
    }
}
