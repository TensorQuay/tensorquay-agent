//! The MCP proxy (ADR-0031): `POST /mcp/{connector}` on the main listener. Sandbox code can read its runtime token, so
//! everything that must hold is checked here, on every request: the caller's token and address, the conversation's
//! connector set, the method allow-list and the tool filter. The proxy injects the credential (the user's own, else
//! the shared one), reaches the remote only at checked addresses without following redirects, and answers within its
//! bounds. The sandbox never sees a credential, and never the remote's session id (`sessions`). `GET` opens the
//! session's server stream and `DELETE` ends the session.
pub(crate) mod jsonrpc;
pub(crate) mod rate;
mod session_calls;
pub(crate) mod sessions;
pub(crate) mod sse;

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, State as AppState},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use secrecy::ExposeSecret;

use crate::{
    ConnectorFailure, EgressResolver, GatewayError, InvalidReason,
    app::State,
    connectors::{ConnectorDocument, CredentialSource, Scheme, credentials},
};
use jsonrpc::{MAX_MESSAGE_BYTES, Unusable, Verdict};
pub(crate) use session_calls::{close, listen};

/// Connecting to a remote server, TLS included.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The outer bound on one proxied request: the longest connector call timeout (600 s), connecting, and a margin.
pub(crate) const DEADLINE: Duration = Duration::from_secs(620);

/// Calls in flight through the proxy as a whole.
const MAX_IN_FLIGHT: usize = 256;
/// Calls in flight from one conversation to one connector (the stdio servers' bound, ADR-0011).
const MAX_IN_FLIGHT_PER_CONNECTOR: usize = 4;
/// A JSON-RPC server error (implementation-defined range), for an answer the proxy had to cut off.
const ANSWER_CUT_OFF: i64 = -32000;

/// The proxy's own state: the resolver for connector hosts (the system's; tests inject theirs) and the in-flight
/// bounds.
pub(crate) struct McpProxy {
    resolver: RwLock<Arc<dyn EgressResolver>>,
    in_flight: Arc<Semaphore>,
    per_connector: Mutex<HashMap<(uuid::Uuid, String), Arc<Semaphore>>>,
    sessions: Arc<Mutex<sessions::Sessions>>,
    rate: Mutex<rate::Rate>,
}

impl Default for McpProxy {
    fn default() -> Self {
        Self {
            resolver: RwLock::new(Arc::new(crate::SystemResolver)),
            in_flight: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
            per_connector: Mutex::new(HashMap::new()),
            sessions: Arc::new(Mutex::new(sessions::Sessions::default())),
            rate: Mutex::new(rate::Rate::default()),
        }
    }
}

/// Held while a call is in flight, a streamed answer included.
pub(crate) struct Permits {
    _in_flight: OwnedSemaphorePermit,
    _connector: OwnedSemaphorePermit,
}

impl McpProxy {
    pub(crate) fn set_resolver(&self, resolver: Arc<dyn EgressResolver>) {
        *self
            .resolver
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = resolver;
    }

    /// Takes one of the conversation's slots for this connector and one of the proxy's, or refuses at once.
    fn permits(&self, conversation: uuid::Uuid, connector: &str) -> Result<Permits, GatewayError> {
        let busy = || failure(ConnectorFailure::Busy);
        let one = {
            let mut map = self
                .per_connector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // Forget idle entries so the map only holds conversations with calls in flight.
            map.retain(|_, semaphore| {
                Arc::strong_count(semaphore) > 1
                    || semaphore.available_permits() < MAX_IN_FLIGHT_PER_CONNECTOR
            });
            Arc::clone(
                map.entry((conversation, connector.to_owned()))
                    .or_insert_with(|| Arc::new(Semaphore::new(MAX_IN_FLIGHT_PER_CONNECTOR))),
            )
        };
        let connector = one.try_acquire_owned().map_err(|_| busy())?;
        let in_flight = Arc::clone(&self.in_flight)
            .try_acquire_owned()
            .map_err(|_| busy())?;
        Ok(Permits {
            _in_flight: in_flight,
            _connector: connector,
        })
    }

    /// Charges `owner` one call per message against the effective `mcp.calls_per_minute`, read at each call so an
    /// administrator's change applies from the next one.
    fn charge(&self, owner: uuid::Uuid, cost: usize, per_minute: u32) -> Result<(), GatewayError> {
        self.rate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .admit(owner, cost, per_minute, tokio::time::Instant::now())
            .map_err(GatewayError::RateLimited)
    }

    fn resolver(&self) -> Arc<dyn EgressResolver> {
        Arc::clone(
            &self
                .resolver
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

fn failure(failure: ConnectorFailure) -> GatewayError {
    GatewayError::Connector(failure)
}

/// `POST /mcp/{connector}`: one JSON-RPC message or batch from a runtime, to one connector.
///
/// # Errors
/// `401` without a live runtime token from its sandbox's address, `403 connector_not_allowed`, `400`/`413` for the
/// body, `429 rate_limited` past the owner's `mcp.calls_per_minute`, `424 connector_credential_missing`, `424
/// connector_credential_rejected` when the remote refuses the person's own credential, `502`/`504` for the remote,
/// `503` when a store is unavailable.
pub(crate) async fn call(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let (conversation, owner) = admitted(&state, &headers, peer.ip(), &name).await?;
    let connector = state
        .settings
        .current()
        .connectors
        .iter()
        .find(|document| document.name == name && document.enabled)
        .cloned()
        .ok_or(failure(ConnectorFailure::NotAllowed))?;
    let messages = jsonrpc::parse(&body).map_err(|unusable| match unusable {
        Unusable::TooLarge => GatewayError::BodyTooLarge,
        Unusable::Invalid => GatewayError::Invalid {
            field: "body",
            reason: InvalidReason::BodyInvalid,
        },
    })?;
    let method = messages.label();
    let outcome = relay_call(
        &state,
        (conversation, owner, &name),
        &connector,
        &headers,
        (messages, body),
    )
    .await;
    let label = match &outcome {
        Ok(response) if response.status().is_success() => "ok",
        Ok(_) => "remote_error",
        Err(error) => error.code(),
    };
    state.telemetry.mcp_proxy_request(&name, method, label);
    outcome
}

/// The checked call: the local refusals, the credential, the session and the permits, then the remote.
async fn relay_call(
    state: &Arc<State>,
    (conversation, owner, name): (uuid::Uuid, uuid::Uuid, &str),
    connector: &ConnectorDocument,
    headers: &HeaderMap,
    (messages, body): (jsonrpc::Messages, Bytes),
) -> Result<Response, GatewayError> {
    let method = messages.label();

    if let Some(refused) = refused_here(state, owner, connector, &messages)? {
        return Ok(refused);
    }
    let credential = credentials::resolve(state, owner, name, Some(conversation)).await?;
    if credential.is_none() && connector.auth.required {
        return Err(failure(ConnectorFailure::CredentialMissing));
    }
    let source = credential.as_ref().map(|credential| credential.source);
    let session = session_of(state, headers, (conversation, name), source)?;
    let permits = state.mcp.permits(conversation, name)?;
    let outbound = outbound_headers(headers, session.as_ref().map(|(_, remote)| remote.as_str()));
    let cancel = CancelOnDrop::arm(
        state,
        connector,
        &messages,
        session.as_ref().map(|(_, remote)| remote.clone()),
        credential.clone(),
    );
    let answer = forward(
        state,
        connector,
        (reqwest::Method::POST, &outbound, Some(body)),
        credential,
    )
    .await;
    if let Err(error) = &answer {
        tracing::warn!(
            connector = %name,
            method,
            code = error.code(),
            credential = source_label(source),
            "connector call failed"
        );
    }
    let answer = answer?;
    let opaque = bind_session(
        state,
        session.map(|(opaque, _)| opaque),
        answer.headers(),
        (conversation, name),
        source,
    )?;
    let list_ids = messages.tools_list_ids();
    let mut response = relay_answer(connector, answer, list_ids, (permits, cancel));
    set_session_header(&mut response, opaque.as_deref());
    Ok(response)
}

/// The remote's answer for the runtime: a JSON answer filtered at once, or an event stream relayed event by event while
/// the permits and the cancel guard stay held.
fn relay_answer(
    connector: &ConnectorDocument,
    answer: Upstream,
    list_ids: Vec<serde_json::Value>,
    (permits, mut cancel): (Permits, CancelOnDrop),
) -> Response {
    match answer {
        Upstream::Json(status, answer_headers, mut answer) => {
            cancel.disarm();
            if let Some(answer) = answer.as_mut() {
                jsonrpc::filter_answer(answer, &list_ids, connector.tools.as_ref());
            }
            respond(status, &answer_headers, answer.as_ref())
        }
        Upstream::Stream(status, answer_headers, response) => stream(
            (status, &answer_headers, response),
            Filtering {
                list_ids,
                filter: connector.tools.clone(),
            },
            Held {
                _permits: Some(permits),
                _stream: None,
                cancel: Some(cancel),
                idle: None,
            },
        ),
    }
}

/// What the proxy refuses itself, never reaching the remote: a method or tool the connector doesn't pass, costing no
/// call; then the owner's call rate. The answer for a refused body, `None` for one that goes on.
///
/// # Errors
/// `429 rate_limited` past the owner's `mcp.calls_per_minute`.
fn refused_here(
    state: &State,
    owner: uuid::Uuid,
    connector: &ConnectorDocument,
    messages: &jsonrpc::Messages,
) -> Result<Option<Response>, GatewayError> {
    match jsonrpc::check(messages, connector.tools.as_ref()) {
        Verdict::Refuse(Some(answer)) => return Ok(Some(axum::Json(answer).into_response())),
        Verdict::Refuse(None) => return Ok(Some(StatusCode::ACCEPTED.into_response())),
        Verdict::Forward => {}
    }
    let per_minute = state.settings.current().limits.mcp_calls_per_minute;
    state.mcp.charge(owner, messages.count(), per_minute)?;
    Ok(None)
}

/// The caller's conversation owner, when the token is live, bound to this address, and the conversation's connector
/// set contains `name`. Anything else is refused the same way, so names cannot be probed.
async fn admitted(
    state: &State,
    headers: &HeaderMap,
    peer: IpAddr,
    name: &str,
) -> Result<(uuid::Uuid, uuid::Uuid), GatewayError> {
    let token = crate::auth::token::bearer(headers)?;
    let (conversation, _) = state
        .runtime_tokens
        .resolve(&token, peer)
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    let id = crate::conversations::uuid(&conversation)?;
    let record = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT owner_id,connectors FROM conversations WHERE id=$1 AND state='active'",
            id
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?
    .ok_or(GatewayError::Unauthorized)?;
    if !record.connectors.iter().any(|connector| connector == name) {
        return Err(failure(ConnectorFailure::NotAllowed));
    }
    Ok((id, record.owner_id))
}

const fn source_label(source: Option<CredentialSource>) -> &'static str {
    match source {
        Some(CredentialSource::User) => "user",
        Some(CredentialSource::Shared) => "shared",
        None => "none",
    }
}

/// The remote's answer: one bounded JSON body, or an event stream to relay event by event.
enum Upstream {
    Json(StatusCode, HeaderMap, Option<serde_json::Value>),
    Stream(StatusCode, HeaderMap, reqwest::Response),
}

impl Upstream {
    const fn headers(&self) -> &HeaderMap {
        match self {
            Self::Json(_, headers, _) | Self::Stream(_, headers, _) => headers,
        }
    }
}

/// Whether the remote refused the calling person's own credential: 401 or 403 to a request that carried it. A shared
/// credential's refusal stays the remote's answer, since only an administrator can fix it (ADR-0031 point 3).
fn refuses_own_credential(source: Option<CredentialSource>, status: reqwest::StatusCode) -> bool {
    source == Some(CredentialSource::User)
        && matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        )
}

/// Sends the body to the remote at checked addresses and reads its bounded JSON answer.
async fn forward(
    state: &State,
    connector: &ConnectorDocument,
    (method, headers, body): (reqwest::Method, &HeaderMap, Option<Bytes>),
    credential: Option<crate::ConnectorCredential>,
) -> Result<Upstream, GatewayError> {
    let unavailable = || failure(ConnectorFailure::Unavailable);
    let url = url::Url::parse(&connector.url).map_err(|_| unavailable())?;
    let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
        return Err(unavailable());
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addresses = checked_addresses(&state.mcp.resolver(), host, port).await?;
    let mut builder = reqwest::Client::builder()
        .https_only(!connector.insecure_http)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(Duration::from_secs(u64::from(
            connector.call_timeout_seconds,
        )))
        .resolve_to_addrs(host, &addresses)
        .user_agent("tqa-gateway-mcp-proxy");
    if let Some(path) = &connector.ca_file {
        let pem = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            tokio::fs::read(path).await.map_err(|_| unavailable())
        })
        .await?;
        for certificate in reqwest::Certificate::from_pem_bundle(&pem).map_err(|_| unavailable())? {
            builder = builder.add_root_certificate(certificate);
        }
    }
    let client = builder.build().map_err(|_| unavailable())?;
    let mut request = client.request(method, url);
    if let Some(body) = body {
        request = request.body(body);
    }
    for (name, value) in headers {
        if jsonrpc::forward_request_header(name.as_str()) {
            request = request.header(name, value);
        }
    }
    let source = credential.as_ref().map(|credential| credential.source);
    if let Some(credential) = credential {
        request = inject(request, connector, &credential.value)?;
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            failure(ConnectorFailure::Timeout)
        } else {
            unavailable()
        }
    })?;
    let status = response.status();
    if status.is_redirection() {
        return Err(failure(ConnectorFailure::Redirect));
    }
    // The person's own key reached the remote and was refused: say so, before the body is read, rather than an
    // unreachable connector (a 401's body is often not JSON at all).
    if refuses_own_credential(source, status) {
        return Err(failure(ConnectorFailure::CredentialRejected));
    }
    if status.is_server_error() {
        return Err(unavailable());
    }
    let answer_headers = response.headers().clone();
    let content_type = answer_headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.starts_with("text/event-stream") {
        return Ok(Upstream::Stream(status, answer_headers, response));
    }
    let bytes = bounded_body(response).await?;
    let answer = if bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&bytes).map_err(|_| unavailable())?)
    };
    Ok(Upstream::Json(status, answer_headers, answer))
}

/// Resolves the host once and refuses a private or reserved answer unless the URL names that address itself; the
/// client then connects only to these addresses (no second resolution, so DNS cannot rebind in between).
async fn checked_addresses(
    resolver: &Arc<dyn EgressResolver>,
    host: &str,
    port: u16,
) -> Result<Vec<SocketAddr>, GatewayError> {
    let literal = host.parse::<IpAddr>().ok();
    let addresses = match literal {
        Some(address) => vec![SocketAddr::new(address, port)],
        None => resolver
            .resolve(host, port)
            .await
            .map_err(|_| failure(ConnectorFailure::Unavailable))?,
    };
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| crate::egress::guarded(address.ip(), literal.is_some()))
    {
        tracing::warn!(
            host,
            port,
            "connector host refused: no address, or a private or reserved one"
        );
        return Err(failure(ConnectorFailure::Unavailable));
    }
    Ok(addresses)
}

/// Adds the credential the connector's scheme asks for.
fn inject(
    request: reqwest::RequestBuilder,
    connector: &ConnectorDocument,
    value: &secrecy::SecretString,
) -> Result<reqwest::RequestBuilder, GatewayError> {
    let secret = |text: String| {
        let mut header =
            HeaderValue::from_str(&text).map_err(|_| failure(ConnectorFailure::Unavailable))?;
        header.set_sensitive(true);
        Ok::<_, GatewayError>(header)
    };
    Ok(
        match (connector.auth.scheme, connector.auth.header.as_deref()) {
            (Scheme::Bearer, _) => request.header(
                reqwest::header::AUTHORIZATION,
                secret(format!("Bearer {}", value.expose_secret()))?,
            ),
            (Scheme::Header, Some(header)) => {
                request.header(header, secret(value.expose_secret().to_owned())?)
            }
            _ => request,
        },
    )
}

/// Reads at most one message's worth of answer.
async fn bounded_body(response: reqwest::Response) -> Result<Vec<u8>, GatewayError> {
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            failure(if error.is_timeout() {
                ConnectorFailure::Timeout
            } else {
                ConnectorFailure::Unavailable
            })
        })?;
        if bytes.len() + chunk.len() > MAX_MESSAGE_BYTES {
            return Err(failure(ConnectorFailure::AnswerTooLarge));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// The runtime's answer: the remote's status, its protocol headers only, and the (filtered) JSON.
fn respond(
    status: StatusCode,
    headers: &HeaderMap,
    answer: Option<&serde_json::Value>,
) -> Response {
    let mut response = match answer {
        Some(answer) => (status, axum::Json(answer)).into_response(),
        None => status.into_response(),
    };
    for (name, value) in headers {
        if jsonrpc::forward_answer_header(name.as_str()) && name != reqwest::header::CONTENT_TYPE {
            response.headers_mut().insert(name.clone(), value.clone());
        }
    }
    response
}

/// What an event-stream answer is filtered by.
struct Filtering {
    list_ids: Vec<serde_json::Value>,
    filter: Option<crate::connectors::ToolFilter>,
}

/// What a relayed stream keeps alive until it ends: the in-flight permits, the session's stream mark, the cancel
/// guard of the call it answers, and the idle bound of a server stream.
struct Held {
    _permits: Option<Permits>,
    _stream: Option<StreamMark>,
    cancel: Option<CancelOnDrop>,
    idle: Option<Duration>,
}

/// The relay's state between chunks.
struct Relay {
    upstream: futures::stream::BoxStream<'static, reqwest::Result<Bytes>>,
    framer: sse::Framer,
    filtering: Filtering,
    held: Held,
    ended: bool,
}

/// Relays an event stream: cut into whole events of at most one message, `tools/list` results filtered, and ended
/// with a JSON-RPC error event if an event is too large. What `held` keeps lives until the stream ends; a stream that
/// ends normally no longer needs its call cancelled.
fn stream(
    (status, headers, response): (StatusCode, &HeaderMap, reqwest::Response),
    filtering: Filtering,
    held: Held,
) -> Response {
    let relay = Relay {
        upstream: response.bytes_stream().boxed(),
        framer: sse::Framer::new(MAX_MESSAGE_BYTES),
        filtering,
        held,
        ended: false,
    };
    let events = futures::stream::unfold(relay, |mut relay| async move {
        if relay.ended {
            return None;
        }
        let next = match relay.held.idle {
            Some(idle) => tokio::time::timeout(idle, relay.upstream.next())
                .await
                .ok()?,
            None => relay.upstream.next().await,
        };
        // The remote ending, failing (its timeout included) or going quiet too long ends the stream.
        let Some(Ok(chunk)) = next else {
            if next.is_none()
                && let Some(cancel) = relay.held.cancel.as_mut()
            {
                cancel.disarm();
            }
            return None;
        };
        let text = if let Ok(events) = relay.framer.push(&chunk) {
            events
                .into_iter()
                .map(|event| relay_event(event, &relay.filtering))
                .collect::<String>()
        } else {
            relay.ended = true;
            let error = serde_json::json!({"jsonrpc": "2.0", "id": null, "error": {
                "code": ANSWER_CUT_OFF, "message": "the connector's event was larger than one message may be"}});
            sse::Event {
                data: error.to_string(),
                ..sse::Event::default()
            }
            .encode()
        };
        Some((Ok::<_, std::io::Error>(Bytes::from(text)), relay))
    });
    let mut response = Response::new(axum::body::Body::from_stream(events));
    *response.status_mut() = status;
    response.headers_mut().insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    for (name, value) in headers {
        if jsonrpc::forward_answer_header(name.as_str()) && name != reqwest::header::CONTENT_TYPE {
            response.headers_mut().insert(name.clone(), value.clone());
        }
    }
    response
}

/// One event, with a `tools/list` result filtered when its data is one.
fn relay_event(mut event: sse::Event, filtering: &Filtering) -> String {
    if let Ok(mut message) = serde_json::from_str::<serde_json::Value>(&event.data) {
        jsonrpc::filter_answer(&mut message, &filtering.list_ids, filtering.filter.as_ref());
        event.data = message.to_string();
    }
    event.encode()
}

/// How long a server stream may stay quiet before the proxy ends it.
const STREAM_IDLE: Duration = Duration::from_mins(5);
/// How long the proxy tries to tell the remote that a call was abandoned.
const CANCEL_DEADLINE: Duration = Duration::from_secs(2);
const SESSION_HEADER: &str = "mcp-session-id";

/// The request's opaque session id and the remote session it stands for; `None` without a session header. A header
/// that names nothing usable is `404`, which MCP clients answer by initialising again.
fn session_of(
    state: &State,
    headers: &HeaderMap,
    (conversation, connector): (uuid::Uuid, &str),
    source: Option<CredentialSource>,
) -> Result<Option<(String, String)>, GatewayError> {
    let Some(opaque) = headers.get(SESSION_HEADER) else {
        return Ok(None);
    };
    let opaque = opaque
        .to_str()
        .map_err(|_| GatewayError::NotFound)?
        .to_owned();
    let found = state
        .mcp
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .find(
            &opaque,
            (conversation, connector),
            source,
            tokio::time::Instant::now(),
        );
    match found {
        sessions::Found::Remote(remote) => Ok(Some((opaque, remote))),
        sessions::Found::Unknown => Err(GatewayError::NotFound),
    }
}

/// The runtime's headers for the remote: its session id replaced by the remote's (or dropped).
fn outbound_headers(headers: &HeaderMap, remote: Option<&str>) -> HeaderMap {
    let mut outbound = headers.clone();
    outbound.remove(SESSION_HEADER);
    if let Some(value) = remote.and_then(|remote| HeaderValue::from_str(remote).ok()) {
        outbound.insert(SESSION_HEADER, value);
    }
    outbound
}

/// The opaque session id to give the runtime: the one it used, or a new one for a session the remote just opened.
fn bind_session(
    state: &State,
    used: Option<String>,
    answer: &HeaderMap,
    key: (uuid::Uuid, &str),
    source: Option<CredentialSource>,
) -> Result<Option<String>, GatewayError> {
    if used.is_some() {
        return Ok(used);
    }
    let Some(remote) = answer
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|remote| !remote.is_empty() && remote.len() <= 256)
    else {
        return Ok(None);
    };
    let mut bytes = [0_u8; 16];
    rand::TryRngCore::try_fill_bytes(&mut rand::rngs::OsRng, &mut bytes)
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let opaque = bytes
        .iter()
        .fold(String::with_capacity(32), |mut out, byte| {
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
            out
        });
    let opaque = state
        .mcp
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .open(
            (opaque, remote.to_owned()),
            key,
            source,
            tokio::time::Instant::now(),
        );
    tracing::info!(connector = key.1, "connector session opened");
    Ok(Some(opaque))
}

/// Never the remote's session id: the opaque one, or none.
fn set_session_header(response: &mut Response, opaque: Option<&str>) {
    response.headers_mut().remove(SESSION_HEADER);
    if let Some(value) = opaque.and_then(|opaque| HeaderValue::from_str(opaque).ok()) {
        response.headers_mut().insert(SESSION_HEADER, value);
    }
}

/// While armed, dropping it tells the remote that the runtime abandoned its `tools/call` (a cancelled run, a retired
/// sandbox), as MCP's `notifications/cancelled`, within [`CANCEL_DEADLINE`].
struct CancelOnDrop {
    armed: Option<Cancel>,
}

struct Cancel {
    state: Arc<State>,
    connector: ConnectorDocument,
    remote: Option<String>,
    request: serde_json::Value,
    credential: Option<crate::ConnectorCredential>,
}

impl CancelOnDrop {
    /// Armed only for one `tools/call` request (with an id).
    fn arm(
        state: &Arc<State>,
        connector: &ConnectorDocument,
        messages: &jsonrpc::Messages,
        remote: Option<String>,
        credential: Option<crate::ConnectorCredential>,
    ) -> Self {
        Self {
            armed: messages.single_call_id().map(|request| Cancel {
                state: Arc::clone(state),
                connector: connector.clone(),
                remote,
                request,
                credential,
            }),
        }
    }

    fn disarm(&mut self) {
        self.armed = None;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let Some(cancel) = self.armed.take() else {
            return;
        };
        let body = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": {"requestId": cancel.request, "reason": "the caller went away"}});
        tokio::spawn(async move {
            let headers = outbound_headers(&HeaderMap::new(), cancel.remote.as_deref());
            let mut headers = headers;
            headers.insert(
                reqwest::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            let sent = tokio::time::timeout(
                CANCEL_DEADLINE,
                forward(
                    &cancel.state,
                    &cancel.connector,
                    (
                        reqwest::Method::POST,
                        &headers,
                        Some(Bytes::from(body.to_string())),
                    ),
                    cancel.credential,
                ),
            )
            .await;
            tracing::info!(connector = %cancel.connector.name, sent = matches!(sent, Ok(Ok(_))), "abandoned connector call cancelled");
        });
    }
}

/// Marks the session's server stream open while it lives.
struct StreamMark {
    sessions: Arc<Mutex<sessions::Sessions>>,
    opaque: String,
}

impl Drop for StreamMark {
    fn drop(&mut self) {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .end_stream(&self.opaque);
    }
}
