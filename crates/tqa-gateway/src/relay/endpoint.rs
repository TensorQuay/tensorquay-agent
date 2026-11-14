use super::{
    forward,
    request::{self, Operation},
    throttle::{self, Scope},
};
use crate::{GatewayError, app, request::ApiJson};
use axum::{
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    response::{IntoResponse as _, Response},
};
use secrecy::{ExposeSecret, SecretString};
use std::{net::SocketAddr, sync::Arc};
use tqa_llm::{Auth, Protocol, Route};
use tqa_types::{ConversationId, RunId};

/// Who calls which route: the runtime's address, the route in the path and the request headers.
struct Call<'a> {
    peer: SocketAddr,
    route: &'a str,
    headers: &'a HeaderMap,
}

struct Authorized {
    conversation: ConversationId,
    owner: uuid::Uuid,
    route: Route,
}

pub(crate) async fn chat(
    State(state): State<Arc<app::State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(route): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> Result<Response, GatewayError> {
    let call = Call {
        peer,
        route: &route,
        headers: &headers,
    };
    complete(Operation::Chat, state, call, body).await
}

pub(crate) async fn responses(
    State(state): State<Arc<app::State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(route): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> Result<Response, GatewayError> {
    let call = Call {
        peer,
        route: &route,
        headers: &headers,
    };
    complete(Operation::Responses, state, call, body).await
}

pub(crate) async fn messages(
    State(state): State<Arc<app::State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(route): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> Result<Response, GatewayError> {
    let call = Call {
        peer,
        route: &route,
        headers: &headers,
    };
    complete(Operation::Messages, state, call, body).await
}

/// One completion on the route's own protocol: a route of another protocol has no such path (404),
/// and a request the route's policy refuses is a bad request (400) that never goes upstream.
async fn complete(
    operation: Operation,
    state: Arc<app::State>,
    call: Call<'_>,
    mut body: serde_json::Value,
) -> Result<Response, GatewayError> {
    let Call {
        peer,
        route,
        headers,
    } = call;
    let protocol = operation.protocol().ok_or(GatewayError::NotFound)?;
    let caller = authorize(&state, headers, peer, route, protocol)
        .await
        .map_err(store_deadline)?;
    if let Some(refusal) = admit(&state, &caller) {
        return Ok(refusal);
    }
    tqa_llm::relay_shaping::shape_request(&caller.route, &mut body)
        .map_err(|_| GatewayError::BadRequest("relay request violates route policy"))?;
    let bytes = serde_json::to_vec(&body)?;
    // Only if the server reports no usage (a step cancelled mid-reply): its prompt, estimated from what goes out.
    let prompt = super::meter::prompt_estimate(&body);
    if bytes.len() > state.config.relay.body_bytes {
        return Err(GatewayError::BodyTooLarge);
    }
    // A server that just couldn't be reached is answered for at once, before the run's records and the limiter
    // (GW-I-320): the task's retries don't wait again, and nothing is metered for a call that goes nowhere.
    if state.relay.reach.held(&caller.route.name) {
        return Err(GatewayError::UpstreamUnavailable);
    }
    // The run the runtime names, kept only when it is this task's (the run-id contract); a malformed one is refused.
    let run = super::attribution::attributed(
        &state,
        &caller.conversation,
        run_id(headers)?,
        &caller.route.name,
    )
    .await
    .map_err(store_deadline)?;
    // An agent team's budget and deadline, before its key is read or anything is forwarded (ADR-0034 point 8).
    crate::teams::budget::check(&state, &caller.conversation)
        .await
        .map_err(store_deadline)?;
    let secret = credential(&state, &caller.route, &reader(&caller, run.as_ref())?).await?;
    let request = request::build(
        &state.relay.client,
        &caller.route,
        operation,
        secret.as_ref(),
    )?
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(bytes);
    // The route's limiter (LLM-U-012) before metering: a refused request is never forwarded and has no usage row.
    let limiter = state.relay.limiters.for_route(&caller.route);
    let queued = std::time::Instant::now();
    let permit = match limiter.acquire(&state.cancel).await {
        Ok(permit) => permit,
        Err(refusal) => {
            let seconds = refused(refusal)?;
            return Ok(throttled(&state, &caller, Scope::Route, seconds));
        }
    };
    state
        .telemetry
        .relay_queue(&caller.route.name, queued.elapsed().as_secs_f64());
    let meter = super::meter::Meter::start(&state, &caller.conversation, route, run.as_ref())
        .await
        .map_err(store_deadline)?
        .with_prompt_estimate(prompt);
    forward::send(state, request, &caller.route, Some(meter), Some(permit)).await
}

/// The wait a route limiter's refusal asks for, in whole seconds rounded up and never zero; cancellation is not a
/// refusal.
fn refused(refusal: tqa_llm::LimiterRefusal) -> Result<u64, GatewayError> {
    match refusal {
        tqa_llm::LimiterRefusal::QueueFull { retry_after }
        | tqa_llm::LimiterRefusal::Timeout { retry_after } => {
            let seconds = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
            Ok(seconds.max(1))
        }
        tqa_llm::LimiterRefusal::Cancelled => Err(GatewayError::Cancelled),
    }
}

/// One of the relay's own refusals for this caller, labelled with the route it resolved to.
fn throttled(state: &app::State, caller: &Authorized, scope: Scope, seconds: u64) -> Response {
    let conversation = caller.conversation.to_string();
    let refusal = throttle::Refusal {
        scope,
        seconds,
        conversation: &conversation,
        route: &caller.route.name,
    };
    throttle::refuse(
        &state.telemetry,
        &state.relay.throttle_logs,
        &refusal,
        std::time::Instant::now(),
    )
}

/// The user's relay rate, shared by all their conversations and routes: the refusal to send, if any, is a user
/// throttle.
fn admit(state: &app::State, caller: &Authorized) -> Option<Response> {
    match state.relay.rate.admit(
        caller.owner,
        std::time::Instant::now(),
        state.settings.current().limits.per_user,
    ) {
        Ok(()) => None,
        Err(GatewayError::RateLimited(seconds)) => {
            Some(throttled(state, caller, Scope::User, seconds))
        }
        Err(error) => Some(error.into_response()),
    }
}

pub(crate) async fn models(
    State(state): State<Arc<app::State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(route): Path<String>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    // Model discovery is a Chat-protocol endpoint.
    let caller = authorize(&state, &headers, peer, &route, Protocol::OpenAiChat)
        .await
        .map_err(store_deadline)?;
    if let Some(refusal) = admit(&state, &caller) {
        return Ok(refusal);
    }
    let secret = credential(&state, &caller.route, &reader(&caller, None)?).await?;
    let request = request::build(
        &state.relay.client,
        &caller.route,
        Operation::Models,
        secret.as_ref(),
    )?;
    forward::send(state, request, &caller.route, None, None).await
}
/// Authentication and metering wait only on the store, so their deadline is a store outage: 503, retryable.
fn store_deadline(error: GatewayError) -> GatewayError {
    match error {
        GatewayError::Timeout => GatewayError::StoreUnavailable,
        other => other,
    }
}

async fn authorize(
    state: &Arc<app::State>,
    headers: &HeaderMap,
    peer: SocketAddr,
    route: &str,
    protocol: Protocol,
) -> Result<Authorized, GatewayError> {
    let token = crate::auth::token::bearer(headers)?;
    let (conversation, _) = state
        .runtime_tokens
        .resolve(&token, peer.ip())
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    let id = crate::conversations::uuid(&conversation)?;
    let record = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "SELECT owner_id,model_route FROM conversations WHERE id=$1 AND state='active'",
            id
        )
        .fetch_optional(&state.pool)
        .await?
        .ok_or(GatewayError::Unauthorized)
    })
    .await?;
    // The conversation's route, or one of its admin-declared fallbacks (074, ADR-0025), from the same registry that
    // built `session.config`. There are no per-user route permissions yet; a later permission model filters here.
    // One settings snapshot for both lookups (ADR-0028); a changed route applies from the next request.
    let settings = state.settings.current();
    let allowed = record.model_route == route
        || settings
            .registry
            .get(&record.model_route)
            .is_some_and(|primary| primary.fallbacks.iter().any(|fallback| fallback == route));
    if !allowed {
        return Err(GatewayError::Forbidden("runtime route is not authorized"));
    }
    let route = settings
        .registry
        .get(route)
        .ok_or(GatewayError::NotFound)?
        .clone();
    if route.protocol != protocol {
        return Err(GatewayError::NotFound);
    }
    Ok(Authorized {
        conversation,
        owner: record.owner_id,
        route,
    })
}
pub(super) async fn credential(
    state: &app::State,
    route: &Route,
    context: &crate::secrets::reads::ReadContext,
) -> Result<Option<SecretString>, GatewayError> {
    if route.auth == Auth::None {
        return Ok(None);
    }
    let reference = route
        .key_ref
        .as_ref()
        .ok_or(GatewayError::SecretUnavailable)?
        .parse()
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let value = crate::secrets::reads::read(
        state,
        &reference,
        (crate::secrets::reads::ConsumerKind::Route, &route.name),
        context,
    )
    .await?
    .ok_or(GatewayError::SecretUnavailable)?;
    if value.expose_secret().is_empty() {
        return Err(GatewayError::SecretUnavailable);
    }
    Ok(Some(value))
}

/// Whose read a relayed request's key is.
fn reader(
    caller: &Authorized,
    run: Option<&RunId>,
) -> Result<crate::secrets::reads::ReadContext, GatewayError> {
    Ok(crate::secrets::reads::ReadContext {
        conversation: Some(crate::conversations::uuid(&caller.conversation)?),
        user: Some(caller.owner),
        run: run.map(|run| run.as_str().to_owned()),
    })
}
fn run_id(headers: &HeaderMap) -> Result<Option<RunId>, GatewayError> {
    let mut values = headers.get_all("x-tqa-run-id").iter();
    let first = values.next();
    if values.next().is_some() {
        return Err(GatewayError::BadRequest("run identifier must appear once"));
    }
    first
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse().ok())
                .ok_or(GatewayError::BadRequest("invalid run identifier"))
        })
        .transpose()
}
