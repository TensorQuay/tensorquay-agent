//! The decision engine relay (ADR-0013 points 5 and 6): `POST /relay/d/<engine>/v1/{before_step,after_response}`.
//! A runtime calls it with its token; the gateway checks that the conversation's profile names this engine, that the
//! request carries nothing the engine's history exposure excludes (a tool in the sandbox could read the token and call
//! here directly), counts it against the owner's relay rate, records a usage row `decision:<name>`, adds the
//! engine's credential from the secret store and forwards within the engine's deadline. Bodies are bounded both ways
//! and only the content type comes back.
use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use futures::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tqa_types::{DecisionExposure, DecisionPoint, decision_v1};

use crate::{
    GatewayError, app,
    connectors::Scheme,
    decision_engines::{EngineDocument, EngineProtocol},
    secrets::reads::{ConsumerKind, ReadContext},
};

/// One consultation, relayed.
///
/// # Errors
/// `401` without a live runtime token, `403` for an engine the conversation's profile doesn't name, `404` for an
/// unknown or disabled engine or point, `413` for a body over 64 KiB, `400` for a request that isn't `decision/v1` or
/// carries what the exposure excludes, `429` past the owner's relay rate, `503` without the engine's credential, `502`
/// for a failed or oversized answer and `504` past the engine's deadline.
pub(crate) async fn decision(
    State(state): State<Arc<app::State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path((engine, point)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let point = match point.as_str() {
        "before_step" => DecisionPoint::BeforeStep,
        "after_response" => DecisionPoint::AfterResponse,
        _ => return Err(GatewayError::NotFound),
    };
    let token = crate::auth::token::bearer(&headers)?;
    let (conversation, _) = state
        .runtime_tokens
        .resolve(&token, peer.ip())
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    let id = crate::conversations::uuid(&conversation)?;
    let record = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "SELECT owner_id,profile->>'decision_engine' AS engine FROM conversations WHERE id=$1 AND state='active'",
            id
        )
        .fetch_optional(&state.pool)
        .await?
        .ok_or(GatewayError::Unauthorized)
    })
    .await?;
    // Only the engine the conversation's profile names, as the model relay allows only the conversation's routes.
    if record.engine.as_deref() != Some(engine.as_str()) {
        return Err(GatewayError::Forbidden(
            "runtime decision engine is not authorized",
        ));
    }
    let document = enabled_engine(&state, &engine, point)?;
    if body.len() > decision_v1::MAX_REQUEST_BYTES {
        return Err(GatewayError::BodyTooLarge);
    }
    let request: decision_v1::Request = serde_json::from_slice(&body)
        .map_err(|_| GatewayError::BadRequest("not a decision/v1 request"))?;
    if request.point != point {
        return Err(GatewayError::BadRequest(
            "the request's point differs from its path",
        ));
    }
    if exceeds(&request, document.exposure) {
        return Err(GatewayError::BadRequest(
            "the request carries history the engine's exposure excludes",
        ));
    }
    state.relay.rate.admit(
        record.owner_id,
        std::time::Instant::now(),
        state.settings.current().limits.per_user,
    )?;
    let consumer = format!("decision:{engine}");
    // The request's run, kept only when it is this task's (the run-id contract): the body is the runtime's word.
    let run = super::attribution::attributed(
        &state,
        &conversation,
        Some(request.run_id.clone()),
        &consumer,
    )
    .await?;
    let context = ReadContext {
        conversation: Some(id),
        user: Some(record.owner_id),
        run: run.as_ref().map(|run| run.as_str().to_owned()),
    };
    // A typed question is built, and refused when too large, before anything else: a question never sent costs nothing,
    // so it reads no key and isn't counted against the budget.
    let question = match document.protocol {
        EngineProtocol::TypedQuestionsV1 => Some(super::typed::question(&document, &request)?),
        EngineProtocol::DecisionV1 => None,
    };
    // An agent team's budget, then the engine's, before its key is read or any call leaves the gateway (ADR-0034
    // point 8; ADR-0013, the adapter amendment).
    crate::teams::budget::check(&state, &conversation).await?;
    super::budget::check(&state, &document, &conversation).await?;
    let credential = credential(&state, &document, &context).await?;
    // A usage row per consultation about to be sent, recorded before forwarding, so it counts requests, failed ones
    // included; that is what the engine's budget counts.
    let mut meter =
        super::meter::Meter::start(&state, &conversation, &consumer, run.as_ref()).await?;
    match question {
        None => forward(&state, &document, point_path(point), credential, body).await,
        Some(question) => {
            super::typed::ask(&state, &document, question, credential, &mut meter).await
        }
    }
}

/// The engine's path for `point`.
const fn point_path(point: DecisionPoint) -> &'static str {
    match point {
        DecisionPoint::BeforeStep => "before_step",
        _ => "after_response",
    }
}

/// The enabled engine called `engine`, if it answers `point`: a point its kind doesn't answer is not a route here, as
/// for an unknown point.
fn enabled_engine(
    state: &app::State,
    engine: &str,
    point: DecisionPoint,
) -> Result<EngineDocument, GatewayError> {
    let document = state
        .settings
        .current()
        .decision_engines
        .iter()
        .find(|stored| stored.name == engine && stored.enabled)
        .cloned()
        .ok_or(GatewayError::NotFound)?;
    if document
        .protocol
        .points()
        .is_none_or(|points| points.contains(&point))
    {
        Ok(document)
    } else {
        Err(GatewayError::NotFound)
    }
}

/// Whether the request carries anything the exposure excludes: arguments under `none`, and the response's text or the
/// transcript under anything short of `full`.
fn exceeds(request: &decision_v1::Request, exposure: DecisionExposure) -> bool {
    let arguments = request
        .previous_calls
        .iter()
        .any(|call| call.arguments.is_some())
        || request
            .response_calls
            .iter()
            .any(|call| call.arguments.is_some());
    let history = request.response_text.is_some() || request.transcript.is_some();
    (exposure < DecisionExposure::Calls && arguments)
        || (exposure < DecisionExposure::Full && history)
}

/// The engine's credential from the secret store, read as the engine's consumer and audited; `None` without auth.
async fn credential(
    state: &app::State,
    document: &EngineDocument,
    context: &ReadContext,
) -> Result<Option<SecretString>, GatewayError> {
    if document.auth.scheme == Scheme::None {
        return Ok(None);
    }
    let reference = document
        .key_ref
        .as_ref()
        .ok_or(GatewayError::SecretUnavailable)?
        .parse()
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let value = crate::secrets::reads::read(
        state,
        &reference,
        (ConsumerKind::Engine, &document.name),
        context,
    )
    .await?
    .ok_or(GatewayError::SecretUnavailable)?;
    if value.expose_secret().is_empty() {
        return Err(GatewayError::SecretUnavailable);
    }
    Ok(Some(value))
}

/// Posts the body to `<url>/v1/<path>` within the engine's deadline and returns its answer, bounded, with only its
/// content type.
async fn forward(
    state: &Arc<app::State>,
    document: &EngineDocument,
    path: &str,
    credential: Option<SecretString>,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let client = client(state, document).await?;
    let url = format!("{}/v1/{path}", document.url.trim_end_matches('/'));
    let mut request = client
        .post(url)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body);
    if let Some(credential) = &credential {
        request = match (document.auth.scheme, document.auth.header.as_deref()) {
            (Scheme::Header, Some(name)) => request.header(name, credential.expose_secret()),
            _ => request.bearer_auth(credential.expose_secret()),
        };
    }
    let deadline = Duration::from_millis(u64::from(document.timeout_ms));
    let exchange = async {
        let upstream = request
            .send()
            .await
            .map_err(|_| GatewayError::UpstreamUnavailable)?;
        if !upstream.status().is_success() {
            return Err(GatewayError::UpstreamUnavailable);
        }
        let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
        let mut answer = Vec::new();
        let mut stream = upstream.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| GatewayError::UpstreamUnavailable)?;
            if answer.len() + chunk.len() > decision_v1::MAX_RESPONSE_BYTES {
                return Err(GatewayError::UpstreamUnavailable);
            }
            answer.extend_from_slice(&chunk);
        }
        Ok((content_type, answer))
    };
    let (content_type, answer) = tokio::select! {
        () = state.cancel.cancelled() => return Err(GatewayError::Cancelled),
        outcome = tokio::time::timeout(deadline, exchange) => outcome.map_err(|_| GatewayError::Timeout)??,
    };
    let mut response = Response::new(Body::from(answer));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        content_type.unwrap_or_else(|| HeaderValue::from_static("application/json")),
    );
    Ok(response)
}

/// The relay's shared client, or one of the engine's own for a private certificate authority or plain `http` (both
/// file-only); engines are administrator-configured, so there is no address pinning, as for routes.
pub(super) async fn client(
    state: &Arc<app::State>,
    document: &EngineDocument,
) -> Result<reqwest::Client, GatewayError> {
    if document.ca_file.is_none() && !document.insecure_http {
        return Ok(state.relay.client.clone());
    }
    let mut builder = reqwest::Client::builder()
        .https_only(!document.insecure_http)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy();
    if let Some(path) = &document.ca_file {
        let pem = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            tokio::fs::read(path)
                .await
                .map_err(|_| GatewayError::UpstreamUnavailable)
        })
        .await?;
        for certificate in reqwest::Certificate::from_pem_bundle(&pem)
            .map_err(|_| GatewayError::UpstreamUnavailable)?
        {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder
        .build()
        .map_err(|_| GatewayError::UpstreamUnavailable)
}
