use crate::{GatewayError, app, stream::EventQuery};
use axum::{
    Json,
    extract::{ConnectInfo, Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Page {
    after: Option<u64>,
    limit: Option<u32>,
}

pub(crate) async fn history(
    State(state): State<Arc<app::State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(page): Query<Page>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let token = crate::auth::token::bearer(&headers)?;
    let (conversation, _) = state
        .runtime_tokens
        .resolve(&token, peer.ip())
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    let query = EventQuery {
        after: page.after,
        limit: page.limit,
        stream: None,
        protocol: None,
    };
    let (after, limit) = query.page(state.config.limits.page_size)?;
    let frames = state.events.read(&conversation, after, limit).await?;
    let next_after = frames.last().and_then(crate::wire::sequence);
    Ok(Json(
        serde_json::json!({"items":frames,"next_after":next_after}),
    ))
}
