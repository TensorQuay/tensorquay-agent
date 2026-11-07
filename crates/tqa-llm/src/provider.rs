//! The provider boundary used by the loop and by protocol adapters.

use crate::{CompletionRequest, CompletionStream, HttpConfig, LlmError, Protocol, Route};
use async_trait::async_trait;
use secrecy::SecretString;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};

/// Supplies streaming completions without exposing a transport to its caller.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Returns the route selected at construction.
    fn route(&self) -> &Route;
    /// Starts one attempt; the returned stream owns its request and observes cancellation.
    ///
    /// # Errors
    /// Reports invalid inputs, transport failures, HTTP errors or cancellation before streaming.
    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionStream, LlmError>;
}

/// Constructs the selected adapter with explicit HTTP, time and credential dependencies.
///
/// # Errors
/// Rejects invalid routes, missing credentials, unsupported protocols and HTTP client configuration errors.
pub fn provider_for(
    route: Route,
    secret: Option<SecretString>,
    http: HttpConfig,
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
) -> Result<Arc<dyn ModelProvider>, LlmError> {
    match route.protocol {
        Protocol::OpenAiChat => Ok(Arc::new(crate::openai_chat::ChatProvider::new(
            route, secret, http, clock, sleeper,
        )?)),
        Protocol::OpenAiResponses => Ok(Arc::new(crate::openai_responses::ResponsesProvider::new(
            route, secret, http, clock, sleeper,
        )?)),
        Protocol::AnthropicMessages => Ok(Arc::new(
            crate::anthropic_messages::MessagesProvider::new(route, secret, http, clock, sleeper)?,
        )),
    }
}

/// Returns the exact size in bytes of the request body the route's adapter would send, so a
/// caller can fit optional content, such as images, under the relay's request limit.
///
/// # Errors
/// Returns the adapter's encoding error.
pub fn request_body_len(route: &Route, request: &CompletionRequest) -> Result<usize, LlmError> {
    Ok(crate::encode_request(route, request)?.len())
}
