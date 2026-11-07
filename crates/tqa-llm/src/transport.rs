//! The HTTP side shared by the protocol adapters: route URLs, one place per credential kind,
//! deadlines, status classification with `Retry-After`, and the owned SSE response stream.

mod retry_after;

use crate::{
    Auth, CompletionStream, HttpConfig, LlmError, RegistryError, Route, TimeoutKind, http,
    sse::{self, Wire},
};
use secrecy::{ExposeSecret as _, SecretString};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};

/// The header that attributes a relayed call to its run (a validated identifier, so always a valid header value).
pub const RUN_HEADER: &str = "x-tqa-run-id";

/// One encoded completion request: the adapter's endpoint, its body, and the run it belongs to.
pub(crate) struct Outgoing<'a> {
    pub(crate) path: &'a str,
    pub(crate) body: Vec<u8>,
    pub(crate) run: Option<&'a tqa_types::RunId>,
}

pub(crate) struct Transport {
    pub(crate) route: Route,
    secret: Option<SecretString>,
    pub(crate) client: reqwest::Client,
    pub(crate) http: HttpConfig,
    clock: Arc<dyn Clock>,
    pub(crate) sleeper: Arc<dyn Sleeper>,
    /// Fixed headers the protocol requires on every completion request.
    protocol_headers: &'static [(&'static str, &'static str)],
}

impl Transport {
    pub(crate) fn new(
        route: Route,
        secret: Option<SecretString>,
        http: HttpConfig,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Result<Self, LlmError> {
        crate::registry::validate_route(&route)
            .map_err(|reason| RegistryError::at(&reason.to_string(), "", None))?;
        if route.auth != Auth::None
            && secret
                .as_ref()
                .is_none_or(|secret| secret.expose_secret().is_empty())
        {
            return Err(LlmError::Protocol(
                "authenticated route requires a resolved credential".into(),
            ));
        }
        let client = http::client(http, Arc::new(http::dns::SystemDns))?;
        Ok(Self {
            route,
            secret,
            client,
            http,
            clock,
            sleeper,
            protocol_headers: &[],
        })
    }

    /// Sends these fixed headers with every completion request.
    pub(crate) const fn with_protocol_headers(
        mut self,
        headers: &'static [(&'static str, &'static str)],
    ) -> Self {
        self.protocol_headers = headers;
        self
    }

    pub(crate) fn request(
        &self,
        path: &str,
        method: reqwest::Method,
    ) -> Result<reqwest::RequestBuilder, LlmError> {
        let mut url = url::Url::parse(&self.route.base_url)
            .map_err(|_| LlmError::Protocol("invalid route URL".into()))?;
        url.set_path(&format!("{}/{path}", url.path().trim_end_matches('/')));
        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in &self.route.extra_headers {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| LlmError::Protocol("invalid header name".into()))?,
                reqwest::header::HeaderValue::from_str(value)
                    .map_err(|_| LlmError::Protocol("invalid header value".into()))?,
            );
        }
        if let Some(secret) = &self.secret {
            let secret = secret.expose_secret();
            let header = match &self.route.auth {
                Auth::None => None,
                Auth::Bearer => Some(("authorization", format!("Bearer {secret}"))),
                Auth::Header { name } => Some((name.as_str(), secret.to_owned())),
                Auth::Query { name } => {
                    url.query_pairs_mut().append_pair(name, secret);
                    None
                }
            };
            if let Some((name, value)) = header {
                let mut value = reqwest::header::HeaderValue::from_str(&value)
                    .map_err(|_| LlmError::Protocol("invalid credential header value".into()))?;
                value.set_sensitive(true);
                headers.insert(
                    reqwest::header::HeaderName::from_bytes(name.as_bytes())
                        .map_err(|_| LlmError::Protocol("invalid credential header name".into()))?,
                    value,
                );
            }
        }
        Ok(self.client.request(method, url).headers(headers))
    }

    pub(crate) async fn send(
        &self,
        request: reqwest::RequestBuilder,
        cancel: &CancellationToken,
    ) -> Result<reqwest::Response, LlmError> {
        let started = self.clock.now();
        let response = http::deadline(
            request.send(),
            self.sleeper.as_ref(),
            self.http.first_byte_timeout,
            cancel,
            TimeoutKind::FirstByte,
        )
        .await?
        .map_err(|error| http::before_headers_error(error, self.http.peer))?;
        tracing::debug!(elapsed = ?self.clock.now().saturating_duration_since(started), "completion response headers received");
        Ok(response)
    }

    pub(crate) async fn status_error(
        &self,
        mut response: reqwest::Response,
        cancel: &CancellationToken,
    ) -> Result<LlmError, LlmError> {
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| retry_after::parse(value, SystemTime::now()));
        let throttle = response
            .headers()
            .get(crate::RELAY_THROTTLE_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(crate::ThrottleScope::parse);
        let gateway = response
            .headers()
            .get(tqa_types::GATEWAY_ERROR_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(tqa_types::gateway_error_code)
            .map(str::to_owned);
        let bytes = http::body(&mut response, self.http, self.sleeper.as_ref(), cancel).await?;
        if status == 429
            && let Some(scope) = throttle
        {
            return Ok(LlmError::Throttled {
                retry_after: retry_after
                    .unwrap_or(Duration::from_secs(1))
                    .max(Duration::from_secs(1)),
                scope,
            });
        }
        // The gateway's own refusal: its code, never the body (the relay's throttle above keeps its meaning).
        if let Some(code) = gateway {
            return Ok(LlmError::GatewayRefused {
                code,
                status,
                retry_after,
            });
        }
        if status == 429 {
            return Ok(LlmError::RateLimited { retry_after });
        }
        if status == 503 && retry_after.is_some() {
            return Ok(LlmError::Unavailable { retry_after });
        }
        let mut excerpt = crate::redact(&String::from_utf8_lossy(&bytes));
        if let Some(secret) = &self.secret
            && !secret.expose_secret().is_empty()
        {
            excerpt = excerpt.replace(secret.expose_secret(), "[redacted]");
        }
        Ok(LlmError::Http {
            status,
            body_excerpt: excerpt,
        })
    }

    /// Posts one encoded streaming request and returns its decoded response stream.
    pub(crate) async fn stream<W: Wire + 'static>(
        &self,
        outgoing: Outgoing<'_>,
        wire: W,
        cancel: CancellationToken,
    ) -> Result<CompletionStream, LlmError> {
        if cancel.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        let Outgoing { path, body, run } = outgoing;
        let mut request = self
            .request(path, reqwest::Method::POST)?
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "text/event-stream");
        for (name, value) in self.protocol_headers {
            request = request.header(*name, *value);
        }
        // Only the relay attributes a call to its run; a provider never learns the run's identifier.
        if let (crate::Peer::Relay, Some(run)) = (self.http.peer, run) {
            request = request.header(RUN_HEADER, run.as_str());
        }
        let request = request.body(body);
        let response = self.send(request, &cancel).await?;
        if !response.status().is_success() {
            return Err(self.status_error(response, &cancel).await?);
        }
        Ok(sse::response_stream(
            response,
            self.http,
            wire,
            self.sleeper.clone(),
            cancel,
        ))
    }
}
