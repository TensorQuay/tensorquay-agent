//! The keyed hosts and runs the host-keys proxy tests share (GW-I-166, GW-I-175, GW-I-176, GW-I-180): test hosts under a
//! test authority (one keep-alive, one that closes after answering, raw ones that drop or cut their answers), a resolver
//! that sends public-looking addresses to them, and a run going with keys.
#![allow(dead_code, unused_imports)] // Each integration target compiles this shared harness on its own.
use crate::support::{TestResult, call};
use base64::Engine as _;
use bytes::Bytes;
use http_body_util::{BodyExt as _, Full};
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use reqwest::Method;
use secrecy::ExposeSecret as _;
use serde_json::json;
use sqlx::PgPool;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tqa_types::{ClientFrame, Event, ServerFrame};

pub(crate) type Error = Box<dyn std::error::Error>;

pub(crate) const VALUE: &str = "tqa-test-host-key-proxy-61f0";

/// One request a test upstream saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sent {
    /// The request target as it arrived: origin form, unless a client sent another.
    pub(crate) target: String,
    pub(crate) path: String,
    pub(crate) authorization: String,
    pub(crate) encoding: String,
    pub(crate) body: String,
}

/// What a test upstream saw.
#[derive(Clone, Debug, Default)]
pub(crate) struct Seen(pub(crate) Arc<Mutex<Vec<Sent>>>);

/// Names to public addresses, and public addresses to the local servers behind them.
pub(crate) struct Resolver {
    pub(crate) names: HashMap<String, SocketAddr>,
    pub(crate) servers: HashMap<SocketAddr, SocketAddr>,
}

#[async_trait::async_trait]
impl tqa_gateway::EgressResolver for Resolver {
    async fn resolve(&self, host: &str, _: u16) -> std::io::Result<Vec<SocketAddr>> {
        self.names
            .get(host)
            .map(|address| vec![*address])
            .ok_or_else(|| std::io::Error::other("unknown name"))
    }

    async fn connect(&self, address: SocketAddr) -> std::io::Result<tokio::net::TcpStream> {
        let local = self
            .servers
            .get(&address)
            .ok_or_else(|| std::io::Error::other("no server"))?;
        tokio::net::TcpStream::connect(local).await
    }
}

/// A test CA and a TLS upstream for `host` under it: records what it is sent; `/gz` answers compressed.
pub(crate) async fn upstream(
    ca: &(rcgen::KeyPair, rcgen::CertificateParams),
    host: &str,
) -> Result<(SocketAddr, Seen), Error> {
    upstream_with(ca, host, true).await
}

/// As [`upstream`]; without `keep_alive` it closes each connection after its answer (`Connection: close`), as many
/// servers and every HTTP/1.0 one do (GW-I-180).
pub(crate) async fn upstream_with(
    ca: &(rcgen::KeyPair, rcgen::CertificateParams),
    host: &str,
    keep_alive: bool,
) -> Result<(SocketAddr, Seen), Error> {
    let key = rcgen::KeyPair::generate()?;
    let leaf = rcgen::CertificateParams::new(vec![host.to_owned()])?
        .signed_by(&key, &rcgen::Issuer::from_params(&ca.1, &ca.0))?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
    )?;
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let seen = Seen::default();
    let record = seen.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve_one(
                acceptor.clone(),
                socket,
                record.clone(),
                keep_alive,
            ));
        }
    });
    Ok((address, seen))
}

/// Serves one TLS connection of a test upstream.
pub(crate) async fn serve_one(
    acceptor: tokio_rustls::TlsAcceptor,
    socket: tokio::net::TcpStream,
    record: Seen,
    keep_alive: bool,
) {
    let Ok(stream) = acceptor.accept(socket).await else {
        return;
    };
    let service = hyper::service::service_fn(move |request| respond(request, record.clone()));
    let _ = hyper::server::conn::http1::Builder::new()
        .keep_alive(keep_alive)
        .serve_connection(TokioIo::new(stream), service)
        .await;
}

/// Records the request and answers `ok`, compressed on `/gz`; on `/upgrade`, `101` to a WebSocket upgrade and `426`
/// to a request without one (GW-I-194).
pub(crate) async fn respond(
    request: Request<hyper::body::Incoming>,
    record: Seen,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let header = |name| {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let (target, path, authorization, encoding) = (
        request.uri().to_string(),
        request.uri().path().to_owned(),
        header("authorization"),
        header("accept-encoding"),
    );
    let upgrade = header("upgrade");
    let body = request.into_body().collect().await?.to_bytes();
    // The credential the host saw last, before this request: what a "show my token" page or a session sends back.
    let last = record.0.lock().ok().and_then(|seen| {
        seen.iter()
            .rev()
            .map(|sent| sent.authorization.clone())
            .find(|authorization| !authorization.is_empty())
    });
    if let Ok(mut seen) = record.0.lock() {
        seen.push(Sent {
            target,
            path: path.clone(),
            authorization: authorization.clone(),
            encoding,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }
    if path == "/hang" {
        // A host that never begins its answer.
        std::future::pending::<()>().await;
    }
    if path == "/upgrade" {
        let mut response = Response::new(Full::new(Bytes::new()));
        if upgrade.eq_ignore_ascii_case("websocket") {
            *response.status_mut() = hyper::StatusCode::SWITCHING_PROTOCOLS;
            let headers = response.headers_mut();
            headers.insert(
                "upgrade",
                hyper::header::HeaderValue::from_static("websocket"),
            );
            headers.insert(
                "connection",
                hyper::header::HeaderValue::from_static("Upgrade"),
            );
        } else {
            *response.status_mut() = hyper::StatusCode::UPGRADE_REQUIRED;
        }
        return Ok(response);
    }
    if path == "/echo" {
        // Sends the key straight back, in a header and the body: what scrubbing is for.
        let mut response =
            Response::new(Full::new(Bytes::from(format!("you sent {authorization}"))));
        if let Ok(value) = hyper::header::HeaderValue::from_str(&authorization) {
            response.headers_mut().insert("x-echo", value);
        }
        return Ok(response);
    }
    if path == "/last" {
        let last = last.unwrap_or_default();
        let mut response = Response::new(Full::new(Bytes::from(format!("last {last}"))));
        if let Ok(value) = hyper::header::HeaderValue::from_str(&last) {
            response.headers_mut().insert("x-echo", value);
        }
        return Ok(response);
    }
    let mut response = Response::new(Full::new(Bytes::from_static(b"ok")));
    if path == "/gz" {
        response.headers_mut().insert(
            "content-encoding",
            hyper::header::HeaderValue::from_static("gzip"),
        );
    }
    Ok(response)
}

/// A CONNECT through the proxy, then TLS trusting only `roots` for `host`, then one HTTP/1.1 sender.
pub(crate) async fn keyed_client(
    proxy: SocketAddr,
    host: &str,
    roots: rustls::RootCertStore,
) -> Result<hyper::client::conn::http1::SendRequest<Full<Bytes>>, Error> {
    client_at(proxy, &format!("{host}:443"), host, roots).await
}

/// A CONNECT to `authority` (an IPv6 address in brackets), then TLS for `name` trusting only `roots`.
pub(crate) async fn client_at(
    proxy: SocketAddr,
    authority: &str,
    name: &str,
    roots: rustls::RootCertStore,
) -> Result<hyper::client::conn::http1::SendRequest<Full<Bytes>>, Error> {
    client_as(proxy, (authority, name), roots, None).await
}

/// As [`client_at`], the CONNECT naming its sandbox by the runtime's `bearer` (`Proxy-Authorization`), as a runtime
/// does when its transport address is shared, as two test runtimes on loopback share theirs.
pub(crate) async fn client_as(
    proxy: SocketAddr,
    (authority, name): (&str, &str),
    roots: rustls::RootCertStore,
    bearer: Option<&str>,
) -> Result<hyper::client::conn::http1::SendRequest<Full<Bytes>>, Error> {
    let host = name;
    let mut socket = tokio::net::TcpStream::connect(proxy).await?;
    let credential = bearer.map_or_else(String::new, |token| {
        format!("Proxy-Authorization: Bearer {token}\r\n")
    });
    socket
        .write_all(
            format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n{credential}\r\n")
                .as_bytes(),
        )
        .await?;
    let mut answer = [0_u8; 39];
    socket.read_exact(&mut answer).await?;
    assert_eq!(&answer, b"HTTP/1.1 200 Connection Established\r\n\r\n");
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(
            rustls::pki_types::ServerName::try_from(host.to_owned())?,
            socket,
        )
        .await?;
    let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls)).await?;
    tokio::spawn(connection);
    Ok(sender)
}

pub(crate) async fn send(
    sender: &mut hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    request: Request<Full<Bytes>>,
) -> Result<(u16, String), Error> {
    let response = sender.send_request(request).await?;
    let status = response.status().as_u16();
    let body = response.into_body().collect().await?.to_bytes();
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

/// The `x-echo` header and the body of an answer.
pub(crate) async fn echo(
    sender: &mut hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    request: Request<Full<Bytes>>,
) -> Result<(String, String), Error> {
    let response = sender.send_request(request).await?;
    let header = response
        .headers()
        .get("x-echo")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response.into_body().collect().await?.to_bytes();
    Ok((header, String::from_utf8_lossy(&body).into_owned()))
}

pub(crate) fn get(
    host: &str,
    path: &str,
    authorization: Option<&str>,
) -> Result<Request<Full<Bytes>>, Error> {
    let mut request = Request::get(path)
        .header("host", host)
        .header("accept-encoding", "gzip");
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    Ok(request.body(Full::new(Bytes::new()))?)
}

pub(crate) fn roots(pems: &str) -> Result<rustls::RootCertStore, Error> {
    let mut store = rustls::RootCertStore::empty();
    for block in pems
        .split("-----END CERTIFICATE-----")
        .filter(|block| block.contains("BEGIN"))
    {
        let body: String = block
            .split("-----BEGIN CERTIFICATE-----")
            .nth(1)
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        store.add(rustls::pki_types::CertificateDer::from(
            base64::engine::general_purpose::STANDARD.decode(body)?,
        ))?;
    }
    Ok(store)
}

/// The test CA and the three local hosts behind public-looking addresses: a keyed one, an unkeyed one, and a keyed
/// one whose certificate the gateway won't trust.
pub(crate) struct Hosts {
    pub(crate) ca: Vec<u8>,
    pub(crate) resolver: Arc<Resolver>,
    pub(crate) seen: Seen,
    /// What the IPv4 and the IPv6 address's upstreams were sent (GW-I-176).
    pub(crate) by_address: (Seen, Seen),
    /// What the host that closes after each answer was sent (GW-I-180).
    pub(crate) closing: Seen,
    /// How many requests the host that cuts its answers short read (GW-I-180).
    pub(crate) short: Arc<std::sync::atomic::AtomicUsize>,
    /// How many requests the host that drops idle connections read (GW-I-180).
    pub(crate) old: Arc<std::sync::atomic::AtomicUsize>,
}

/// A keyed host that closes its connection after each answer (GW-I-180).
pub(crate) const CLOSING: &str = "close.keyed.test";
/// A keyed host that closes its connection in the middle of its answer (GW-I-180).
pub(crate) const SHORT: &str = "short.keyed.test";
/// A keyed host that answers keep-alive and then drops the connection, as one closing an idle connection (GW-I-180).
pub(crate) const OLD: &str = "old.keyed.test";

/// A TLS host for `host` that reads one request's head, writes `answer` and drops the connection (no TLS
/// `close_notify`). Counts the requests it read.
pub(crate) async fn raw_host(
    ca: &(rcgen::KeyPair, rcgen::CertificateParams),
    host: &str,
    answer: &'static [u8],
) -> Result<(SocketAddr, Arc<std::sync::atomic::AtomicUsize>), Error> {
    let key = rcgen::KeyPair::generate()?;
    let leaf = rcgen::CertificateParams::new(vec![host.to_owned()])?
        .signed_by(&key, &rcgen::Issuer::from_params(&ca.1, &ca.0))?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
    )?;
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = requests.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(answer_once(
                acceptor.clone(),
                socket,
                counted.clone(),
                answer,
            ));
        }
    });
    Ok((address, requests))
}

/// One connection of a [`raw_host`]: the request's head read, `answer` written, then dropped without `close_notify`.
pub(crate) async fn answer_once(
    acceptor: tokio_rustls::TlsAcceptor,
    socket: tokio::net::TcpStream,
    counted: Arc<std::sync::atomic::AtomicUsize>,
    answer: &'static [u8],
) {
    let Ok(mut stream) = acceptor.accept(socket).await else {
        return;
    };
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") && stream.read_exact(&mut byte).await.is_ok() {
        head.push(byte[0]);
    }
    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let _ = stream.write_all(answer).await;
    let _ = stream.flush().await;
}

/// Exact addresses with keys (GW-I-176): an IPv4 and an IPv6 one, each with its own allow rule, and one without.
pub(crate) const V4: &str = "192.0.2.34";
pub(crate) const V6: &str = "2001:db8::34";
pub(crate) const UNLISTED: &str = "192.0.2.35";

pub(crate) async fn hosts() -> Result<Hosts, Error> {
    let ca_key = rcgen::KeyPair::generate()?;
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca = ca_params.self_signed(&ca_key)?.der().to_vec();
    let signer = (ca_key, ca_params);
    let (keyed, seen) = upstream(&signer, "api.keyed.test").await?;
    let (plain, _) = upstream(&signer, "plain.test").await?;
    let other_key = rcgen::KeyPair::generate()?;
    let mut other = rcgen::CertificateParams::new(Vec::<String>::new())?;
    other.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let (untrusted, _) = upstream(&(other_key, other), "bad.keyed.test").await?;
    let (v4, v4_seen) = upstream(&signer, V4).await?;
    let (v6, v6_seen) = upstream(&signer, V6).await?;
    let (closing, closing_seen) = upstream_with(&signer, CLOSING, false).await?;
    // An answer cut short: 64 bytes announced, a few sent.
    let (short, short_requests) = raw_host(
        &signer,
        SHORT,
        b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\n\r\nonly part of it",
    )
    .await?;
    // A whole HTTP/1.0 answer, which closes the connection without saying so.
    let (old, old_requests) = raw_host(
        &signer,
        OLD,
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nwhole",
    )
    .await?;
    // Public addresses the guard lets through; the test resolver connects them to the local servers instead.
    let public = |last: u8| SocketAddr::from((std::net::Ipv4Addr::new(93, 184, 216, last), 443));
    let resolver = Arc::new(Resolver {
        names: HashMap::from([
            ("api.keyed.test".to_owned(), public(31)),
            ("plain.test".to_owned(), public(32)),
            ("bad.keyed.test".to_owned(), public(33)),
            (CLOSING.to_owned(), public(36)),
            (SHORT.to_owned(), public(37)),
            (OLD.to_owned(), public(38)),
        ]),
        servers: HashMap::from([
            (public(31), keyed),
            (public(32), plain),
            (public(33), untrusted),
            (public(36), closing),
            (public(37), short),
            (public(38), old),
            (SocketAddr::new(V4.parse()?, 443), v4),
            (SocketAddr::new(V6.parse()?, 443), v6),
        ]),
    });
    Ok(Hosts {
        ca,
        resolver,
        seen,
        by_address: (v4_seen, v6_seen),
        closing: closing_seen,
        short: short_requests,
        old: old_requests,
    })
}

/// The gateway's file: the three hosts allowed, and a keyed host's answer bound.
pub(crate) fn config(answer_seconds: u64) -> String {
    format!(
        "[limits]\nio_timeout=5\n[egress]\nlisten='127.0.0.1:3128'\nkeyed_answer_seconds={answer_seconds}\n\
         [[egress.allow]]\nhost='api.keyed.test'\nports=[443]\n[[egress.allow]]\nhost='plain.test'\nports=[443]\n\
         [[egress.allow]]\nhost='bad.keyed.test'\nports=[443]\n[[egress.allow]]\nhost='{V4}'\nports=[443]\n\
         [[egress.allow]]\nhost='2001:DB8:0::34'\nports=[443]\n\
         [[egress.allow]]\nhost='{CLOSING}'\nports=[443]\n[[egress.allow]]\nhost='{SHORT}'\nports=[443]\n\
         [[egress.allow]]\nhost='{OLD}'\nports=[443]\n"
    )
}

/// A run going with keys for the keyed hosts: the proxy, the roots the sandbox trusts, the run's placeholder, and its
/// runtime (kept, so the run stays).
pub(crate) struct Run {
    pub(crate) proxy: SocketAddr,
    pub(crate) install: rustls::RootCertStore,
    pub(crate) placeholder: String,
    /// Every key's placeholder, by its variable.
    pub(crate) placeholders: HashMap<String, String>,
    /// The task's owner and their session, to start another task of theirs ([`task_run`]).
    pub(crate) owner: (uuid::Uuid, String),
    /// The task's runtime token, which names its sandbox to the egress proxy ([`client_as`]).
    pub(crate) token: String,
    /// The task's id.
    pub(crate) conversation: String,
    pub(crate) _runtime: crate::support::runtime::Runtime,
}

/// The keys `started` gives a run: the keyed host and the one whose certificate fails.
pub(crate) const KEYS: [(&str, &str); 2] = [
    ("API_KEY", "https://api.keyed.test"),
    ("BAD_KEY", "https://bad.keyed.test"),
];

pub(crate) async fn keyed_run(f: &crate::support::Fixture, hosts: &Hosts) -> Result<Run, Error> {
    f.gateway.trust_upstream_root(&hosts.ca)?;
    started(f, hosts).await
}

/// A run going with keys, trusting only the roots the gateway's configuration gives it.
pub(crate) async fn started(f: &crate::support::Fixture, hosts: &Hosts) -> Result<Run, Error> {
    started_with(f, hosts, &KEYS).await
}

/// A run going with `keys` (variable, address), trusting only the roots the gateway's configuration gives it.
pub(crate) async fn started_with(
    f: &crate::support::Fixture,
    hosts: &Hosts,
    keys: &[(&str, &str)],
) -> Result<Run, Error> {
    let (proxy, _task) = f.egress(hosts.resolver.clone()).await?;
    let (owner, cookie) = f.admin().await?;
    for &(name, address) in keys {
        let body = json!({"name": name, "address": address, "scheme": "bearer", "value": VALUE});
        let (status, added) = call(
            f,
            Method::POST,
            "/api/v1/host-keys",
            &cookie,
            (Some(body), Some(name)),
        )
        .await?;
        assert_eq!(status, 201, "{added}");
    }
    let first = keys.first().ok_or("a key")?.0;
    task_run(f, (owner.id, &cookie), proxy, first).await
}

/// Another task of `owner`'s, its run going with the keys `owner` holds, through the egress proxy at `proxy`; its
/// `placeholder` is the key `first`'s.
pub(crate) async fn task_run(
    f: &crate::support::Fixture,
    (owner, cookie): (uuid::Uuid, &str),
    proxy: SocketAddr,
    first: &str,
) -> Result<Run, Error> {
    let created = f.conversation(owner).await?;
    let mut runtime = f.runtime(&created).await?;
    let install = roots(runtime.config["host_ca"].as_str().ok_or("host_ca")?)?;
    let path = format!("/api/v1/conversations/{}/messages", created.id);
    let posted = f
        .action(
            &path,
            cookie,
            // An idempotency key is the person's: each task's message has its own.
            Some(&format!("message-{}", created.id)),
            &json!({"text": "call the api"}),
        )
        .await?;
    let status = posted.status();
    assert!(status.is_success(), "{status}: {}", posted.text().await?);
    let ClientFrame::UserMessage { host_keys, .. } = runtime.action().await? else {
        return Err("expected the user turn".into());
    };
    let placeholders: HashMap<String, String> = host_keys
        .iter()
        .map(|key| (key.env().to_owned(), key.placeholder().to_owned()))
        .collect();
    let placeholder = placeholders.get(first).ok_or("the first key")?.clone();
    let started = Event::RunStarted {
        run_id: "run-1".parse()?,
    };
    let frame = ServerFrame::Event {
        seq: 1.into(),
        run_id: Some("run-1".parse()?),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: started,
    };
    runtime.send(&frame).await?;
    f.until_sequence(&created.id, 1).await?;
    Ok(Run {
        proxy,
        install,
        placeholder,
        placeholders,
        owner: (owner, cookie.to_owned()),
        conversation: created.id.to_string(),
        token: f
            .backend
            .token(&created.id)
            .ok_or("the runtime's token")?
            .expose_secret()
            .to_owned(),
        _runtime: runtime,
    })
}

pub(crate) const HOST: &str = "api.keyed.test";

/// The gateway's count of keyed calls with `outcome`, from its metrics (guardrails §1.12).
pub(crate) async fn uses(f: &crate::support::Fixture, outcome: &str) -> Result<u64, Error> {
    use tower::ServiceExt as _;
    let response = f
        .gateway
        .metrics_router()
        .oneshot(axum::http::Request::get("/metrics").body(axum::body::Body::empty())?)
        .await?;
    let text = String::from_utf8(response.into_body().collect().await?.to_bytes().to_vec())?;
    let line = format!("tqa_host_key_uses_total{{outcome=\"{outcome}\"}} ");
    Ok(text
        .lines()
        .find_map(|row| row.strip_prefix(&line))
        .and_then(|count| count.trim().parse().ok())
        .unwrap_or(0))
}

/// The page at `path` as `cookie` reads it, once `ready` holds for it. A refused use's row is queued, not awaited
/// (`secrets::reads::refused`), so a test that reads one reads the page until its batch is in, within the deadline.
pub async fn page_until(
    f: &crate::support::Fixture,
    path: &str,
    cookie: &str,
    ready: impl Fn(&serde_json::Value) -> bool,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    tokio::time::timeout(
        tqa_testkit::deadline::scaled(crate::support::DEADLINE),
        async {
            loop {
                let (status, page) = call(f, Method::GET, path, cookie, (None, None)).await?;
                assert_eq!(status, 200, "{page}");
                if ready(&page) {
                    return Ok(page);
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        },
    )
    .await?
}
