//! `GET /api/v1/admin/routes/{name}/models` (GW-I-112, SEC-042): an administrator lists the models a route's endpoint
//! offers, with the route's stored credential, which never leaves the gateway. The request is the relay's own
//! (`{base_url}/models`, the route's auth and headers, no redirects), admitted like a probe: one listing per route at a
//! time, rate-limited per route and per administrator. A refusal by the endpoint comes back as the probe names it,
//! redacted; the answer keeps only ids and display names, at most [`models_list::MAX_MODELS`].
use super::{
    endpoint::credential,
    models_list::{self, Collected},
    probe::{self, Failure},
    probe_api::Admission,
    request::{self, Operation},
};
use crate::{GatewayError, admin::require_admin, app, auth::Authz, settings};
use axum::{
    Json,
    extract::{Path, State as AppState},
    response::{IntoResponse as _, Response},
};
use futures::StreamExt as _;
use secrecy::SecretString;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tqa_llm::{Protocol, Route};

/// The longest one listing may take, every page included.
pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(30);
/// The most bytes read from one page.
const PAGE_BYTES: usize = 256 * 1024;
/// The most pages followed; a Messages page holds up to [`PAGE_SIZE`] models.
const MAX_PAGES: usize = 10;
const PAGE_SIZE: &str = "100";

/// `GET /api/v1/admin/routes/{name}/models` (administrator, session or token).
///
/// # Errors
/// `403`, `404` for an unknown route, `409 key_missing`, `409 models_running`, `429 rate_limited` with `Retry-After`,
/// store failure. A refusal by the endpoint is a `200` whose `failure` names it.
pub(crate) async fn models(
    AppState(state): AppState<Arc<app::State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let stored = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        settings::routes::entries(&mut connection)
            .await?
            .into_iter()
            .find(|entry| entry.config.name == name)
            .ok_or(GatewayError::NotFound)
    })
    .await?;
    if settings::routes::key_missing(&stored.config, state.secret_store.as_ref()).await? {
        return Err(GatewayError::Conflict("key_missing"));
    }
    let route = state
        .settings
        .current()
        .registry
        .get(&name)
        .ok_or(GatewayError::NotFound)?
        .clone();
    // Limited per route version, as a probe is: a changed route may be listed again at once.
    let ticket = match state
        .relay
        .listings
        .admit(&name, stored.version, user.id, Instant::now())
    {
        Ok(ticket) => ticket,
        Err(Admission::Running) => return Err(GatewayError::Conflict("models_running")),
        Err(Admission::RateLimited(wait)) => {
            return Err(GatewayError::RateLimited(wait.as_secs().max(1)));
        }
    };
    let reader = crate::secrets::reads::ReadContext {
        user: Some(user.id),
        ..crate::secrets::reads::ReadContext::default()
    };
    let secret = credential(&state, &route, &reader).await?;
    let (collected, failure) =
        match tokio::time::timeout(LIST_TIMEOUT, collect(&state, &route, secret.as_ref())).await {
            Ok(result) => result,
            Err(_) => (Collected::default(), Some(probe::fail("timeout"))),
        };
    drop(ticket);
    tracing::info!(
        route = name.as_str(),
        outcome = failure.as_ref().map_or("ok", |failure| failure.code),
        count = collected.models.len(),
        truncated = collected.truncated,
        "route models listed"
    );
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        settings::audit(
            &mut transaction,
            &settings::Change {
                area: "routes",
                actor: Some(user.id),
                item: &name,
                action: "list_models",
                before: Some(stored.version),
                after: Some(stored.version),
                value_changed: false,
            },
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    let body = serde_json::json!({
        "route": name,
        "protocol": serde_json::to_value(route.protocol)?,
        "models": collected.models,
        "truncated": collected.truncated,
        "failure": failure,
    });
    Ok(Json(body).into_response())
}

/// Every page of the list, up to its caps, or the failure that stopped it.
async fn collect(
    state: &app::State,
    route: &Route,
    secret: Option<&SecretString>,
) -> (Collected, Option<Failure>) {
    let mut collected = Collected::default();
    let mut after: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let body = match fetch(state, route, secret, after.as_deref()).await {
            Ok(body) => body,
            Err(failure) => return (Collected::default(), Some(failure)),
        };
        let Ok(page) = models_list::page(&body) else {
            let start = String::from_utf8_lossy(&body[..body.len().min(512)]).into_owned();
            return (
                Collected::default(),
                Some(Failure {
                    excerpt: Some(probe::bounded(&start, secret)),
                    ..probe::fail("models_unreadable")
                }),
            );
        };
        if !collected.add(page.models) {
            break;
        }
        match page.next {
            Some(next) if route.protocol == Protocol::AnthropicMessages => {
                if collected.full() {
                    collected.truncated = true;
                    break;
                }
                after = Some(next);
            }
            _ => break,
        }
    }
    (collected, None)
}

/// One page's body, at most [`PAGE_BYTES`], or the failure the probe would name.
async fn fetch(
    state: &app::State,
    route: &Route,
    secret: Option<&SecretString>,
    after: Option<&str>,
) -> Result<Vec<u8>, Failure> {
    let mut builder = request::build(&state.relay.client, route, Operation::Models, secret)
        .map_err(|_| probe::fail("configuration"))?;
    if route.protocol == Protocol::AnthropicMessages {
        builder = builder.query(&[("limit", PAGE_SIZE)]);
        if let Some(after) = after {
            builder = builder.query(&[("after_id", after)]);
        }
    }
    let headers = Duration::from_secs(state.config.relay.header_timeout);
    let response = match tokio::time::timeout(headers, builder.send()).await {
        Err(_) => return Err(probe::fail("timeout")),
        Ok(Err(error)) if error.is_timeout() => return Err(probe::fail("timeout")),
        Ok(Err(_)) => return Err(probe::fail("unreachable")),
        Ok(Ok(response)) => response,
    };
    // A listing sends no tools, so a refusal never points at a tool setting: no suggestion, unlike the probe's.
    let response = probe::refuse_status(response, secret)
        .await
        .map_err(|failure| Failure {
            suggestion: None,
            ..failure
        })?;
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| probe::fail("unreachable"))?;
        if body.len() + chunk.len() > PAGE_BYTES {
            return Err(Failure {
                detail: Some(format!("a page is larger than {} KiB", PAGE_BYTES / 1024)),
                ..probe::fail("models_unreadable")
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
