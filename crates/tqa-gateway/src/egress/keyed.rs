//! A keyed host's connection (ADR-0037 §4): only when the caller's running run holds a key for the CONNECT's host and
//! port is TLS opened. The proxy answers the sandbox with the install CA's leaf for exactly that host, opens verified
//! TLS upstream to the address it already checked (never a fallback), and serves HTTP/1.1 between them: each request
//! must name the CONNECT's host; a placeholder is judged, its use recorded, and only then swapped for the value; a
//! placeholder anywhere else, a request body included, is refused, and nothing of it reaches the host.
use crate::{
    app::State,
    host_keys::{self, Call, Form, Outcome, Placement},
    secrets::reads::ReadContext,
};
use base64::Engine as _;
use bytes::Bytes;
use futures::StreamExt as _;
use http_body_util::{BodyExt as _, BodyStream, Full, StreamBody, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode,
    body::{Frame, Incoming},
    header::{self, HeaderValue},
};
use hyper_util::rt::TokioIo;
use secrecy::ExposeSecret as _;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use time::OffsetDateTime;
use tokio::{
    net::TcpStream,
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tqa_types::ConversationId;
use uuid::Uuid;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type Body = BoxBody<Bytes, BoxError>;
/// One exchange with the keyed host: the request, and where its answer goes.
type Exchange = (
    Request<Body>,
    tokio::sync::oneshot::Sender<Result<Response<Incoming>, hyper::Error>>,
);
/// The way to the keyed host's connection: requests go one at a time to the loop that owns its sender.
type Upstream = tokio::sync::mpsc::Sender<Exchange>;

/// The placeholder's mark, which no other part of a request may carry.
const MARK: &[u8] = b"tqa_ph_";
/// Headers that belong to one hop and are never forwarded.
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "proxy-connection",
    "keep-alive",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// What the proxy knows about a keyed tunnel's caller.
#[derive(Clone)]
pub(super) struct Tunnel {
    pub conversation: ConversationId,
    id: Uuid,
    sandbox: String,
    sandbox_made: OffsetDateTime,
    /// The run's sender, whose keys the run holds: the task's owner, who alone posts to it today.
    sender: Uuid,
    pub host: String,
    pub port: u16,
    /// The values the sender's swaps taught, on any of their connections to this host, scrubbed from every answer on
    /// this one from its first byte (ADR-0037 §6; the second review's H2).
    scrubber: Arc<Mutex<super::scrub::Scrubber>>,
    /// How long the keyed host may take to begin an answer (`[egress] keyed_answer_seconds`).
    answer: std::time::Duration,
    /// The keyed host closed this connection (GW-I-180): later requests on it are answered in words, never judged,
    /// audited or sent.
    upstream_closed: Arc<std::sync::atomic::AtomicBool>,
}

/// Tests only (the `testing` feature, never in a release build): how many keyed hosts closed their connection, so a
/// test can wait for the proxy to have seen a close before its next request (GW-I-180).
#[cfg(feature = "testing")]
#[derive(Default)]
pub(crate) struct Closes {
    count: std::sync::atomic::AtomicUsize,
    seen: tokio::sync::Notify,
}

#[cfg(feature = "testing")]
impl Closes {
    fn seen(&self) {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.seen.notify_waiters();
    }

    /// Waits until at least `count` keyed hosts have closed their connection.
    pub(crate) async fn at_least(&self, count: usize) {
        loop {
            let notified = self.seen.notified();
            if self.count.load(std::sync::atomic::Ordering::SeqCst) >= count {
                return;
            }
            notified.await;
        }
    }
}

/// The bounds on keyed connections: `[egress] keyed_connections` in all and `keyed_per_sandbox` per sandbox.
pub(super) struct Limits {
    all: Arc<Semaphore>,
    per_sandbox: usize,
    per: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl Limits {
    pub(super) fn new(all: usize, per_sandbox: usize) -> Self {
        Self {
            all: Arc::new(Semaphore::new(all)),
            per_sandbox,
            per: Mutex::new(HashMap::new()),
        }
    }

    /// A keyed connection's two permits, or none when either bound is reached.
    fn acquire(&self, sandbox: &str) -> Option<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
        let all = self.all.clone().try_acquire_owned().ok()?;
        let one = {
            let mut per = self.per.lock().ok()?;
            if per.len() > 1024 {
                per.retain(|_, held| held.available_permits() < self.per_sandbox);
            }
            per.entry(sandbox.to_owned())
                .or_insert_with(|| Arc::new(Semaphore::new(self.per_sandbox)))
                .clone()
        };
        Some((all, one.try_acquire_owned().ok()?))
    }
}

/// The tunnel's keyed context when the caller's running run holds a key for `host:port`; None for a plain tunnel.
pub(super) async fn keyed(
    state: &State,
    conversation: &ConversationId,
    host: &str,
    port: u16,
) -> Option<Tunnel> {
    let id = crate::conversations::uuid(conversation).ok()?;
    // A key's host is stored canonical (an IP address without brackets, zeros compressed); so is the CONNECT's.
    let host = &super::rules::normalized(host);
    let row = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT t.sandbox_id AS \"sandbox!\", t.created_at, c.owner_id FROM runtime_tokens t \
             JOIN conversations c ON c.id=t.conversation_id WHERE t.conversation_id=$1 AND t.sandbox_id IS NOT NULL \
             AND EXISTS (SELECT 1 FROM host_key_placeholders p WHERE p.conversation_id=$1 AND p.sandbox_id=t.sandbox_id \
             AND p.run_id IS NOT NULL AND p.ended_at IS NULL AND p.host=$2 AND p.port=$3)",
            id,
            host,
            i32::from(port)
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await
    .ok()??;
    Some(Tunnel {
        conversation: conversation.clone(),
        id,
        sandbox: row.sandbox,
        sandbox_made: row.created_at,
        sender: row.owner_id,
        host: host.to_owned(),
        port,
        scrubber: state.host_keys.scrubbers.of(row.owner_id, host, port),
        answer: state.config.egress.keyed_answer,
        upstream_closed: Arc::default(),
    })
}

/// Serves a keyed tunnel until either side closes: TLS both ways, then each request through [`handle`].
pub(super) async fn intercept(
    state: Arc<State>,
    limits: &Limits,
    client: TcpStream,
    upstream: TcpStream,
    tunnel: Tunnel,
) {
    let Some(_permits) = limits.acquire(&tunnel.sandbox) else {
        tracing::warn!(conversation_id = %tunnel.conversation, "too many keyed connections; one was closed");
        return;
    };
    let Some(client) = accept(&state, client, &tunnel).await else {
        return;
    };
    let Some((mut sender, connection)) = connect(&state, upstream, &tunnel).await else {
        // Verified TLS upstream failed: the sandbox hears why, and nothing is sent (never a fallback).
        let words = format!(
            "{}'s certificate couldn't be verified, so nothing was sent.",
            tunnel.host
        );
        serve(client, state.config.egress.idle, move |_| {
            let words = words.clone();
            async move { refusal(StatusCode::BAD_GATEWAY, &words) }
        })
        .await;
        return;
    };
    // The tunnel owns its three parts and ends them together: the upstream connection, the loop that owns its
    // sender (one exchange at a time, no lock held across it), and the sandbox's side.
    let (upstream, mut exchanges) = tokio::sync::mpsc::channel::<Exchange>(1);
    let owner = async move {
        while let Some((request, answer)) = exchanges.recv().await {
            let _ = answer.send(sender.send_request(request).await);
        }
    };
    let tunnel = Arc::new(tunnel);
    let closed = tunnel.upstream_closed.clone();
    #[cfg(feature = "testing")]
    let watcher = state.clone();
    let sandbox = serve(client, state.config.egress.idle, move |request| {
        let (state, upstream, tunnel) = (state.clone(), upstream.clone(), tunnel.clone());
        async move { handle(&state, &upstream, &tunnel, request).await }
    });
    // The sandbox's side decides how long the tunnel lives. A host that closes after answering (`Connection: close`, as
    // every HTTP/1.0 server does) ends `connection`, but the answer it sent is still being delivered: ending the tunnel
    // then would cut it short (GW-I-180). So the connection and the sender loop run alongside until the sandbox's side
    // ends; a later request on a closed host is refused in words (`handle`), and a host that closes mid-answer leaves
    // the sandbox a connection that ends, never a reply that looks whole.
    let upstream_side = async {
        let ((), ()) = tokio::join!(
            async {
                let _ = connection.await;
                closed.store(true, std::sync::atomic::Ordering::SeqCst);
                #[cfg(feature = "testing")]
                watcher.keyed_closes.seen();
            },
            owner
        );
        std::future::pending::<()>().await;
    };
    tokio::select! {biased;
        () = sandbox => {}
        () = upstream_side => {}
    }
}

/// TLS with the sandbox, with the install CA's leaf for exactly the CONNECT's host; HTTP/1.1 only.
async fn accept(
    state: &State,
    client: TcpStream,
    tunnel: &Tunnel,
) -> Option<tokio_rustls::server::TlsStream<TcpStream>> {
    let ca = state.host_ca.as_ref()?;
    let leaf = ca
        .leaf(&tunnel.host, tunnel.sandbox_made, OffsetDateTime::now_utc())
        .ok()?;
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(leaf.key().to_vec().into());
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .ok()?
    .with_no_client_auth()
    .with_single_cert(vec![leaf.certificate().to_vec().into()], key)
    .ok()?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    tokio::time::timeout(state.config.io_timeout(), acceptor.accept(client))
        .await
        .ok()?
        .ok()
}

/// Verified TLS to the checked address, SNI the CONNECT's host, against the public roots only.
async fn connect(
    state: &State,
    upstream: TcpStream,
    tunnel: &Tunnel,
) -> Option<(
    hyper::client::conn::http1::SendRequest<Body>,
    hyper::client::conn::http1::Connection<
        TokioIo<tokio_rustls::client::TlsStream<TcpStream>>,
        Body,
    >,
)> {
    let roots = state.upstream_roots.read().ok()?.clone();
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .ok()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let name = rustls::pki_types::ServerName::try_from(tunnel.host.clone()).ok()?;
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let stream = tokio::time::timeout(state.config.io_timeout(), connector.connect(name, upstream))
        .await
        .ok()?
        .ok()?;
    tokio::time::timeout(
        state.config.io_timeout(),
        hyper::client::conn::http1::handshake(TokioIo::new(stream)),
    )
    .await
    .ok()?
    .ok()
}

async fn serve<F, R>(
    client: tokio_rustls::server::TlsStream<TcpStream>,
    idle: std::time::Duration,
    handler: F,
) where
    F: Fn(Request<Incoming>) -> R + Send + Sync + 'static,
    R: std::future::Future<Output = Response<Body>> + Send + 'static,
{
    let service = hyper::service::service_fn(move |request| {
        let answer = handler(request);
        async move { Ok::<_, std::convert::Infallible>(answer.await) }
    });
    // A request's head must come within the idle bound, so a keyed connection never waits forever.
    let _ = hyper::server::conn::http1::Builder::new()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(idle)
        .serve_connection(TokioIo::new(client), service)
        .await;
}

/// One request on a keyed connection.
async fn handle(
    state: &State,
    upstream: &Upstream,
    tunnel: &Tunnel,
    request: Request<Incoming>,
) -> Response<Body> {
    if tunnel
        .upstream_closed
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        // The host ended this connection after its last answer, which was delivered whole: nothing is sent, and the
        // sandbox opens a new connection.
        let mut closed = refusal(
            StatusCode::BAD_GATEWAY,
            &format!(
                "{} closed this connection; open a new one to send again.",
                tunnel.host
            ),
        );
        closed
            .headers_mut()
            .insert(header::CONNECTION, HeaderValue::from_static("close"));
        return closed;
    }
    if let Some(refused) = full_target(&request, tunnel) {
        return refused;
    }
    if !names_host(&request, tunnel) {
        return refusal(
            StatusCode::FORBIDDEN,
            &format!(
                "This connection is for {}; its requests must name it in Host.",
                tunnel.host
            ),
        );
    }
    let path = request
        .uri()
        .path_and_query()
        .map_or("/", hyper::http::uri::PathAndQuery::as_str)
        .to_owned();
    let headers: Vec<(String, Vec<u8>)> = request
        .headers()
        .iter()
        .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
        .collect();
    let run = current_run(state, tunnel).await;
    let trip = Trip::default();
    let response = match host_keys::placement(&path, &headers) {
        Placement::None => forward((upstream, tunnel), request, (None, &trip)).await,
        Placement::Refused => {
            let found = any_placeholder(&path, &headers);
            let bound = match found {
                Some(placeholder) => find(state, &placeholder).await,
                None => None,
            };
            refuse(
                state,
                tunnel,
                (run.as_deref(), bound.as_ref()),
                Outcome::WrongPosition,
            )
            .await
        }
        Placement::One { placeholder, form } => {
            swap(
                state,
                upstream,
                tunnel,
                (request, &path, run.as_deref()),
                (&placeholder, form, &trip),
            )
            .await
        }
    };
    unless_tripped(state, tunnel, (run.as_deref(), &trip), response).await
}

/// The exchange's answer, unless a placeholder in the body stopped the request before it was sent: then it is refused
/// like one anywhere else, counted and told (the second review's M1). A header swap on the same request stands; its
/// head had gone.
async fn unless_tripped(
    state: &State,
    tunnel: &Tunnel,
    (run, trip): (Option<&str>, &Trip),
    response: Response<Body>,
) -> Response<Body> {
    let Some(placeholder) = trip.taken() else {
        return response;
    };
    let bound = find(state, &placeholder).await;
    refuse(state, tunnel, (run, bound.as_ref()), Outcome::WrongPosition).await
}

/// A request with a placeholder in its accepted form: judged, its use recorded, then sent with the value in its place;
/// or refused, and nothing of it sent.
async fn swap(
    state: &State,
    upstream: &Upstream,
    tunnel: &Tunnel,
    (request, path, run): (Request<Incoming>, &str, Option<&str>),
    (placeholder, form, trip): (&str, Form, &Trip),
) -> Response<Body> {
    let bound = find(state, placeholder).await;
    let caller = host_keys::Caller {
        conversation: tunnel.id,
        sandbox: &tunnel.sandbox,
        current_run: run,
        host: &tunnel.host,
        port: tunnel.port,
        path,
        form: form.clone(),
        sender_active: active(state, bound.as_ref()).await,
        allowed: allowed(state, tunnel),
    };
    let outcome = host_keys::judge(bound.as_ref(), &caller);
    let (Outcome::Ok, Some(bound), Some(run)) = (outcome, bound.as_ref(), run) else {
        return refuse(state, tunnel, (run, bound.as_ref()), outcome).await;
    };
    let key_use = host_keys::Use {
        reference: bound.secret_ref.as_deref().unwrap_or_default(),
        host: &tunnel.host,
        port: tunnel.port,
        context: context(tunnel, Some(run)),
    };
    let value = match host_keys::value(state, &key_use).await {
        Ok(value) => value,
        Err(outcome) => return refuse(state, tunnel, (Some(run), Some(bound)), outcome).await,
    };
    let header = match &form {
        Form::Bearer => format!("Bearer {}", value.expose_secret()),
        Form::Basic { user } => format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD
                .encode(format!("{user}:{}", value.expose_secret()))
        ),
    };
    let Ok(mut header) = HeaderValue::from_str(&header) else {
        return refuse(
            state,
            tunnel,
            (Some(run), Some(bound)),
            Outcome::WrongPosition,
        )
        .await;
    };
    header.set_sensitive(true);
    let user = match &form {
        Form::Basic { user } => Some(user.as_str()),
        Form::Bearer => None,
    };
    if !super::scrub::learn_shared(
        &tunnel.scrubber,
        value.expose_secret().as_bytes(),
        placeholder,
        user,
    ) {
        return refuse(
            state,
            tunnel,
            (Some(run), Some(bound)),
            Outcome::Unavailable,
        )
        .await;
    }
    state.telemetry.host_key_use(Outcome::Ok.as_str());
    notice(state, tunnel, (run, Some(bound)), Outcome::Ok).await;
    forward((upstream, tunnel), request, (Some(header), trip)).await
}

/// Inside a keyed tunnel a request names its host only in Host. A target with a scheme or an authority (absolute or
/// authority form), which a server or a shared front may route by over Host (RFC 9112 §3.2.2), is refused before
/// anything is judged, recorded or sent (the second review's H1: domain fronting).
fn full_target(request: &Request<Incoming>, tunnel: &Tunnel) -> Option<Response<Body>> {
    (request.uri().scheme().is_some() || request.uri().authority().is_some()).then(|| {
        refusal(
            StatusCode::FORBIDDEN,
            &format!(
                "This connection is for {}: name what you want by its path, with the host in Host.",
                tunnel.host
            ),
        )
    })
}

/// Whether the request's Host is the CONNECT's host (with its port, which may be left out at 443).
fn names_host(request: &Request<Incoming>, tunnel: &Tunnel) -> bool {
    let Some(named) = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    // `host`, `host:port`, `[v6]` or `[v6]:port`, compared in the canonical form the tunnel's host has.
    let (host, port) = match named.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && (!host.contains(':') || host.ends_with(']')) => {
            (host, port.parse::<u16>().ok())
        }
        _ => (named, Some(443)),
    };
    let host = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host);
    port == Some(tunnel.port) && super::rules::normalized(host) == tunnel.host
}

/// Sends the request on, its hop-by-hop headers left behind; with `swap`, the key's Authorization and a plain
/// (uncompressed) answer asked for, and a compressed one refused. Bodies stream both ways.
async fn forward(
    (upstream, tunnel): (&Upstream, &Tunnel),
    request: Request<Incoming>,
    (swap, trip): (Option<HeaderValue>, &Trip),
) -> Response<Body> {
    let (mut parts, body) = request.into_parts();
    for name in HOP_BY_HOP {
        parts.headers.remove(name);
    }
    let swapped = swap.is_some();
    if let Some(authorization) = swap {
        parts.headers.insert(header::AUTHORIZATION, authorization);
        parts.headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("identity"),
        );
    }
    let request = Request::from_parts(parts, guarded(body, trip.clone()));
    // The answer's head within the bound for a keyed host; its body then streams.
    let (reply, answer) = tokio::sync::oneshot::channel();
    let answer = async {
        upstream.send((request, reply)).await.ok()?;
        answer.await.ok()?.ok()
    };
    let Ok(Some(response)) = tokio::time::timeout(tunnel.answer, answer).await else {
        // A refused request body breaks the upstream exchange too: the sandbox opens a new connection.
        let mut closed = refusal(
            StatusCode::BAD_GATEWAY,
            "The keyed host didn't answer, or the request was stopped because it held a key's placeholder.",
        );
        closed
            .headers_mut()
            .insert(header::CONNECTION, HeaderValue::from_static("close"));
        return closed;
    };
    let compressed = response
        .headers()
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("identity"));
    let scrubber = tunnel
        .scrubber
        .lock()
        .map(|scrubber| scrubber.clone())
        .unwrap_or_default();
    if (swapped || !scrubber.is_empty()) && compressed {
        return refusal(
            StatusCode::BAD_GATEWAY,
            "The keyed host sent a compressed answer, which a keyed connection can't take.",
        );
    }
    let (mut parts, body) = response.into_parts();
    let body = body
        .map_err(|error| -> BoxError { Box::new(error) })
        .boxed();
    if scrubber.is_empty() {
        return Response::from_parts(parts, body);
    }
    // A key's value never comes back: headers and the body are scrubbed, which may change the body's length.
    parts.headers.remove(header::CONTENT_LENGTH);
    scrubber.headers(&mut parts.headers);
    Response::from_parts(parts, scrubbed(body, &scrubber))
}

/// An answer's body with every learned value replaced, chunk by chunk.
fn scrubbed(body: Body, scrubber: &super::scrub::Scrubber) -> Body {
    let start = (
        BodyStream::new(body),
        scrubber.stream(),
        None::<Frame<Bytes>>,
        false,
    );
    let frames =
        futures::stream::unfold(start, |(mut inner, mut stream, pending, done)| async move {
            if let Some(frame) = pending {
                return Some((Ok(frame), (inner, stream, None, done)));
            }
            if done {
                return None;
            }
            match inner.next().await {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => {
                        let out = Frame::data(Bytes::from(stream.push(&data)));
                        Some((Ok(out), (inner, stream, None, false)))
                    }
                    Err(trailers) => {
                        let out = Frame::data(Bytes::from(stream.finish()));
                        Some((Ok(out), (inner, stream, Some(trailers), true)))
                    }
                },
                Some(Err(error)) => Some((Err(error), (inner, stream, None, true))),
                None => {
                    let rest = stream.finish();
                    (!rest.is_empty()).then(|| {
                        (
                            Ok(Frame::data(Bytes::from(rest))),
                            (inner, stream, None, true),
                        )
                    })
                }
            }
        });
    http_body_util::BodyExt::boxed(StreamBody::new(frames))
}

/// The placeholder a request body held, when its guard stopped it; shared by the guard and the request's handler.
#[derive(Clone, Default)]
struct Trip(Arc<Mutex<Option<String>>>);

impl Trip {
    /// Keeps `bytes` from a placeholder's mark: the mark and up to its 43 characters, as many as arrived.
    fn set(&self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MARK.len() + 43)]).into_owned();
        if let Ok(mut held) = self.0.lock() {
            *held = Some(text);
        }
    }

    /// The placeholder the body held, if the guard stopped it.
    fn taken(&self) -> Option<String> {
        self.0.lock().ok().and_then(|mut held| held.take())
    }
}

/// Reads `inner` into `carry` until it holds `want` bytes, the body ends or a non-data frame comes: the rest of a
/// placeholder whose mark was found, bounded by its length.
async fn read_on(inner: &mut BodyStream<Incoming>, carry: &mut Vec<u8>, want: usize) {
    while carry.len() < want {
        let Some(Ok(frame)) = inner.next().await else {
            return;
        };
        let Ok(more) = frame.into_data() else {
            return;
        };
        carry.extend_from_slice(&more);
    }
}

/// A request body that stops, before the part holding it is sent, at a placeholder's mark across chunk boundaries;
/// it reads on to the whole placeholder (or the body's end) to keep it in `trip`, so the refusal can name its key.
fn guarded(body: Incoming, trip: Trip) -> Body {
    struct Guard {
        inner: BodyStream<Incoming>,
        carry: Vec<u8>,
        pending: Option<Frame<Bytes>>,
        done: bool,
        trip: Trip,
    }
    let start = Guard {
        inner: BodyStream::new(body),
        carry: Vec::new(),
        pending: None,
        done: false,
        trip,
    };
    let frames = futures::stream::unfold(start, |mut guard| async move {
        if let Some(frame) = guard.pending.take() {
            return Some((Ok(frame), guard));
        }
        if guard.done {
            return None;
        }
        match guard.inner.next().await {
            Some(Ok(frame)) => match frame.into_data() {
                Ok(data) => {
                    guard.carry.extend_from_slice(&data);
                    if let Some(at) = guard
                        .carry
                        .windows(MARK.len())
                        .position(|window| window == MARK)
                    {
                        guard.done = true;
                        read_on(&mut guard.inner, &mut guard.carry, at + MARK.len() + 43).await;
                        guard.trip.set(&guard.carry[at..]);
                        let error: BoxError = "a placeholder in a request body is refused".into();
                        return Some((Err(error), guard));
                    }
                    let keep = guard.carry.len().saturating_sub(MARK.len() - 1);
                    let out: Vec<u8> = guard.carry.drain(..keep).collect();
                    Some((Ok(Frame::data(Bytes::from(out))), guard))
                }
                Err(trailers) => {
                    let carried = std::mem::take(&mut guard.carry);
                    guard.pending = Some(trailers);
                    Some((Ok(Frame::data(Bytes::from(carried))), guard))
                }
            },
            Some(Err(error)) => {
                guard.done = true;
                Some((Err(Box::new(error) as BoxError), guard))
            }
            None => {
                guard.done = true;
                let carried = std::mem::take(&mut guard.carry);
                (!carried.is_empty()).then(|| (Ok(Frame::data(Bytes::from(carried))), guard))
            }
        }
    });
    http_body_util::BodyExt::boxed(StreamBody::new(frames))
}

/// Records and tells of a refusal, and answers it in plain words for the model.
async fn refuse(
    state: &State,
    tunnel: &Tunnel,
    (run, bound): (Option<&str>, Option<&host_keys::Bound>),
    outcome: Outcome,
) -> Response<Body> {
    if let Some(reference) = bound.and_then(|bound| bound.secret_ref.as_deref()) {
        let key_use = host_keys::Use {
            reference,
            host: &tunnel.host,
            port: tunnel.port,
            context: context(tunnel, run),
        };
        host_keys::refuse(state, &key_use, outcome).await;
    }
    state.telemetry.host_key_use(outcome.as_str());
    if let Some(run) = run {
        notice(state, tunnel, (run, bound), outcome).await;
    }
    refusal(StatusCode::FORBIDDEN, &words(outcome, bound))
}

async fn notice(
    state: &State,
    tunnel: &Tunnel,
    (run, bound): (&str, Option<&host_keys::Bound>),
    outcome: Outcome,
) {
    let call = Call {
        conversation: &tunnel.conversation,
        run,
        sender: tunnel.sender,
        key: bound.and_then(|bound| bound.key_id),
        // A key is named only to its own sender: a teammate's placeholder never tells this task its name.
        name: bound
            .filter(|bound| bound.sender_id == tunnel.sender)
            .map(|bound| bound.name.as_str()),
        host: &tunnel.host,
        port: tunnel.port,
    };
    host_keys::notify(state, &call, outcome).await;
}

/// Why a keyed request was refused, for the model: plain words, no codes (C1).
fn words(outcome: Outcome, bound: Option<&host_keys::Bound>) -> String {
    match outcome {
        Outcome::ForeignRun => "This key's placeholder belongs to another run; use the variable as it is set now.".into(),
        Outcome::ForeignSandbox => "This placeholder belongs to another task.".into(),
        Outcome::Expired => "This placeholder's run has ended.".into(),
        Outcome::WrongHost => bound.map_or_else(
            || "This key isn't for this address.".into(),
            |bound| format!("This key is only for https://{}{}.", bound.host, bound.path_prefix.as_deref().unwrap_or_default()),
        ),
        Outcome::WrongPosition => "A key goes only in the Authorization header: `Authorization: Bearer $NAME`, or Basic with \
            the user shown and the variable as the password."
            .into(),
        Outcome::NotAllowed => "This site isn't on this task's allowed sites any more.".into(),
        Outcome::Absent => "This placeholder isn't a key this task holds.".into(),
        Outcome::Revoked => "This key was deleted, or its person can no longer use it.".into(),
        Outcome::Unrecorded => "The key couldn't be used: its use couldn't be recorded. Try again.".into(),
        Outcome::Unavailable => "The key store is unavailable. Try again in a moment.".into(),
        Outcome::Ok => String::new(),
    }
}

fn refusal(status: StatusCode, words: &str) -> Response<Body> {
    let mut response = Response::new(
        Full::new(Bytes::from(words.to_owned()))
            .map_err(|never| -> BoxError { match never {} })
            .boxed(),
    );
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn context(tunnel: &Tunnel, run: Option<&str>) -> ReadContext {
    ReadContext {
        conversation: Some(tunnel.id),
        user: Some(tunnel.sender),
        run: run.map(str::to_owned),
    }
}

/// The run going in the task now: its latest lifecycle event, when that's a start or a resume.
async fn current_run(state: &State, tunnel: &Tunnel) -> Option<String> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT run_id, lifecycle FROM events WHERE conversation_id=$1 AND lifecycle IS NOT NULL \
             ORDER BY seq DESC LIMIT 1",
            tunnel.id
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await
    .ok()
    .flatten()
    .filter(|row| matches!(row.lifecycle.as_deref(), Some("run.started" | "run.resumed")))
    .and_then(|row| row.run_id)
}

async fn find(state: &State, placeholder: &str) -> Option<host_keys::Bound> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        host_keys::find(&mut connection, placeholder).await
    })
    .await
    .ok()
    .flatten()
}

/// Whether the key's sender is still an active member.
async fn active(state: &State, bound: Option<&host_keys::Bound>) -> bool {
    let Some(bound) = bound else {
        return false;
    };
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT disabled_at IS NULL AS \"active!\" FROM users WHERE id=$1",
            bound.sender_id
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}

/// Whether the task's network list still allows the host and port. A task's list is the installation's allow-list
/// today, as minting reads it too (ADR-0037 §3; the second review's M4).
fn allowed(state: &State, tunnel: &Tunnel) -> bool {
    !state.config.egress.disabled
        && matches!(
            state
                .settings
                .current()
                .egress
                .decide(&tunnel.host, tunnel.port),
            super::Decision::Allowed { .. }
        )
}

/// The first placeholder-shaped text in the request, to attribute a misplaced one to its key.
fn any_placeholder(path: &str, headers: &[(String, Vec<u8>)]) -> Option<String> {
    std::iter::once(path.as_bytes())
        .chain(headers.iter().map(|(_, value)| value.as_slice()))
        .find_map(|bytes| {
            let start = bytes
                .windows(MARK.len())
                .position(|window| window == MARK)?;
            let candidate = bytes.get(start..start + MARK.len() + 43)?;
            candidate
                .iter()
                .skip(MARK.len())
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                .then(|| String::from_utf8_lossy(candidate).into_owned())
        })
}
