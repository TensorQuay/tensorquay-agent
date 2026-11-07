//! `GET` and `DELETE /mcp/{connector}` (ADR-0031 decision 4): the session's server stream, and the end of a session.
//! Both name the session by its opaque id and pass the same checks as a call.
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::{
    extract::{ConnectInfo, Path, State as AppState},
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
};

use super::{
    Filtering, Held, SESSION_HEADER, STREAM_IDLE, StreamMark, Upstream, admitted, failure, forward,
    outbound_headers, respond, session_of, set_session_header, stream,
};
use crate::{
    ConnectorFailure, GatewayError,
    app::State,
    connectors::{ConnectorDocument, credentials},
};

/// What a `GET` or `DELETE` needs: the caller, the connector, the credential and the session named by the header.
struct SessionCall {
    connector: ConnectorDocument,
    credential: Option<crate::ConnectorCredential>,
    opaque: String,
    remote: String,
}

async fn session_call(
    state: &State,
    headers: &HeaderMap,
    peer: IpAddr,
    name: &str,
) -> Result<SessionCall, GatewayError> {
    let (conversation, owner) = admitted(state, headers, peer, name).await?;
    let connector = state
        .settings
        .current()
        .connectors
        .iter()
        .find(|document| document.name == name && document.enabled)
        .cloned()
        .ok_or(failure(ConnectorFailure::NotAllowed))?;
    if !headers.contains_key(SESSION_HEADER) {
        return Err(GatewayError::BadRequest("Mcp-Session-Id is required"));
    }
    let credential = credentials::resolve(state, owner, name, Some(conversation)).await?;
    if credential.is_none() && connector.auth.required {
        return Err(failure(ConnectorFailure::CredentialMissing));
    }
    let source = credential.as_ref().map(|credential| credential.source);
    let (opaque, remote) =
        session_of(state, headers, (conversation, name), source)?.ok_or(GatewayError::NotFound)?;
    Ok(SessionCall {
        connector,
        credential,
        opaque,
        remote,
    })
}

/// `GET /mcp/{connector}`: the session's server stream (one per session), relayed like an answer and ended after
/// [`STREAM_IDLE`] without an event.
///
/// # Errors
/// As `POST`, plus `400` without a session, `404` for an unusable one, `409 stream_open` for a second stream.
pub(crate) async fn listen(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    let call = session_call(&state, &headers, peer.ip(), &name).await?;
    if !state
        .mcp
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .start_stream(&call.opaque)
    {
        return Err(GatewayError::Conflict("stream_open"));
    }
    let mark = StreamMark {
        sessions: Arc::clone(&state.mcp.sessions),
        opaque: call.opaque.clone(),
    };
    let mut outbound = outbound_headers(&HeaderMap::new(), Some(&call.remote));
    outbound.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("text/event-stream"),
    );
    let mut response = match forward(
        &state,
        &call.connector,
        (reqwest::Method::GET, &outbound, None),
        call.credential,
    )
    .await?
    {
        Upstream::Stream(status, answer_headers, upstream) => stream(
            (status, &answer_headers, upstream),
            Filtering {
                list_ids: Vec::new(),
                filter: call.connector.tools.clone(),
            },
            Held {
                _permits: None,
                _stream: Some(mark),
                cancel: None,
                idle: Some(STREAM_IDLE),
            },
        ),
        // A server without a stream answers 405; nothing to hold open.
        Upstream::Json(status, answer_headers, answer) => {
            respond(status, &answer_headers, answer.as_ref())
        }
    };
    set_session_header(&mut response, Some(&call.opaque));
    Ok(response)
}

/// `DELETE /mcp/{connector}`: ends the session at the remote and forgets it.
///
/// # Errors
/// As `POST`, plus `400` without a session and `404` for an unusable one.
pub(crate) async fn close(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    let call = session_call(&state, &headers, peer.ip(), &name).await?;
    let (conversation, _) = admitted(&state, &headers, peer.ip(), &name).await?;
    state
        .mcp
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .close(&call.opaque, (conversation, &name));
    let outbound = outbound_headers(&HeaderMap::new(), Some(&call.remote));
    let answer = forward(
        &state,
        &call.connector,
        (reqwest::Method::DELETE, &outbound, None),
        call.credential,
    )
    .await?;
    tracing::info!(connector = %name, "connector session closed");
    let status = match answer {
        Upstream::Json(status, ..) | Upstream::Stream(status, ..) => status,
    };
    Ok(status.into_response())
}
