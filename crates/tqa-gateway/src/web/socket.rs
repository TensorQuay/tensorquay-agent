//! Development-only hot reload; both directions are bounded and owned by gateway shutdown.

use crate::{GatewayError, app::State};
use axum::{
    extract::{
        FromRequestParts, Request, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message as UpstreamMessage, client::IntoClientRequest},
};

pub(super) async fn upgrade(state: Arc<State>, request: Request) -> Response {
    let Ok(permit) = state.web.sockets.clone().try_acquire_owned() else {
        return GatewayError::Busy.into_response();
    };
    let (mut parts, _) = request.into_parts();
    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
        Ok(upgrade) => upgrade,
        Err(error) => return error.into_response(),
    };
    let upstream = tokio::select! {
        () = state.cancel.cancelled() => return GatewayError::Cancelled.into_response(),
        result = connect(&state, &parts) => match result {
            Ok(socket) => socket,
            Err(_) => return super::unavailable(),
        }
    };
    let tasks = state.tasks.clone();
    let cancel = state.cancel.clone();
    upgrade
        .max_message_size(1024 * 1024)
        .on_upgrade(move |socket| async move {
            tasks.spawn(async move {
            let _permit = permit;
            tokio::select! {
                () = cancel.cancelled() => {},
                _ = tokio::time::timeout(Duration::from_hours(1), relay(socket, upstream)) => {},
            }
        }).await.ok();
        })
}

async fn connect(
    state: &State,
    parts: &axum::http::request::Parts,
) -> Result<WebSocketStream<TcpStream>, GatewayError> {
    let config = state.config.web.as_ref().ok_or(GatewayError::NotFound)?;
    let host = config.upstream.host_str().ok_or(GatewayError::NotFound)?;
    let port = config
        .upstream
        .port_or_known_default()
        .ok_or(GatewayError::NotFound)?;
    let connection =
        tokio::time::timeout(super::CONNECT_TIMEOUT, crate::net::connect((host, port)))
            .await
            .map_err(|_| GatewayError::Timeout)??;
    let mut url = config.upstream.clone();
    url.set_scheme("ws").map_err(|()| GatewayError::Io)?;
    url.set_path(super::HMR_PATH);
    url.set_query(parts.uri.query());
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| GatewayError::Io)?;
    for (name, value) in
        &super::headers::request(parts.headers.clone(), &state.config.public_origin)
    {
        if !name.as_str().starts_with("sec-websocket-") {
            request.headers_mut().insert(name, value.clone());
        }
    }
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(1024 * 1024));
    tokio::time::timeout(
        super::HEAD_TIMEOUT,
        tokio_tungstenite::client_async_with_config(request, connection, Some(config)),
    )
    .await
    .map_err(|_| GatewayError::Timeout)?
    .map(|(socket, _)| socket)
    .map_err(|_| GatewayError::Io)
}

async fn relay(mut browser: WebSocket, mut upstream: WebSocketStream<TcpStream>) {
    loop {
        tokio::select! {
            message = browser.recv() => {
                let Some(Ok(message)) = message else { break; };
                let message = match message {
                    Message::Text(value) => UpstreamMessage::Text(value.as_str().into()),
                    Message::Binary(value) => UpstreamMessage::Binary(value),
                    Message::Ping(value) => UpstreamMessage::Ping(value),
                    Message::Pong(value) => UpstreamMessage::Pong(value),
                    Message::Close(_) => break,
                };
                if !matches!(tokio::time::timeout(super::HEAD_TIMEOUT, upstream.send(message)).await, Ok(Ok(()))) { break; }
            },
            message = upstream.next() => {
                let Some(Ok(message)) = message else { break; };
                let message = match message {
                    UpstreamMessage::Text(value) => Message::Text(value.as_str().into()),
                    UpstreamMessage::Binary(value) => Message::Binary(value),
                    UpstreamMessage::Ping(value) => Message::Ping(value),
                    UpstreamMessage::Pong(value) => Message::Pong(value),
                    UpstreamMessage::Close(_) => break,
                    UpstreamMessage::Frame(_) => continue,
                };
                if !matches!(tokio::time::timeout(super::HEAD_TIMEOUT, browser.send(message)).await, Ok(Ok(()))) { break; }
            },
        }
    }
}
