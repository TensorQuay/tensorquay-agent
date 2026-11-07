//! Stateless, bounded HTTP serving for the test-only standalone model binary.
mod route_check;
mod scenario;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode, header},
    response::{IntoResponse, Json, Response},
};
pub use scenario::Scenario;
use scenario::{ByModel, Engine, Select};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashSet,
    future::IntoFuture,
    io,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_util::sync::CancellationToken;
use tqa_llm::Role;

const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_CONCURRENT_STREAMS: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a held reply sends a keep-alive comment: well inside a client's idle timeout (the provider's is 60 s).
const KEEP_ALIVE: Duration = Duration::from_secs(15);

struct Shared {
    scenario: Scenario,
    /// The hosted provider whose request rules and refusals this endpoint reproduces, if any.
    profile: Option<crate::FakeProfile>,
    cancel: CancellationToken,
    capacity: Arc<Semaphore>,
    /// Scripted replies served so far, for arrival-order selection.
    arrivals: AtomicUsize,
    /// The `once` replies already served, by (the conversation's first user message, reply index) (TK-U-039).
    served_once: Mutex<HashSet<(String, usize)>>,
}

/// Serves immutable scripts until cancellation, then drains within five seconds.
/// Only the model endpoints are served (models, Chat completions and Responses); every request
/// requires a nonempty bearer.
///
/// # Errors
/// Reports listener failure or a shutdown deadline; log failures stop the server.
pub async fn serve(
    listener: TcpListener,
    scenario: Scenario,
    cancel: CancellationToken,
) -> io::Result<()> {
    serve_as(listener, scenario, None, cancel).await
}

/// [`serve`], answering as `profile`'s fake: a request that breaks one of its rules, or uses another protocol's path,
/// gets the provider's refusal before the scenario is consulted (TK-U-018).
///
/// # Errors
/// As [`serve`].
pub async fn serve_as(
    listener: TcpListener,
    scenario: Scenario,
    profile: Option<crate::FakeProfile>,
    cancel: CancellationToken,
) -> io::Result<()> {
    let shared = Arc::new(Shared {
        scenario,
        profile,
        cancel: cancel.clone(),
        capacity: Arc::new(Semaphore::new(MAX_CONCURRENT_STREAMS)),
        arrivals: AtomicUsize::new(0),
        served_once: Mutex::new(HashSet::new()),
    });
    let app = Router::new().fallback(handle).with_state(shared);
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(cancel.clone().cancelled_owned())
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result,
        () = cancel.cancelled() => tokio::time::timeout(IO_TIMEOUT, &mut server).await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "server shutdown deadline"))?,
    }
}

async fn handle(State(shared): State<Arc<Shared>>, request: Request<Body>) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let mut turn = None;
    // Ollama's own endpoints and the scenario's decision engines take no key (TK-U-046, TK-U-048).
    let result = if path.starts_with("/engines/") {
        engine(&shared, request).await
    } else if let Some(answer) = ollama(&shared, request.method(), &path) {
        answer
    } else {
        dispatch(&shared, request, &mut turn)
            .await
            .unwrap_or_else(IntoResponse::into_response)
    };
    let line = format!(
        "{}\n",
        json!({"method":method,"path":path,"turn":turn,"status":result.status().as_u16()})
    );
    // Flushed before the answer goes out: tokio's stderr hands the bytes to a blocking thread, so without the flush a
    // reader of the log (a test, a stack driver) could miss the line for a request it has already had answered.
    let logged = async {
        let mut stderr = tokio::io::stderr();
        stderr.write_all(line.as_bytes()).await?;
        stderr.flush().await
    };
    if !matches!(tokio::time::timeout(IO_TIMEOUT, logged).await, Ok(Ok(()))) {
        shared.cancel.cancel();
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    result
}

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .is_some_and(|(scheme, token)| {
            scheme.eq_ignore_ascii_case("Bearer") && !token.trim().is_empty()
        })
}

async fn dispatch(
    shared: &Arc<Shared>,
    request: Request<Body>,
    turn: &mut Option<usize>,
) -> Result<Response, StatusCode> {
    if !authorized(request.headers()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if request.method() == Method::GET && request.uri().path() == "/v1/models" {
        return Ok(models(shared));
    }
    let path = request.uri().path().to_owned();
    let kind = endpoint(request.method(), &path);
    // A profile answers any other POST path itself, as its provider does.
    if kind.is_none() && (shared.profile.is_none() || request.method() != Method::POST) {
        return Err(StatusCode::NOT_FOUND);
    }
    let permit = shared
        .capacity
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let bytes = tokio::select! {
        () = shared.cancel.cancelled() => return Err(StatusCode::SERVICE_UNAVAILABLE),
        result = tokio::time::timeout(IO_TIMEOUT, to_bytes(request.into_body(), MAX_REQUEST_BYTES)) => {
            result.map_err(|_| StatusCode::REQUEST_TIMEOUT)?.map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?
        }
    };
    if let Some(refusal) = refusal(shared.profile, &path, &bytes)? {
        return Ok(refusal);
    }
    let (responses, messages) = kind.ok_or(StatusCode::NOT_FOUND)?;
    // Each wire's history counts earlier model responses its own way, and says whether it wants usage.
    let (turns, toolless, stream_requested, wire) = if responses {
        let payload: ResponsesRequest =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
        let turns = earlier_responses(&payload.input);
        (
            turns,
            payload.tools.is_empty(),
            payload.stream,
            Wire::Responses,
        )
    } else if messages {
        // Each earlier model response is one assistant turn; the history alternates turns.
        let payload: MessagesRequest =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
        let turns = assistant_turns(&payload.messages);
        (
            turns,
            payload.tools.is_empty(),
            payload.stream,
            Wire::Messages,
        )
    } else {
        let payload: CompletionRequest =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
        let usage = payload
            .stream_options
            .is_some_and(|options| options.include_usage);
        let wire = if usage {
            Wire::ChatWithUsage
        } else {
            Wire::Chat
        };
        let turns = assistant_turns(&payload.messages);
        (turns, payload.tools.is_empty(), payload.stream, wire)
    };
    if !stream_requested {
        return Err(StatusCode::BAD_REQUEST);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    // A model name that fails on request answers first, even a route check (TK-U-040), as does a refused
    // `stream_options` (TK-U-047).
    if let Some(behaviour) = shared.scenario.failing(&body) {
        return failure(shared, behaviour, &body, (turns, wire), permit);
    }
    // The gateway's route check is answered before any selection, so it never uses up or misses a scripted reply.
    if let Some(answer) = route_check::recognise(&body) {
        let reply = answer
            .compile(turns)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(stream(shared, &reply, wire, permit));
    }
    let (served, reply) = choose(shared, turns, toolless, &bytes)?;
    *turn = served;
    Ok(stream_reply(shared, reply, &body, wire, permit))
}

/// Streams `reply`. One with a `seq_of` placeholder takes its seq from this request's history, or is refused loudly,
/// naming the placeholder: never a guessed seq (TK-U-042).
fn stream_reply(
    shared: &Shared,
    reply: &scenario::Response,
    body: &serde_json::Value,
    wire: Wire,
    permit: OwnedSemaphorePermit,
) -> Response {
    let Some(template) = &reply.template else {
        return stream(shared, reply, wire, permit);
    };
    match template.resolve(body) {
        Ok(resolved) => stream(shared, &resolved, wire, permit),
        Err(reason) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": {"type": "scenario_error", "message": reason}})),
        )
            .into_response(),
    }
}

/// A consultation of the scenario's decision engine `<name>`, posted to `/engines/<name>/v1/<point>` (TK-U-048): a
/// stopping engine stops the run; a passing one calls the request's first route with all its tools, and keeps every
/// call of a response. An engine the scenario doesn't name is not found; a request that isn't `decision/v1` is 400.
async fn engine(shared: &Shared, request: Request<Body>) -> Response {
    use tqa_types::{DecisionPoint, decision_v1};
    let path = request.uri().path().to_owned();
    let Some((name, point)) = path
        .strip_prefix("/engines/")
        .and_then(|rest| rest.split_once("/v1/"))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(engine) = shared.scenario.server.engines.get(name).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if request.method() != Method::POST || !matches!(point, "before_step" | "after_response") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(Ok(bytes)) =
        tokio::time::timeout(IO_TIMEOUT, to_bytes(request.into_body(), MAX_REQUEST_BYTES)).await
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(consultation) = serde_json::from_slice::<decision_v1::Request>(&bytes) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let decision = match (engine, consultation.point) {
        (Engine::Stop(reason), _) => decision_v1::Response::Stop { reason },
        (Engine::Pass, DecisionPoint::BeforeStep) => {
            let Some(route) = consultation.routes.first() else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            decision_v1::Response::CallModel {
                route: route.name.clone(),
                tools: consultation
                    .tools
                    .iter()
                    .map(|tool| tool.name.clone())
                    .collect(),
            }
        }
        (Engine::Pass, DecisionPoint::AfterResponse) => decision_v1::Response::Select {
            call_ids: consultation
                .response_calls
                .iter()
                .map(|call| call.id.clone())
                .collect(),
        },
        // A point this protocol version doesn't name: not a consultation this engine answers.
        (Engine::Pass, _) => return StatusCode::BAD_REQUEST.into_response(),
    };
    Json(decision).into_response()
}

/// Ollama's `GET /api/version` and `GET /api/ps`, when the scenario names Ollama models (TK-U-046): the route check
/// reads the loaded model's `context_length` from them.
fn ollama(shared: &Shared, method: &Method, path: &str) -> Option<Response> {
    if shared.scenario.server.ollama.is_empty() || method != Method::GET {
        return None;
    }
    match path {
        "/api/version" => Some(Json(json!({"version": "0.12.0"})).into_response()),
        "/api/ps" => {
            let models: Vec<serde_json::Value> = shared
                .scenario
                .server
                .ollama
                .iter()
                .map(|(name, context)| json!({"name": name, "model": name, "context_length": context}))
                .collect();
            Some(Json(json!({"models": models})).into_response())
        }
        _ => None,
    }
}

/// The model list: the scenario's model, and each model name that fails on request, so List models can pick one.
fn models(shared: &Shared) -> Response {
    let data: Vec<serde_json::Value> = std::iter::once(&shared.scenario.model)
        .chain(shared.scenario.by_model.keys())
        .map(|id| json!({"id": id, "object": "model"}))
        .collect();
    Json(json!({"object": "list", "data": data})).into_response()
}

/// A model name's answer (TK-U-040): its error status as the provider sends one, or its reply for the request's turn,
/// streamed on its wire.
fn failure(
    shared: &Arc<Shared>,
    behaviour: &ByModel,
    body: &serde_json::Value,
    (turns, wire): (usize, Wire),
    permit: OwnedSemaphorePermit,
) -> Result<Response, StatusCode> {
    if let ByModel::Status {
        code,
        body: text,
        retry_after,
    } = behaviour
    {
        let status = StatusCode::from_u16(*code).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut response = (
            status,
            [(header::CONTENT_TYPE, "application/json")],
            text.clone(),
        )
            .into_response();
        if let Some(value) = retry_after.as_deref().and_then(|value| value.parse().ok()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        return Ok(response);
    }
    // The request's first offered tool, on any wire (Chat nests it under `function`).
    let tool = body["tools"][0]["function"]["name"]
        .as_str()
        .or_else(|| body["tools"][0]["name"].as_str())
        .unwrap_or("tool");
    let reply = behaviour
        .reply(tool, turns)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(stream(shared, &reply, wire, permit))
}

/// Which model endpoint a request is for, as (Responses, Messages); `None` for any other.
fn endpoint(method: &Method, path: &str) -> Option<(bool, bool)> {
    match (method, path) {
        (&Method::POST, "/v1/chat/completions") => Some((false, false)),
        (&Method::POST, "/v1/responses") => Some((true, false)),
        (&Method::POST, "/v1/messages") => Some((false, true)),
        _ => None,
    }
}

/// The profile's refusal of a request, if it breaks one of the profile's rules or uses another protocol's path.
fn refusal(
    profile: Option<crate::FakeProfile>,
    path: &str,
    bytes: &[u8],
) -> Result<Option<Response>, StatusCode> {
    let Some(profile) = profile else {
        return Ok(None);
    };
    let body: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(profile.check(path, &body).map(|refusal| {
        let status = StatusCode::from_u16(refusal.status).unwrap_or(StatusCode::BAD_REQUEST);
        (
            status,
            [(header::CONTENT_TYPE, "application/json")],
            refusal.body,
        )
            .into_response()
    }))
}

fn assistant_turns(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .count()
}

/// The reply for one request, and the scripted index it used (none for the summary reply). A request without
/// tools gets the summary reply when the scenario has one; otherwise the scenario's selection applies.
fn choose<'a>(
    shared: &'a Shared,
    history_turns: usize,
    toolless: bool,
    body: &[u8],
) -> Result<(Option<usize>, &'a scenario::Response), StatusCode> {
    let scenario = &shared.scenario;
    if toolless && let Some(summary) = &scenario.summary {
        return Ok((None, summary));
    }
    let index = match scenario.select {
        Select::Turn => history_turns,
        Select::Arrival => shared.arrivals.fetch_add(1, Ordering::Relaxed),
        Select::Prompt => {
            // The replies naming a phrase of this conversation's first user message, by turn; none is a visible
            // refusal, never another conversation's script.
            let body: serde_json::Value =
                serde_json::from_slice(body).map_err(|_| StatusCode::BAD_REQUEST)?;
            let first = first_user_text(&body);
            let own: Vec<usize> = scenario
                .responses
                .iter()
                .enumerate()
                .filter(|(_, reply)| {
                    reply
                        .when
                        .as_deref()
                        .is_some_and(|when| first.contains(when))
                })
                .map(|(index, _)| index)
                .collect();
            let last = own
                .len()
                .checked_sub(1)
                .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
            let position = history_turns.min(last);
            let index = own[position];
            // A `once` reply this conversation already had gives way to the next one naming its phrase (TK-U-039).
            if scenario.responses[index].once {
                let mut served = shared
                    .served_once
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if served.insert((first, index)) {
                    index
                } else {
                    own[(position + 1).min(last)]
                }
            } else {
                index
            }
        }
    };
    let last = scenario.responses.len() - 1;
    Ok((Some(index), &scenario.responses[index.min(last)]))
}

/// The text of a request's first user message, on the Chat and Messages wires (`messages`) and the Responses wire
/// (`input`): a string content, or the text of its parts.
fn first_user_text(body: &serde_json::Value) -> String {
    let items = body["messages"]
        .as_array()
        .or_else(|| body["input"].as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let Some(first) = items.iter().find(|item| item["role"] == "user") else {
        return String::new();
    };
    match &first["content"] {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The earlier model responses in a Responses `input`: each assistant message item starts one, and
/// so does a run of `function_call` items that does not follow an assistant message.
fn earlier_responses(input: &[serde_json::Value]) -> usize {
    let mut count = 0;
    let mut in_response = false;
    for item in input {
        let assistant = item["type"] == "message" && item["role"] == "assistant";
        let call = item["type"] == "function_call";
        if assistant || (call && !in_response) {
            count += 1;
        }
        in_response = assistant || call;
    }
    count
}

#[derive(Deserialize)]
struct MessagesRequest {
    messages: Vec<Message>,
    stream: bool,
    #[serde(default)]
    tools: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ResponsesRequest {
    input: Vec<serde_json::Value>,
    stream: bool,
    #[serde(default)]
    tools: Vec<serde_json::Value>,
}

/// Which precompiled rendering of a response to stream.
#[derive(Clone, Copy)]
enum Wire {
    Chat,
    ChatWithUsage,
    Responses,
    Messages,
}

#[derive(Deserialize)]
struct CompletionRequest {
    messages: Vec<Message>,
    stream: bool,
    stream_options: Option<StreamOptions>,
    #[serde(default)]
    tools: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct Message {
    role: Role,
}
#[derive(Deserialize)]
struct StreamOptions {
    #[serde(default)]
    include_usage: bool,
}

fn stream(
    shared: &Shared,
    response: &scenario::Response,
    wire: Wire,
    permit: OwnedSemaphorePermit,
) -> Response {
    let mut frames = match wire {
        Wire::Chat => response.plain.clone(),
        Wire::ChatWithUsage => response.usage.clone(),
        Wire::Responses => response.responses.clone(),
        Wire::Messages => response.messages.clone(),
    };
    // A Chat stream goes out as the profile's provider streams it, as the in-process fake does (TK-U-018).
    if let Some(profile) = shared
        .profile
        .filter(|_| matches!(wire, Wire::Chat | Wire::ChatWithUsage))
    {
        frames = frames
            .iter()
            .map(|frame| crate::script_wire::Frame {
                bytes: profile.restyle_chat_frame(&frame.bytes).into(),
                delay: frame.delay,
            })
            .collect();
    }
    let delay = response.delay;
    // A held reply (TK-U-035) sends an SSE comment every KEEP_ALIVE until its wait is over, so no client idle timer
    // fires, then its records.
    let state = (frames, 0, response.wait, shared.cancel.clone(), permit);
    let body = futures::stream::unfold(
        state,
        move |(frames, index, wait, cancel, permit)| async move {
            if !wait.is_zero() {
                let step = wait.min(KEEP_ALIVE);
                tokio::select! { biased;
                    () = cancel.cancelled() => return None,
                    () = tokio::time::sleep(step) => {}
                }
                let comment = axum::body::Bytes::from_static(b": held\n\n");
                return Some((
                    Ok::<_, io::Error>(comment),
                    (frames, index, wait.saturating_sub(step), cancel, permit),
                ));
            }
            let frame = frames.get(index)?;
            let delay = if index == 0 { Duration::ZERO } else { delay };
            tokio::select! { biased;
                () = cancel.cancelled() => return None,
                () = tokio::time::sleep(delay) => {}
            }
            Some((
                Ok::<_, io::Error>(frame.bytes.clone()),
                (frames, index + 1, Duration::ZERO, cancel, permit),
            ))
        },
    );
    (
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from_stream(body),
    )
        .into_response()
}
