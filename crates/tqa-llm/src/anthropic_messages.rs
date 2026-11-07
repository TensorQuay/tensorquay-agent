//! Streaming message-block completions (`POST {base}/messages`), configured entirely by route data.
//!
//! **Stateless by design**, like the Responses adapter: every request is built from our own
//! history projection. How it differs from Chat completions:
//! - System messages before the first turn become the top-level `system` blocks. A system
//!   message later in the history (a notice) has no role here and is sent as user text.
//! - Turns alternate and start with a user turn: consecutive same-role messages merge, and every
//!   tool result is a `tool_result` block of the next user turn.
//! - `max_tokens` is required, so the route declares `max_output`. Reasoning is a thinking token
//!   budget from the route's `budget_tokens` table, never an effort field; with thinking on, a
//!   temperature is refused because the protocol refuses the pair.
//! - Thinking blocks and their signatures are opaque reasoning state (ADR-0027), replayed first in
//!   their assistant turn. A request whose latest tool turn lost its state is sent without
//!   thinking and without states, since the protocol refuses such a turn while thinking is on.
//! - Each tool call is buffered until its block stops and then emitted whole, as on Responses.
//! - Events and block types this adapter does not know are ignored. Errors inside the stream keep
//!   only their `type`, never the message, which can echo prompt content.

mod request;
mod wire;

pub(crate) use wire::{WireUsage as MessagesUsage, usage_update};

use crate::{
    CompletionRequest, CompletionStream, HttpConfig, LlmError, ModelProvider, Route,
    transport::{Outgoing, Transport},
};
use async_trait::async_trait;
use secrecy::SecretString;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};

/// The protocol's version header, required on every request.
pub const MESSAGES_VERSION_HEADER: &str = "anthropic-version";
/// The pinned protocol version; a route cannot override it.
pub const MESSAGES_VERSION: &str = "2023-06-01";

pub(crate) struct MessagesProvider {
    transport: Transport,
}

impl MessagesProvider {
    pub(crate) fn new(
        route: Route,
        secret: Option<SecretString>,
        http: HttpConfig,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Result<Self, LlmError> {
        Ok(Self {
            transport: Transport::new(route, secret, http, clock, sleeper)?
                .with_protocol_headers(&[(MESSAGES_VERSION_HEADER, MESSAGES_VERSION)]),
        })
    }
}

#[async_trait]
impl ModelProvider for MessagesProvider {
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
        let wire = wire::MessagesWire::new(self.transport.http.stream);
        self.transport
            .stream(
                Outgoing {
                    path: "messages",
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
        wire::MessagesWire::new(http.stream),
        sleeper,
        cancel,
    )
}
