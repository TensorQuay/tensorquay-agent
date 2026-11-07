//! Explicit network deadlines and bounded non-streaming response bodies.
pub(crate) mod dns;

use crate::{ConnectionFailure, LlmError, StreamLimits, TimeoutKind};
use std::{error::Error as _, future::Future, io, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_types::Sleeper;

/// HTTP settings; stream allocation limits must be supplied explicitly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttpConfig {
    /// Maximum connection establishment time, including TLS.
    pub connect_timeout: Duration,
    /// Maximum wait for response headers.
    pub first_byte_timeout: Duration,
    /// Maximum wait between received body chunks.
    pub idle_timeout: Duration,
    /// Maximum bytes retained from a non-streaming HTTP body.
    pub body_cap: usize,
    /// Explicit stream parsing and collection limits.
    pub stream: StreamLimits,
    /// Who answers: a provider, or the gateway's relay.
    pub peer: Peer,
}

/// Who a client talks to. It decides which failures before any response are worth another attempt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Peer {
    /// A model provider: a name that doesn't resolve is a configuration error, and permanent.
    #[default]
    Provider,
    /// The gateway's relay, the sandbox's only peer (ADR-0021): while the gateway restarts, its name can briefly fail
    /// to resolve and its address be unreachable, so both are retried like a refused connection.
    Relay,
}

pub(crate) fn client(
    config: HttpConfig,
    resolver: Arc<dyn reqwest::dns::Resolve>,
) -> Result<reqwest::Client, LlmError> {
    reqwest::Client::builder()
        .connect_timeout(config.connect_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .dns_resolver(Arc::new(dns::Resolver(resolver)))
        .no_proxy()
        .build()
        .map_err(transport_error)
}

impl HttpConfig {
    /// Uses 10/30/60-second connect/header/idle deadlines and a 1 MiB body cap.
    #[must_use]
    pub const fn new(stream: StreamLimits) -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            first_byte_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_mins(1),
            body_cap: 1024 * 1024,
            stream,
            peer: Peer::Provider,
        }
    }
}

pub(crate) async fn deadline<T>(
    future: impl Future<Output = T>,
    sleeper: &dyn Sleeper,
    duration: Duration,
    cancel: &CancellationToken,
    kind: TimeoutKind,
) -> Result<T, LlmError> {
    tokio::select! { biased;
        () = cancel.cancelled() => Err(LlmError::Cancelled),
        result = future => Ok(result),
        () = sleeper.sleep(duration) => Err(LlmError::Timeout(kind)),
    }
}

pub(crate) fn transport_error(error: reqwest::Error) -> LlmError {
    if error.is_timeout() {
        return LlmError::Timeout(if error.is_connect() {
            TimeoutKind::Connect
        } else {
            TimeoutKind::FirstByte
        });
    }
    LlmError::Io(crate::redact(&error.without_url().to_string()))
}

/// Only the send boundary may classify a transient failure with no received response headers. The class is logged,
/// never the URL, so a failure's cause can be read from the runtime's log.
pub(crate) fn before_headers_error(error: reqwest::Error, peer: Peer) -> LlmError {
    let classified = classify_before_headers(error, peer);
    let class = match &classified {
        LlmError::Connection(failure) => failure.class(),
        LlmError::Timeout(_) => "timeout",
        _ => "other",
    };
    tracing::warn!(
        class,
        code = classified.code(),
        "model request failed before response headers"
    );
    classified
}

fn classify_before_headers(error: reqwest::Error, peer: Peer) -> LlmError {
    let dns_failed = std::iter::successors(error.source(), |&cause| cause.source())
        .any(<dyn std::error::Error>::is::<dns::Failure>);
    if dns_failed {
        return match peer {
            Peer::Relay => LlmError::Connection(ConnectionFailure::Unresolved),
            Peer::Provider => LlmError::Io(crate::redact(&error.without_url().to_string())),
        };
    }
    if error.is_request() && !error.is_timeout() {
        let mut source = error.source();
        while let Some(cause) = source {
            let failure = cause
                .downcast_ref::<io::Error>()
                .and_then(|error| connection_failure(error.kind(), peer))
                .or_else(|| {
                    cause
                        .downcast_ref::<hyper::Error>()
                        .filter(|error| error.is_incomplete_message())
                        .map(|_| ConnectionFailure::Closed)
                });
            if let Some(failure) = failure {
                return LlmError::Connection(failure);
            }
            source = cause.source();
        }
    }
    transport_error(error)
}

/// A failure before any response that another attempt may clear, by the operating system's error kind.
pub(crate) const fn connection_failure(
    kind: io::ErrorKind,
    peer: Peer,
) -> Option<ConnectionFailure> {
    match kind {
        io::ErrorKind::ConnectionRefused => Some(ConnectionFailure::Refused),
        io::ErrorKind::ConnectionReset => Some(ConnectionFailure::Reset),
        io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof => Some(ConnectionFailure::Closed),
        io::ErrorKind::HostUnreachable | io::ErrorKind::NetworkUnreachable
            if matches!(peer, Peer::Relay) =>
        {
            Some(ConnectionFailure::Unreachable)
        }
        _ => None,
    }
}

pub(crate) async fn body(
    response: &mut reqwest::Response,
    config: HttpConfig,
    sleeper: &dyn Sleeper,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, LlmError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = deadline(
        response.chunk(),
        sleeper,
        config.idle_timeout,
        cancel,
        TimeoutKind::Idle,
    )
    .await?
    .map_err(transport_error)?
    {
        if chunk.len() > config.body_cap.saturating_sub(bytes.len()) {
            return Err(LlmError::BodyTooLarge {
                max: config.body_cap,
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
