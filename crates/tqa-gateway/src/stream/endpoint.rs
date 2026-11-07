use super::{EventQuery, encode, session::Session};
use crate::{
    GatewayError, app,
    auth::{Authz, check_origin},
    conversations,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Response, Sse, sse::KeepAlive},
};
use std::sync::Arc;
use tqa_types::{ConversationId, ServerFrame};

pub(crate) async fn events(
    State(state): State<Arc<app::State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    let conversation = conversations::authorize(&state, &user, &id, false).await?;
    if !query.is_stream()? {
        let (after, limit) = query.page(state.config.limits.page_size)?;
        let frames = state.events.read(&id, after, limit).await?;
        let next_after = frames.last().and_then(crate::wire::sequence);
        // Clients never receive opaque reasoning items; only the runtime's history carries them.
        let frames: Vec<ServerFrame> = frames.into_iter().map(ServerFrame::for_clients).collect();
        return Ok(
            Json(serde_json::json!({"items":frames, "next_after":next_after})).into_response(),
        );
    }
    check_origin(&headers, &state.config.public_origin, false)?;
    let after = query.stream_start(&headers)?;
    let (ready, failed) =
        super::snapshot::ready(&state, &id, conversation.backend, after != 0.into()).await?;
    let mut opening = vec![ready];
    if let Some(code) = failed {
        opening.push(super::snapshot::failed_action_notice(code));
    }
    let interval = state.config.stream_keepalive;
    state
        .events
        .open_gate()
        .pass(crate::events::OpenPoint::Checked)
        .await;
    let session = Session::open(state, id, after, opening, conversation.backend).await?;
    let viewer = user.id;
    let stream = futures::stream::unfold(session, move |mut session| async move {
        // A keyed call's notice names its key only to the run's sender (ADR-0037, Amendment 1).
        let frame = crate::host_keys::for_viewer(session.next().await?.for_clients(), viewer);
        Some((encode(&frame), session))
    });
    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(interval).text("keep-alive"))
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}
