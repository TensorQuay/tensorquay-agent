//! Typed failures with secret-safe formatting and explicit retry policy.

use crate::{PartialCompletion, RegistryError, StreamLimitKind, redact};
use std::{fmt, time::Duration};

/// The deadline that ended an HTTP operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TimeoutKind {
    /// Establishing the transport connection, including TLS negotiation.
    #[error("connect")]
    Connect,
    /// Waiting for initial response headers.
    #[error("first byte")]
    FirstByte,
    /// Waiting for further response bytes.
    #[error("idle read")]
    Idle,
}

/// A transient connection failure observed before response headers or model output.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConnectionFailure {
    /// No listener accepted the connection.
    #[error("connection refused")]
    Refused,
    /// The peer reset the connection before returning response headers.
    #[error("connection reset before response headers")]
    Reset,
    /// The connection closed before returning complete response headers.
    #[error("connection closed before response headers")]
    Closed,
    /// The relay's host name did not resolve; only for [`crate::Peer::Relay`], whose name the gateway's restart
    /// briefly removes.
    #[error("the relay's name did not resolve")]
    Unresolved,
    /// The host or network was unreachable before any byte was sent; only for [`crate::Peer::Relay`].
    #[error("the relay's host was unreachable")]
    Unreachable,
}

impl ConnectionFailure {
    /// A short, stable name for logs.
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Refused => "refused",
            Self::Reset => "reset",
            Self::Closed => "closed",
            Self::Unresolved => "unresolved",
            Self::Unreachable => "unreachable",
        }
    }
}

/// The response header by which the gateway relay marks a 429 as its own throttle rather than the provider's.
/// The relay forwards upstream headers from an allowlist, so a provider cannot send it.
pub const RELAY_THROTTLE_HEADER: &str = "tqa-relay-throttle";

/// Which of the relay's limits refused a request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ThrottleScope {
    /// The user's rate, shared by all of the user's conversations and routes.
    User,
    /// The route's concurrency queue; a fallback route has its own.
    Route,
}

impl ThrottleScope {
    /// The header value naming this scope.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Route => "route",
        }
    }
    /// Parses a header value; an unknown one is not a throttle.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "route" => Some(Self::Route),
            _ => None,
        }
    }
}

/// A classified provider failure; formatting never prints unredacted diagnostic fields.
#[derive(thiserror::Error)]
pub enum LlmError {
    /// The outgoing tool catalog exceeds its count, schema or total encoded-byte limit.
    #[error(transparent)]
    CatalogLimit(#[from] tqa_types::ToolCatalogLimit),
    /// An unsuccessful HTTP status with a bounded, sanitized excerpt.
    #[error("HTTP {status}: {}", redact(.body_excerpt))]
    Http {
        /// The HTTP status code.
        status: u16,
        /// A bounded response-body excerpt, sanitized again when formatted.
        body_excerpt: String,
    },
    /// Capacity rejection with an optional server-supplied delay.
    #[error("rate limited")]
    RateLimited {
        /// The requested delay before another attempt.
        retry_after: Option<Duration>,
    },
    /// The gateway relay's own limit asked the caller to wait; not a provider failure, so a caller waits it out
    /// instead of spending its retry budget.
    #[error("throttled by the relay")]
    Throttled {
        /// The requested delay, at least one second.
        retry_after: Duration,
        /// Which relay limit refused the request.
        scope: ThrottleScope,
    },
    /// The gateway itself refused the request: the relay's response named its code in
    /// [`tqa_types::GATEWAY_ERROR_HEADER`]. Its body is the gateway's words, not the provider's, so none of it is kept.
    #[error("the gateway refused the request: {code} (HTTP {status})")]
    GatewayRefused {
        /// The gateway's stable error code, a bare token ([`tqa_types::gateway_error_code`]).
        code: String,
        /// The HTTP status code.
        status: u16,
        /// The requested delay before another attempt, when the gateway sent one.
        retry_after: Option<Duration>,
    },
    /// Service unavailable (503) with a server-supplied delay; a 503 without one stays `Http`.
    #[error("service unavailable")]
    Unavailable {
        /// The requested delay before another attempt.
        retry_after: Option<Duration>,
    },
    /// One configured network deadline expired.
    #[error("{0} timeout")]
    Timeout(TimeoutKind),
    /// A typed transient transport failure; DNS, TLS verification and generic I/O are excluded.
    #[error("{0}")]
    Connection(ConnectionFailure),
    /// The response ended before a finish reason; buffered content is retained.
    #[error("stream ended before a terminal completion")]
    StreamCut {
        /// Accumulated values, omitted from error formatting.
        partial: PartialCompletion,
    },
    /// Request data the adapter refuses to send, or a response the protocol doesn't support.
    #[error("protocol error: {}", redact(.0))]
    Protocol(String),
    /// An answer that couldn't be read, raised while reading the response: an unparsable chunk or an inconsistent tool
    /// call. The model loop asks again once, as for a cut stream; a second fails the run (CORE-E-040, LLM-U-050).
    #[error("malformed answer: {}", redact(.0))]
    Malformed(String),
    /// Assembled tool arguments are incomplete or invalid JSON.
    #[error("tool {index} arguments are invalid: {}", redact(.detail))]
    ToolArgsInvalid {
        /// The stream call index.
        index: u32,
        /// A diagnostic description, sanitized when formatted.
        detail: String,
    },
    /// Tool arguments stopped mid-JSON because the answer finished on the output limit.
    #[error("tool {index} arguments were cut at the output limit{}", tokens(*.output_tokens))]
    ToolArgsCut {
        /// The stream call index.
        index: u32,
        /// Output tokens the server reported, when it did.
        output_tokens: Option<u64>,
    },
    /// A non-streaming HTTP body exceeded its configured byte cap.
    #[error("HTTP body exceeds {max} bytes")]
    BodyTooLarge {
        /// Maximum permitted body bytes.
        max: usize,
    },
    /// One explicitly configured stream bound was exceeded.
    #[error("{which} exceeds configured maximum {max}")]
    StreamLimit {
        /// The exceeded bound.
        which: StreamLimitKind,
        /// Its configured maximum.
        max: usize,
    },
    /// The registry selected an adapter not implemented by this build.
    #[error("unsupported protocol: only openai_chat is implemented")]
    UnsupportedProtocol,
    /// The route configuration is invalid.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// The caller cancelled the operation.
    #[error("completion cancelled")]
    Cancelled,
    /// A transport or operating-system failure without credentials or headers.
    #[error("I/O failure: {}", redact(.0))]
    Io(String),
}

impl LlmError {
    /// Identifies transient failures; retry loops still respect attempt and cancellation limits.
    /// Status 529 is the overload status some servers send instead of 503.
    #[must_use]
    pub const fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. }
                | Self::Throttled { .. }
                | Self::Unavailable { .. }
                | Self::Timeout(_)
                | Self::Connection(_)
                | Self::StreamCut { .. }
                | Self::Http {
                    status: 408 | 429 | 500 | 502 | 503 | 504 | 529,
                    ..
                }
                | Self::GatewayRefused {
                    status: 408 | 429 | 500 | 502 | 503 | 504 | 529,
                    ..
                }
        )
    }
    /// Returns a stable diagnostic code, independent of explanatory text.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::CatalogLimit(_) => "tool_catalog_limit",
            Self::Http { .. } => "http",
            Self::GatewayRefused { .. } => "gateway",
            Self::RateLimited { .. } | Self::Throttled { .. } => "rate_limited",
            Self::Unavailable { .. } => "unavailable",
            Self::Timeout(_) => "timeout",
            Self::StreamCut { .. } => "stream_cut",
            Self::Protocol(_) => "protocol",
            Self::Malformed(_) => "malformed",
            Self::ToolArgsInvalid { .. } => "tool_args_invalid",
            Self::ToolArgsCut { .. } => "tool_args_cut",
            Self::BodyTooLarge { .. } => "body_too_large",
            Self::StreamLimit { .. } => "stream_limit",
            Self::UnsupportedProtocol => "unsupported_protocol",
            Self::Registry(_) => "registry",
            Self::Cancelled => "cancelled",
            Self::Io(_) | Self::Connection(_) => "io",
        }
    }
}

fn tokens(output_tokens: Option<u64>) -> String {
    output_tokens.map_or_else(String::new, |count| format!(" ({count} tokens)"))
}

impl fmt::Debug for LlmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("LlmError")
            .field(&self.to_string())
            .finish()
    }
}

impl LlmError {
    /// The provider's own words about an HTTP failure, sanitised and bounded for `run.failed` and `provider.retry`
    /// (LLM-U-036); `None` for every other error and for an empty body.
    #[must_use]
    pub fn provider_message(&self) -> Option<String> {
        match self {
            Self::Http { body_excerpt, .. } => crate::provider_message::sanitise(body_excerpt),
            _ => None,
        }
    }
}
