//! Streaming chat-completions transport configured entirely by route data.

mod request;
mod sse;
pub(crate) mod wire;

#[cfg(test)]
mod connection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wire_tests;
#[cfg(test)]
mod withheld_tests;

use crate::{
    CompletionRequest, CompletionStream, HttpConfig, LlmError, ModelProvider, Route, StreamLimits,
    SystemClock, TokioSleeper, http,
    transport::{Outgoing, Transport},
};
use async_trait::async_trait;
use secrecy::SecretString;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};

pub(crate) struct ChatProvider {
    pub(crate) transport: Transport,
}

impl ChatProvider {
    pub(crate) fn new(
        route: Route,
        secret: Option<SecretString>,
        http: HttpConfig,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Result<Self, LlmError> {
        Ok(Self {
            transport: Transport::new(route, secret, http, clock, sleeper)?,
        })
    }
}

#[async_trait]
impl ModelProvider for ChatProvider {
    fn route(&self) -> &Route {
        &self.transport.route
    }
    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionStream, LlmError> {
        if cancel.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        let bytes = request::encode(&self.transport.route, &request)?;
        self.transport
            .stream(
                Outgoing {
                    path: "chat/completions",
                    body: bytes,
                    run: request.run_id.as_ref(),
                },
                sse::ChatWire::default(),
                cancel,
            )
            .await
    }
}

/// Lists advertised model identifiers; an absent discovery endpoint falls back to the configured model.
///
/// # Errors
/// Returns transport, HTTP, malformed-body or body-cap errors; uses the documented HTTP deadlines.
pub async fn list_models(
    route: &Route,
    secret: Option<SecretString>,
) -> Result<Vec<String>, LlmError> {
    #[derive(serde::Deserialize)]
    struct Models {
        data: Vec<Model>,
    }
    #[derive(serde::Deserialize)]
    struct Model {
        id: String,
    }
    if route.protocol != crate::Protocol::OpenAiChat {
        return Err(LlmError::UnsupportedProtocol);
    }
    let provider = Transport::new(
        route.clone(),
        secret,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    )?;
    let cancel = CancellationToken::new();
    let mut response = provider
        .send(provider.request("models", reqwest::Method::GET)?, &cancel)
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        tracing::info!("model discovery is absent; using the configured model");
        return Ok(vec![route.model.clone()]);
    }
    if !response.status().is_success() {
        return Err(provider.status_error(response, &cancel).await?);
    }
    let bytes = http::body(
        &mut response,
        provider.http,
        provider.sleeper.as_ref(),
        &cancel,
    )
    .await?;
    let models: Models = serde_json::from_slice(&bytes)
        .map_err(|error| LlmError::Protocol(crate::redact(&error.to_string())))?;
    Ok(models.data.into_iter().map(|model| model.id).collect())
}

/// The exact request body this adapter sends.
pub(crate) fn encode(route: &Route, request: &CompletionRequest) -> Result<Vec<u8>, LlmError> {
    request::encode(route, request)
}

/// Decodes a streaming response with this protocol's wire, as the adapter does.
pub(crate) fn decode(
    response: reqwest::Response,
    http: HttpConfig,
    sleeper: Arc<dyn Sleeper>,
    cancel: CancellationToken,
) -> CompletionStream {
    crate::sse::response_stream(response, http, sse::ChatWire::default(), sleeper, cancel)
}
