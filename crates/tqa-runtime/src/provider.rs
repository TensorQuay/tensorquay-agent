//! Only the gateway origin receives the runtime bearer credential.
use crate::{RuntimeConfig, RuntimeError, session::Session, time::Time};
use async_trait::async_trait;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    Auth, CompletionRequest, CompletionStream, HttpConfig, Limits, LlmError, ModelProvider,
    Protocol, Reasoning, Route, StreamLimits,
};
use tqa_types::WireProtocol;
pub(crate) fn build(
    config: &RuntimeConfig,
    session: &Session,
    time: &Time,
) -> Result<Arc<dyn ModelProvider>, RuntimeError> {
    for_route(config, &session.route, time)
}

/// One provider per admin-declared fallback route, each on its own relay path (ADR-0025).
pub(crate) fn fallbacks(
    config: &RuntimeConfig,
    session: &Session,
    time: &Time,
) -> Result<Vec<tqa_core::RouteProvider>, RuntimeError> {
    session
        .fallback_routes
        .iter()
        .map(|route| {
            Ok(tqa_core::RouteProvider {
                route: route.clone(),
                provider: for_route(config, route, time)?,
            })
        })
        .collect()
}

fn for_route(
    config: &RuntimeConfig,
    snapshot: &tqa_types::RouteSnapshot,
    time: &Time,
) -> Result<Arc<dyn ModelProvider>, RuntimeError> {
    let mut url = config.origin.clone();
    url.set_path(&snapshot.relay_path);
    // A protocol this runtime cannot speak fails closed: the run fails durably with no relay traffic.
    let protocol = match snapshot.protocol {
        WireProtocol::OpenAiChat => Some(Protocol::OpenAiChat),
        WireProtocol::OpenAiResponses => Some(Protocol::OpenAiResponses),
        WireProtocol::AnthropicMessages => Some(Protocol::AnthropicMessages),
        _ => None,
    };
    let route = Route {
        name: snapshot.name.clone(),
        // Unused for an unknown protocol: the `Unsupported` provider refuses every request.
        protocol: protocol.unwrap_or(Protocol::OpenAiChat),
        base_url: url.to_string(),
        model: snapshot.model.clone(),
        auth: Auth::Bearer,
        key_ref: Some("runtime-token-file".into()),
        limits: Limits {
            context_window: snapshot.context_window,
            max_output: snapshot.max_output,
            ..Limits::default()
        },
        native_tools: snapshot.native_tools,
        parallel_tools: snapshot.parallel_tools,
        vision: snapshot.vision,
        reasoning: snapshot.reasoning.as_ref().map(|r| Reasoning {
            field: "reasoning_effort".into(),
            allowed: r.allowed.clone(),
            default: r.default.clone(),
            budget_tokens: r.budget_tokens.clone(),
        }),
        cache_hint: snapshot.cache_hint,
        // Always asked for; the relay leaves it out for a route whose server refuses it.
        stream_usage: Some(true),
        extra_headers: BTreeMap::default(),
        extra_body: BTreeMap::default(),
        quirks: vec![],
        // The relay applies the route's tool-schema settings and instructions role; the runtime sends the defaults.
        strict: None,
        schema_keywords: None,
        instructions_role: None,
        // The runtime's wait follows the gateway's bound through relay_http, not this field.
        first_response_secs: None,
        fallbacks: vec![],
    };
    if protocol.is_none() {
        return Ok(Arc::new(Unsupported { route }));
    }
    tqa_llm::provider_for(
        route,
        Some(config.token.clone()),
        relay_http(snapshot.first_response_secs),
        time.clock.clone(),
        time.sleeper.clone(),
    )
    .map_err(|_| RuntimeError::Configuration("model adapter"))
}
struct Unsupported {
    route: Route,
}
#[async_trait]
impl ModelProvider for Unsupported {
    fn route(&self) -> &Route {
        &self.route
    }
    async fn complete(
        &self,
        _: CompletionRequest,
        _: CancellationToken,
    ) -> Result<CompletionStream, LlmError> {
        Err(LlmError::UnsupportedProtocol)
    }
}

/// How much longer than the relay the runtime waits for the first byte, so the relay's own refusal (a 504 naming the
/// gateway) is what the run sees.
const FIRST_BYTE_MARGIN: Duration = Duration::from_secs(10);

/// Every model call goes to the gateway's relay, never to a provider: a relay that is briefly unresolvable or
/// unreachable while the gateway restarts is retried like a refused connection (LLM-E-029). The first byte is awaited
/// a little longer than the route's first-response bound, which the relay applies (LLM-U-048); an older gateway sends
/// none, and the remote default applies.
fn relay_http(first_response_secs: Option<u32>) -> HttpConfig {
    let bound = first_response_secs.map_or(tqa_llm::FIRST_RESPONSE_REMOTE, |secs| {
        Duration::from_secs(u64::from(secs))
    });
    HttpConfig {
        peer: tqa_llm::Peer::Relay,
        first_byte_timeout: bound + FIRST_BYTE_MARGIN,
        ..HttpConfig::new(StreamLimits::RECOMMENDED)
    }
}

#[cfg(test)]
mod tests {
    // TEST: LLM-U-048
    #[test]
    fn llm_u_048_the_runtime_waits_a_little_longer_than_the_relay() {
        use std::time::Duration;
        // The relay's own refusal (a 504 naming the gateway) arrives first; an older gateway sends no bound.
        assert_eq!(
            super::relay_http(Some(300)).first_byte_timeout,
            Duration::from_secs(310)
        );
        assert_eq!(
            super::relay_http(Some(5)).first_byte_timeout,
            Duration::from_secs(15)
        );
        assert_eq!(
            super::relay_http(None).first_byte_timeout,
            Duration::from_secs(70)
        );
    }

    // TEST: LLM-E-029
    #[test]
    fn llm_e_029_the_runtime_talks_to_the_relay() {
        let http = super::relay_http(None);
        assert_eq!(http.peer, tqa_llm::Peer::Relay);
        assert_eq!(
            http.stream,
            tqa_llm::HttpConfig::new(tqa_llm::StreamLimits::RECOMMENDED).stream
        );
    }
}
