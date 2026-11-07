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
        })
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
pub(crate) async fn send(
    request: reqwest::RequestBuilder,
    cancel: &CancellationToken,
) -> Result<(StatusCode, Vec<u8>), ChannelFailure> {
    let exchange = async {
        let mut response = request.send().await.map_err(|error| transport(&error))?;
        let status = response.status();
        let body = bounded_body(&mut response).await?;
        if status.is_success() {
            Ok((status, body))
        } else {
            Err(refusal(status, &body))
        }
    };
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(ChannelFailure::new("cancelled", "the call was cancelled")),
        result = exchange => result,
    }
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
        let (_, body) = send(
            self.request(Method::GET, self.url(&[]), "", CALL_TIMEOUT),
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
        let (_, body) = send(self.request(Method::GET, url, "", timeout), cancel).await?;
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
        let (_, body) = send(request, cancel).await?;
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
        let (status, body) =
            send(self.request(Method::POST, url, key, CALL_TIMEOUT), cancel).await?;
        Ok(Claimed {
            message: parse(&body)?,
            existing: status == StatusCode::OK,
        })
    }
}
