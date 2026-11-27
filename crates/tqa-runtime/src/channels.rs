//! The gateway's runtime channel routes (ADR-0014 point 6), for the channel tools: `<gateway origin>/runtime/channels…`
//! with the runtime token. The URL is built here from the origin and a validated channel id, never from a tool's
//! arguments. No environment proxy, no redirects, a bounded body, and every call ends promptly when cancelled. Failures
//! keep the gateway's stable codes; the client's own are `gateway_unreachable`, `gateway_error`, `timeout`,
//! `response_too_large`, `cancelled` and `invalid_request`.
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Method, StatusCode, header::HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tqa_tools::builtin::channel::{
    ChannelClient, ChannelFailure, ChannelPage, ChannelSummary, Claimed, ReadRequest,
};
use tqa_types::{ChannelMessage, ChannelPost};
use url::Url;

use crate::RuntimeError;

/// The most bytes of one answer: 200 messages of 16 KiB, with room for their envelopes.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
/// A plain call's deadline; a long poll gets its wait on top.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Beyond a long poll's own wait, the time the gateway has to answer it.
const WAIT_GRACE: Duration = Duration::from_secs(10);

/// The channel routes of one gateway, as one run's conversation.
pub struct GatewayChannels {
    client: reqwest::Client,
    origin: Url,
    authorization: HeaderValue,
    /// How long a call keeps trying through a gateway that is restarting ([`send_retrying`]).
    retry_window: Duration,
}

impl GatewayChannels {
    /// A client for `origin` with the run's `token`.
    ///
    /// # Errors
    /// `Configuration` when the token can't be a header or the client can't be built.
    pub fn new(origin: &Url, token: &SecretString) -> Result<Self, RuntimeError> {
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
            .map_err(|_| RuntimeError::Configuration("channel client credential"))?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| RuntimeError::Configuration("channel client"))?;
        Ok(Self {
            client,
            origin: origin.clone(),
            authorization,
            retry_window: RETRY_WINDOW,
        })
    }

    /// The same client with another retry window (a test's own).
    #[must_use]
    pub const fn with_retry_window(mut self, window: Duration) -> Self {
        self.retry_window = window;
        self
    }

    /// `/runtime/channels` followed by `segments`, each a validated path segment.
    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.origin.clone();
        let mut path = String::from("/runtime/channels");
        for segment in segments {
            path.push('/');
            path.push_str(segment);
        }
        url.set_path(&path);
        url.set_query(None);
        url
    }

    /// A request with the credential, the deadline and, when given, the idempotency key.
    fn request(
        &self,
        method: Method,
        url: Url,
        key: &str,
        timeout: Duration,
    ) -> reqwest::RequestBuilder {
        let request = self
            .client
            .request(method, url)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .timeout(timeout);
        if key.is_empty() {
            request
        } else {
            request.header("idempotency-key", key)
        }
    }
}

/// Sends `request` and reads its answer within the bounds; `Ok((status, body))` for any 2xx.
/// How long a team or channel call keeps trying through a gateway that is restarting (RT-I-068): a gateway restart
/// takes seconds; its database coming back can take longer.
pub(crate) const RETRY_WINDOW: Duration = Duration::from_mins(1);

pub(crate) async fn send(
    request: reqwest::RequestBuilder,
    cancel: &CancellationToken,
) -> Result<(StatusCode, Vec<u8>), ChannelFailure> {
    attempt(request, cancel).await.0
}

/// The first pause before a call is sent again, doubled each time up to [`LONGEST_PAUSE`].
const FIRST_PAUSE: Duration = Duration::from_millis(250);
/// The longest pause between two sendings of one call.
const LONGEST_PAUSE: Duration = Duration::from_secs(4);

/// [`send`], and sent again while the gateway is restarting (RT-I-068): a call that couldn't reach the gateway, or that
/// it answered 503 or with an error it marks `retryable`, goes again after a pause that doubles, until `window` ends.
/// The request is the same each time, its idempotency key included, so the gateway answers a call it already took
/// with its first answer. A refusal, a timeout (the gateway may still be working on it) and a cancel end it at once;
/// past the window, the last failure is the answer. A request whose body can't be copied is sent once.
pub(crate) async fn send_retrying(
    request: reqwest::RequestBuilder,
    window: Duration,
    cancel: &CancellationToken,
) -> Result<(StatusCode, Vec<u8>), ChannelFailure> {
    let deadline = tokio::time::Instant::now() + window;
    let mut pause = FIRST_PAUSE;
    loop {
        let Some(this) = request.try_clone() else {
            return send(request, cancel).await;
        };
        match attempt(this, cancel).await {
            (Err(failure), true) if tokio::time::Instant::now() + pause < deadline => {
                tracing::debug!(
                    code = %failure.code,
                    pause_ms = u64::try_from(pause.as_millis()).unwrap_or(u64::MAX),
                    "the gateway didn't take the call; it is sent again"
                );
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => return Err(cancelled()),
                    () = tokio::time::sleep(pause) => {}
                }
                pause = (pause * 2).min(LONGEST_PAUSE);
            }
            (result, _) => return result,
        }
    }
}

fn cancelled() -> ChannelFailure {
    ChannelFailure::new("cancelled", "the call was cancelled")
}

/// One sending: its result, and whether the failure is one a restarting gateway gives (the gateway unreachable, a 503,
/// or an error the gateway marks `retryable`).
async fn attempt(
    request: reqwest::RequestBuilder,
    cancel: &CancellationToken,
) -> (Result<(StatusCode, Vec<u8>), ChannelFailure>, bool) {
    let exchange = async {
        let mut response = match request.send().await {
            Ok(response) => response,
            Err(error) => return (Err(transport(&error)), !error.is_timeout()),
        };
        let status = response.status();
        let body = match bounded_body(&mut response).await {
            Ok(body) => body,
            Err(failure) => return (Err(failure), false),
        };
        if status.is_success() {
            (Ok((status, body)), false)
        } else {
            let again = status == StatusCode::SERVICE_UNAVAILABLE || marked_retryable(&body);
            (Err(refusal(status, &body)), again)
        }
    };
    tokio::select! {
        biased;
        () = cancel.cancelled() => (Err(cancelled()), false),
        result = exchange => result,
    }
}

/// Whether the gateway's error envelope says the call may be tried again (`"retryable": true`).
fn marked_retryable(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|envelope| envelope["error"]["retryable"].as_bool())
        .unwrap_or(false)
}

fn transport(error: &reqwest::Error) -> ChannelFailure {
    if error.is_timeout() {
        ChannelFailure::new("timeout", "the gateway did not answer in time")
    } else {
        ChannelFailure::new(
            "gateway_unreachable",
            "the gateway could not be reached; try again",
        )
    }
}

/// The whole body, at most [`MAX_BODY_BYTES`].
async fn bounded_body(response: &mut reqwest::Response) -> Result<Vec<u8>, ChannelFailure> {
    let mut body = Vec::new();
    loop {
        let chunk = response.chunk().await.map_err(|_| {
            ChannelFailure::new("gateway_error", "the gateway's answer was cut off")
        })?;
        let Some(chunk) = chunk else {
            return Ok(body);
        };
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(ChannelFailure::new(
                "response_too_large",
                "the gateway's answer was larger than 4 MiB; read fewer messages",
            ));
        }
        body.extend_from_slice(&chunk);
    }
}

/// The gateway's own code and message when its error envelope is there; otherwise a generic `gateway_error`.
fn refusal(status: StatusCode, body: &[u8]) -> ChannelFailure {
    let envelope: Option<Value> = serde_json::from_slice(body).ok();
    let error = envelope.as_ref().map(|value| &value["error"]);
    match error.and_then(|error| {
        Some((
            error["code"].as_str()?,
            error["message"].as_str().unwrap_or_default(),
        ))
    }) {
        Some((code, message)) => {
            let message = match error.and_then(|error| error["field"].as_str()) {
                Some(field) => format!("{field}: {message}"),
                None => message.to_owned(),
            };
            ChannelFailure::new(code, &message)
                .with_details(error.and_then(|error| error.get("details")).cloned())
        }
        None => ChannelFailure::new(
            "gateway_error",
            &format!("the gateway answered {}", status.as_u16()),
        ),
    }
}

pub(crate) fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, ChannelFailure> {
    serde_json::from_slice(body)
        .map_err(|_| ChannelFailure::new("gateway_error", "the gateway's answer could not be read"))
}

/// A channel id that can only name one path segment (`[A-Za-z0-9_-]{1,64}`, the gateway's identifier rule).
fn channel_id(id: &str) -> Result<&str, ChannelFailure> {
    let valid = (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if valid {
        Ok(id)
    } else {
        Err(ChannelFailure::new(
            "invalid_request",
            "channel: expected 1 to 64 letters, digits, '_' or '-'",
        ))
    }
}

/// A serde enum's wire name.
fn wire_name<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[async_trait]
impl ChannelClient for GatewayChannels {
    async fn channels(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<ChannelSummary>, ChannelFailure> {
        #[derive(serde::Deserialize)]
        struct Listed {
            channels: Vec<ChannelSummary>,
        }
        let (_, body) = send_retrying(
            self.request(Method::GET, self.url(&[]), "", CALL_TIMEOUT),
            self.retry_window,
            cancel,
        )
        .await?;
        Ok(parse::<Listed>(&body)?.channels)
    }

    async fn read(
        &self,
        channel: &str,
        request: &ReadRequest,
        cancel: &CancellationToken,
    ) -> Result<ChannelPage, ChannelFailure> {
        let mut url = self.url(&[channel_id(channel)?, "messages"]);
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("since", &request.since.to_string());
            query.append_pair("limit", &request.limit.to_string());
            let filter = &request.filter;
            if !filter.types.is_empty() {
                let types: Vec<String> = filter.types.iter().map(wire_name).collect();
                query.append_pair("types", &types.join(","));
            }
            if let Some(parent) = filter.parent {
                query.append_pair("parent", &parent.to_string());
            }
            if let Some(task) = filter.task_seq {
                query.append_pair("task_seq", &task.to_string());
            }
            if let Some(kind) = &filter.author_kind {
                query.append_pair("author_kind", &wire_name(kind));
            }
            if let Some(wait) = request.wait {
                query.append_pair("wait", &wait.as_secs().to_string());
            }
        }
        // The comma list is part of the contract; keep it readable rather than percent-encoded.
        let query = url.query().map(|query| query.replace("%2C", ","));
        url.set_query(query.as_deref());
        let timeout = request.wait.map_or(CALL_TIMEOUT, |wait| wait + WAIT_GRACE);
        let (_, body) = send_retrying(
            self.request(Method::GET, url, "", timeout),
            self.retry_window,
            cancel,
        )
        .await?;
        parse(&body)
    }

    async fn post(
        &self,
        channel: &str,
        post: &ChannelPost,
        key: &str,
        cancel: &CancellationToken,
    ) -> Result<ChannelMessage, ChannelFailure> {
        let url = self.url(&[channel_id(channel)?, "messages"]);
        let request = self
            .request(Method::POST, url, key, CALL_TIMEOUT)
            .json(post);
        let (_, body) = send_retrying(request, self.retry_window, cancel).await?;
        parse(&body)
    }

    async fn claim(
        &self,
        channel: &str,
        task_seq: u64,
        key: &str,
        cancel: &CancellationToken,
    ) -> Result<Claimed, ChannelFailure> {
        let task = task_seq.to_string();
        let url = self.url(&[channel_id(channel)?, "tasks", &task, "claim"]);
        let (status, body) = send_retrying(
            self.request(Method::POST, url, key, CALL_TIMEOUT),
            self.retry_window,
            cancel,
        )
        .await?;
        Ok(Claimed {
            message: parse(&body)?,
            existing: status == StatusCode::OK,
        })
    }
}
