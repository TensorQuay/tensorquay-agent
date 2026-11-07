//! Authenticated history paging and a bounded streaming hop to the existing model fake.
use super::{GatewayRequest, MAX_FRAMES, Shared, seq};
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::{Arc, PoisonError};
use tqa_types::Seq;

/// How long the fake gateway's relay waits for a body or an upstream answer before answering 413 or 502: its own
/// behaviour, which a test may observe, so it is not scaled with the harness's liveness waits.
const RELAY_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

pub(super) fn authorize(shared: &Shared, headers: &HeaderMap, path: &str) -> bool {
    let authenticated =
        headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(shared.bearer.as_str());
    let mut data = shared.data.lock().unwrap_or_else(PoisonError::into_inner);
    if data.requests.len() >= MAX_FRAMES {
        return false;
    }
    let protocol_revision = headers
        .get(tqa_types::PROTOCOL_REVISION_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= 16)
        .map(str::to_owned);
    data.requests.push(GatewayRequest {
        path: path.into(),
        authenticated,
        protocol_revision,
    });
    data.bound && authenticated
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    #[serde(default)]
    after: u64,
    limit: Option<usize>,
}
pub(super) async fn history(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Response {
    if !authorize(&shared, &headers, "/runtime/history") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let limit = page.limit.unwrap_or(100);
    if !(1..=100).contains(&limit) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let items = shared
        .data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .stored
        .iter()
        .filter(|frame| seq(frame).is_some_and(|s| s > Seq::from(page.after)))
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let next_after = items.last().and_then(seq);
    Json(serde_json::json!({"items":items,"next_after":next_after})).into_response()
}
/// `GET /runtime/teams/role`: the scripted role, 404 when the fake has none (an older gateway), or no answer at all
/// while it hangs.
pub(super) async fn team_role(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if !authorize(&shared, &headers, "/runtime/teams/role") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let route = shared
        .data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .team_role
        .clone();
    let role = match route {
        super::RoleRoute::Hangs => std::future::pending().await,
        super::RoleRoute::Role(role) => Some(role),
        super::RoleRoute::Absent => None,
    };
    match role {
        Some(role) => Json(serde_json::json!({"role": role})).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":{"code":"not_found","message":"no such route"}})),
        )
            .into_response(),
    }
}

pub(super) async fn channels(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if !authorize(&shared, &headers, "/runtime/channels") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let channels = shared
        .data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .channels
        .clone();
    match channels {
        Some(channels) => Json(serde_json::json!({"channels": channels})).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":{"code":"not_found","message":"no such route"}})),
        )
            .into_response(),
    }
}
pub(super) async fn relay(State(shared): State<Arc<Shared>>, request: Request<Body>) -> Response {
    if !authorize(&shared, request.headers(), request.uri().path()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // Like the real relay, the operation path is kept; only the route prefix is replaced.
    let path = request.uri().path();
    let operation = if path.ends_with("/responses") {
        "responses"
    } else if path.ends_with("/messages") {
        "messages"
    } else {
        "chat/completions"
    };
    let body = tokio::select! {biased;
        ()=shared.cancel.cancelled()=>return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        result=tokio::time::timeout(RELAY_DEADLINE,to_bytes(request.into_body(),2*1024*1024))=>match result {Ok(Ok(body))=>body,_=>return StatusCode::PAYLOAD_TOO_LARGE.into_response()}
    };
    let request = shared
        .client
        .post(format!("{}/{operation}", shared.model_origin))
        .header("content-type", "application/json")
        .body(body);
    let response = tokio::select! {biased;
        ()=shared.cancel.cancelled()=>return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        result=tokio::time::timeout(RELAY_DEADLINE,request.send())=>match result {Ok(Ok(response))=>response,_=>return StatusCode::BAD_GATEWAY.into_response()}
    };
    let status = response.status();
    // Like the real transparent relay, keep the headers a client needs to classify the reply.
    let kept: Vec<_> = ["content-type", "retry-after"]
        .into_iter()
        .filter_map(|name| Some((name, response.headers().get(name)?.clone())))
        .collect();
    let mut result = Response::new(Body::from_stream(response.bytes_stream()));
    *result.status_mut() = status;
    for (name, value) in kept {
        result.headers_mut().insert(name, value);
    }
    result
}
/// `POST /relay/d/{engine}/v1/{point}`: the next scripted decision, standing in for the gateway's engine relay; 503 once
/// the script is spent. The request is recorded by path like every other.
pub(super) async fn decision(
    State(shared): State<Arc<Shared>>,
    Path((engine, point)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !authorize(&shared, &headers, &format!("/relay/d/{engine}/v1/{point}")) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let next = shared
        .data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .decisions
        .pop_front();
    next.map_or_else(
        || StatusCode::SERVICE_UNAVAILABLE.into_response(),
        |decision| Json(decision).into_response(),
    )
}
