//! The request encoder and response decoder the protocol adapters use, for callers that talk to a route without a
//! provider: the gateway's route probe sends and reads exactly what a runtime would (LLM-C-028).

use crate::{CompletionRequest, CompletionStream, HttpConfig, LlmError, Protocol, Route};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tqa_types::Sleeper;

/// The exact request body the route's adapter sends for this request.
///
/// # Errors
/// Returns the adapter's encoding error for inputs the route or protocol cannot send.
pub fn encode_request(route: &Route, request: &CompletionRequest) -> Result<Vec<u8>, LlmError> {
    match route.protocol {
        Protocol::OpenAiChat => crate::openai_chat::encode(route, request),
        Protocol::OpenAiResponses => crate::openai_responses::encode(route, request),
        Protocol::AnthropicMessages => crate::anthropic_messages::encode(route, request),
    }
}

/// Decodes a successful streaming response with the protocol's own wire, bounds and assembler checks, exactly as
/// its adapter does. The caller checks the HTTP status first.
#[must_use]
pub fn decode_response(
    protocol: Protocol,
    response: reqwest::Response,
    http: HttpConfig,
    sleeper: Arc<dyn Sleeper>,
    cancel: CancellationToken,
) -> CompletionStream {
    match protocol {
        Protocol::OpenAiChat => crate::openai_chat::decode(response, http, sleeper, cancel),
        Protocol::OpenAiResponses => {
            crate::openai_responses::decode(response, http, sleeper, cancel)
        }
        Protocol::AnthropicMessages => {
            crate::anthropic_messages::decode(response, http, sleeper, cancel)
        }
    }
}
