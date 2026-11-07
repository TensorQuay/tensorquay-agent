//! A scripted typed-question service over HTTP (TK-U-029), for the typed-question engine adapter (ADR-0013, the adapter
//! amendment). It serves any path the engine's address names: each call takes the script's next answer, keyed by the
//! request's first question, and every request is recorded as sent. Past the script it answers 503.
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io,
    net::Ipv4Addr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

/// One scripted answer.
#[derive(Clone, Debug)]
pub enum QuestionAnswer {
    /// A choice with its confidence and the call's (input, output) tokens, as the service answers with status 200.
    Choose {
        /// The option chosen.
        choice: String,
        /// Its confidence, 0 to 1.
        confidence: f64,
        /// The (input, output) tokens the service reports for the call.
        tokens: (u64, u64),
    },
    /// Any status and body, for failures and malformed answers.
    Raw {
        /// The HTTP status.
        status: u16,
        /// The body, sent as is.
        body: Vec<u8>,
    },
    /// The inner answer after a delay.
    Delay(Duration, Box<QuestionAnswer>),
    /// No answer until the server stops.
    Hang,
}

/// A call the fake service received.
#[derive(Clone, Debug)]
pub struct QuestionRequest {
    /// The path the engine's address named.
    pub path: String,
    /// The request body, as JSON.
    pub body: Value,
    /// The `Authorization` header, if any.
    pub authorization: Option<String>,
}

struct Shared {
    script: Mutex<VecDeque<QuestionAnswer>>,
    requests: Mutex<Vec<QuestionRequest>>,
    cancel: CancellationToken,
}

/// The running fake service.
pub struct FakeQuestionServer {
    base_url: String,
    shared: Arc<Shared>,
    task: JoinHandle<io::Result<()>>,
}

impl FakeQuestionServer {
    /// Serves `script` on a loopback port, at any path.
    ///
    /// # Errors
    /// A failure to bind the port.
    pub async fn start(script: Vec<QuestionAnswer>) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let shared = Arc::new(Shared {
            script: Mutex::new(script.into()),
            requests: Mutex::new(Vec::new()),
            cancel: CancellationToken::new(),
        });
        let app = Router::new().fallback(ask).with_state(shared.clone());
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
    /// The service's origin, without a path.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    /// The calls received so far, in arrival order.
    #[must_use]
    pub fn requests(&self) -> Vec<QuestionRequest> {
        self.shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// Stops the server; a hanging call is answered 503.
    pub async fn stop(self) {
        self.shared.cancel.cancel();
        let _ = self.task.await;
    }
}

async fn ask(
    State(shared): State<Arc<Shared>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Ok(request) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    let model = request["model"].clone();
    let question = request["questions"]
        .as_object()
        .and_then(|questions| questions.keys().next().cloned())
        .unwrap_or_default();
    shared
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(QuestionRequest {
            path: uri.path().to_owned(),
            body: request,
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
    let Some(mut answer) = next else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    loop {
        match answer {
            QuestionAnswer::Choose {
                choice,
                confidence,
                tokens: (input, output),
            } => {
                let body = json!({"model": model,
                    "answers": {question: {"type": "choice", "choice": choice,
                        "probabilities": {choice.clone(): confidence}, "confidence": confidence}},
                    "usage": {"input_tokens": input, "output_tokens": output}});
                return (StatusCode::OK, axum::Json(body)).into_response();
            }
            QuestionAnswer::Raw { status, body } => {
                let status =
                    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                return (status, body).into_response();
            }
            QuestionAnswer::Delay(delay, inner) => {
                tokio::select! {
                    () = shared.cancel.cancelled() => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
                    () = tokio::time::sleep(delay) => answer = *inner,
                }
            }
            QuestionAnswer::Hang => {
                shared.cancel.cancelled().await;
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
    }
}
