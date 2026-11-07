//! The relay's own refusals: one user's rate, or a route's limiter queue. Each is a 429 with `Retry-After` and
//! `TQA-Relay-Throttle`, so the runtime waits for it instead of spending its retry budget (ADR-0025). Upstream
//! responses never carry the header: the forwarded headers are an allowlist ([`super::forward::forwarded`]), so a
//! provider cannot forge a throttle and switch off route fallback.
use crate::{GatewayError, telemetry::Telemetry};
use axum::{
    http::HeaderValue,
    response::{IntoResponse as _, Response},
};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// The header marking a refusal as the relay's own; the runtime's provider reads the same constant.
pub(crate) use tqa_llm::{RELAY_THROTTLE_HEADER as HEADER, ThrottleScope as Scope};

/// One log line per conversation and scope in this window, like the egress denial notice; the metric counts every
/// refusal.
pub(super) const LOG_WINDOW: Duration = Duration::from_mins(1);
/// Log entries kept; expired ones are dropped first, so many throttled conversations cannot grow memory.
pub(super) const MAX_LOG_ENTRIES: usize = 4096;

/// One refusal: which limit, how long to wait, and for whom.
pub(super) struct Refusal<'a> {
    pub scope: Scope,
    pub seconds: u64,
    pub conversation: &'a str,
    pub route: &'a str,
}

/// When each conversation and scope was last logged, and how many lines have been logged in all.
#[derive(Default)]
pub(crate) struct Logs(Mutex<HashMap<(String, Scope), Instant>>, AtomicUsize);

impl Logs {
    /// Whether a line for this conversation and scope is due now; records it when it is.
    fn due(&self, conversation: &str, scope: Scope, now: Instant) -> bool {
        let Ok(mut logged) = self.0.lock() else {
            return false;
        };
        let key = (conversation.to_owned(), scope);
        if logged
            .get(&key)
            .is_some_and(|last| now.saturating_duration_since(*last) < LOG_WINDOW)
        {
            return false;
        }
        if logged.len() >= MAX_LOG_ENTRIES {
            logged.retain(|_, last| now.saturating_duration_since(*last) < LOG_WINDOW);
            if logged.len() >= MAX_LOG_ENTRIES {
                return false;
            }
        }
        logged.insert(key, now);
        self.1.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// How many lines have been logged.
    #[cfg(test)]
    pub(super) fn logged(&self) -> usize {
        self.1.load(Ordering::Relaxed)
    }
}

/// The refusal the runtime sees: counted every time, logged at most once per conversation and scope per
/// [`LOG_WINDOW`]; no credential or body is in the line.
pub(super) fn refuse(
    telemetry: &Telemetry,
    logs: &Logs,
    refusal: &Refusal<'_>,
    now: Instant,
) -> Response {
    let seconds = refusal.seconds.max(1);
    let scope = refusal.scope.as_str();
    telemetry.relay_throttled(refusal.route, scope);
    if logs.due(refusal.conversation, refusal.scope, now) {
        tracing::warn!(
            conversation_id = refusal.conversation,
            route = refusal.route,
            scope,
            retry_after = seconds,
            "relay throttled a model request"
        );
    }
    let mut response = GatewayError::RateLimited(seconds).into_response();
    response
        .headers_mut()
        .insert(HEADER, HeaderValue::from_static(scope));
    response
}
