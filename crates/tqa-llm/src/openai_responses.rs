//! Streaming item-based responses (`POST {base}/responses`), configured entirely by route data.
//!
//! **Stateless by design.** Every request is built from our own history projection and sent with
//! `store: false`; `previous_response_id` and server-side conversations are never used, because
//! the journal is the authority and a resumed run must send exactly what the history says.
//!
//! What this costs and how it differs from Chat completions:
//! - Reasoning items are not sent back, so a reasoning model re-reasons at each step of a tool
//!   loop (more tokens, less continuity between calls). Reasoning text still streams as
//!   `ReasoningDelta`. Round-tripping opaque reasoning state is a separate, later change.
//! - Requests carry no participant `name` (the item format has none) and no `stop` sequences (the
//!   protocol has none); a request with stop sequences fails before sending.
//! - The reasoning effort always goes in `reasoning.effort`; the route's Chat `reasoning.field`
//!   does not apply.
//! - Each function call's arguments are buffered until that call's `done` event and then emitted
//!   whole, checked against the complete arguments the event carries. A cut stream therefore never
//!   carries part of a call; Chat streams argument fragments as they arrive.
//! - Event types this adapter does not know are ignored, so additive server features do not break
//!   a run. Errors inside the stream keep only their `code`, never the message, which can echo
//!   prompt content.

mod request;
#[cfg(test)]
mod tests;
mod wire;

use crate::{
    CompletionRequest, CompletionStream, HttpConfig, LlmError, ModelProvider, Route,
    transport::{Outgoing, Transport},
};
use async_trait::async_trait;
use secrecy::SecretString;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};

pub(crate) use wire::terminal_usage;

pub(crate) struct ResponsesProvider {
    transport: Transport,
}

impl ResponsesProvider {
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
impl ModelProvider for ResponsesProvider {
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
        let wire = wire::ResponsesWire::new(self.transport.http.stream);
        self.transport
            .stream(
                Outgoing {
                    path: "responses",
                    body: bytes,
                    run: request.run_id.as_ref(),
                },
                wire,
                cancel,
            )
            .await
    }
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
    crate::sse::response_stream(
        response,
        http,
        wire::ResponsesWire::new(http.stream),
        sleeper,
        cancel,
    )
}
