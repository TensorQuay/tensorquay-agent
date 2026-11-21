use super::{meter::Meter, tap::UsageTap};
use crate::{GatewayError, app};
use axum::{
    body::{Body, Bytes},
    response::Response,
};
use futures::{Stream, StreamExt};
use std::{io, pin::Pin, sync::Arc, time::Duration};

struct Forward {
    source: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    meter: Option<Meter>,
    // The route limiter's permit, held until the relayed response ends (LLM-U-012).
    _permit: Option<tqa_llm::LimiterPermit>,
    tap: UsageTap,
    state: Arc<app::State>,
    deadline: tokio::time::Instant,
    received: usize,
    finished: bool,
    /// Whether the body is an event stream to meter; `None` until a text/plain body's start says (GW-U-062).
    is_sse: Option<bool>,
    sniffer: super::stream_kind::Sniffer,
    /// The chunks forwarded while undecided, for the meter once the body is known to be a stream.
    undecided: Vec<Bytes>,
    /// The route, for the one warning a mislabelled stream gets and the line an early end gets.
    route: String,
    /// When the response began, for the line an early end gets.
    started: tokio::time::Instant,
}
/// The upstream headers the runtime sees: an allowlist, so nothing else, including a forged
/// [`super::throttle::HEADER`], passes through.
pub(super) fn forwarded(upstream: &axum::http::HeaderMap) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    for name in [
        "content-type",
        "content-encoding",
        "content-length",
        "retry-after",
        "cache-control",
    ] {
        if let Some(value) = upstream.get(name) {
            headers.insert(name, value.clone());
        }
    }
    headers
}

pub(super) async fn send(
    state: Arc<app::State>,
    request: reqwest::RequestBuilder,
    route: &tqa_llm::Route,
    meter: Option<Meter>,
    permit: Option<tqa_llm::LimiterPermit>,
) -> Result<Response, GatewayError> {
    // The route's first-response bound (GW-I-150): a CPU model server on the same machine reads a long prompt for a
    // minute or more before its first byte.
    let first_byte = Duration::from_secs(u64::from(crate::link::first_response_secs(route)));
    // A server that just couldn't be reached is answered for at once (GW-I-320): a task's retries don't each wait for
    // the connection again, and the hold is short, so a server that comes back is tried within it.
    if state.relay.reach.held(&route.name) {
        return Err(GatewayError::UpstreamUnavailable);
    }
    let reach = &state.relay.reach;
    let upstream = crate::io::bounded(first_byte, &state.cancel, async {
        request.send().await.map_err(|error| {
            // Only a failed connection (refused, no such host, connect timeout) marks the route; a slow answer doesn't.
            if error.is_connect() {
                reach.unreachable(&route.name);
            }
            GatewayError::UpstreamUnavailable
        })
    })
    .await?;
    reach.reached(&route.name);
    let status = upstream.status();
    let headers = forwarded(upstream.headers());
    let content_type = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok());
    let is_sse = if super::stream_kind::sniffed(content_type) {
        None
    } else {
        Some(super::stream_kind::classify(content_type, b"").is_stream())
    };
    let pending = Forward {
        source: Box::pin(upstream.bytes_stream()),
        meter,
        _permit: permit,
        tap: UsageTap::new(route.protocol, state.config.relay.sse_event_bytes),
        deadline: tokio::time::Instant::now()
            + Duration::from_secs(state.config.relay.response_timeout),
        state,
        received: 0,
        finished: false,
        is_sse,
        sniffer: super::stream_kind::Sniffer::default(),
        undecided: Vec::new(),
        route: route.name.clone(),
        started: tokio::time::Instant::now(),
    };
    let body = Body::from_stream(futures::stream::unfold(pending, |mut pending| async move {
        let next = pending.next().await?;
        Some((next, pending))
    }));
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    Ok(response)
}
impl Forward {
    async fn next(&mut self) -> Option<Result<Bytes, io::Error>> {
        if self.finished {
            return None;
        }
        let idle = Duration::from_secs(self.state.config.relay.idle_timeout);
        let next = next_within(&mut self.source, (idle, self.deadline), &self.state.cancel).await;
        match next {
            Ok(Some(Ok(bytes))) => Some(self.observe(bytes)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Ok(Some(Err(_))) => {
                self.finished = true;
                Some(Err(self.ended("upstream response interrupted")))
            }
            Err(error) => {
                self.finished = true;
                Some(Err(self.ended(&error.to_string())))
            }
        }
    }
    /// Ends the response before its end: the error the runtime's read gets, and one line saying why.
    fn ended(&self, reason: &str) -> io::Error {
        ended(&self.route, reason, self.received, self.started.elapsed());
        io::Error::other(reason.to_owned())
    }
    /// A text/plain body's start decided it: a stream is metered from its first byte, with one warning.
    fn decided(&mut self, kind: super::stream_kind::StreamKind) {
        if kind == super::stream_kind::StreamKind::Mislabelled {
            tracing::warn!(route = %self.route, "upstream event stream is labelled text/plain; metered as a stream");
        }
        self.is_sse = Some(kind.is_stream());
        let earlier = std::mem::take(&mut self.undecided);
        if kind.is_stream() {
            for chunk in &earlier {
                self.tap.push(chunk);
            }
            if let Some(meter) = &mut self.meter {
                meter.observe(self.tap.latest());
                meter.observe_streamed(self.tap.streamed());
            }
        }
    }
    fn observe(&mut self, bytes: Bytes) -> Result<Bytes, io::Error> {
        self.received = self.received.saturating_add(bytes.len());
        if self.received > self.state.config.relay.response_bytes {
            self.finished = true;
            return Err(self.ended("relay response byte limit"));
        }
        if self.is_sse.is_none() {
            self.undecided.push(bytes.clone());
            if let Some(kind) = self.sniffer.push(&bytes) {
                self.decided(kind);
            }
            return Ok(bytes);
        }
        if self.is_sse == Some(true) {
            self.tap.push(&bytes);
            if let Some(meter) = &mut self.meter {
                meter.observe(self.tap.latest());
                meter.observe_streamed(self.tap.streamed());
            }
        }
        Ok(bytes)
    }
}

/// The next chunk of a relayed response, unless the gateway stops, the response's deadline passes, or the stream is
/// silent for `idle` (`[relay] idle_timeout`, 300 s by default: a server may send a tool call whole once its arguments
/// are complete, after a long silence). Each ending has its own words for the "ended early" line.
pub(super) async fn next_within<S, T>(
    source: &mut S,
    (idle, deadline): (Duration, tokio::time::Instant),
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<Option<T>, io::Error>
where
    S: Stream<Item = T> + Unpin,
{
    tokio::select! {biased;
        () = cancel.cancelled() => Err(io::Error::other("relay cancelled")),
        () = tokio::time::sleep_until(deadline) => Err(io::Error::other("relay response deadline")),
        next = tokio::time::timeout(idle, source.next()) => next.map_err(|_| io::Error::other("relay response idle deadline")),
    }
}

/// The line a relayed response gets when the relay ends it before its end (a model server silent past the idle limit,
/// the response deadline, the byte limit, the upstream breaking, or the gateway stopping). The runtime only sees its
/// read fail (`stream_cut`), so this line is where the reason is kept (found on 19 Nov 2026: a server that held a long
/// tool call back was cut at the idle limit with nothing in the log).
pub(super) fn ended(route: &str, reason: &str, received: usize, elapsed: Duration) {
    tracing::warn!(
        route = %route,
        reason,
        received_bytes = received,
        elapsed_seconds = elapsed.as_secs(),
        "relayed response ended early"
    );
}

#[cfg(test)]
#[path = "forward_tests.rs"]
mod tests;
