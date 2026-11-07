use super::{Action, store::dispatch};
use crate::{GatewayError, app, auth::Authz, request::ApiJson};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
};
use serde::Deserialize;
use std::sync::Arc;
use tqa_types::{ClientFrame, ConversationId, RunId};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Message {
    text: String,
    #[serde(default)]
    attachments: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Empty {}

pub(crate) async fn message(
    State(state): State<Arc<app::State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Message>,
) -> Result<Response, GatewayError> {
    let route = format!("/api/v1/conversations/{id}/messages");
    let action = Action::new(
        &headers,
        route,
        ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: None,
            text: body.text,
            attachments: body.attachments,
            host_keys: Vec::new(),
        },
    )?;
    let mut copied = None;
    let response =
        super::store::dispatch_copying(&state, &user, &id, (action, Some(&mut copied))).await;
    // The lead reads the copy with its tools, or is woken for it (ADR-0034, the S6 contract).
    if let Some((team, channel, seq)) = copied {
        state.channels.published(channel, seq);
        crate::teams::wake::soon(&state, team);
    }
    response
}

pub(crate) async fn cancel(
    State(state): State<Arc<app::State>>,
    Authz(user): Authz,
    Path((id, run_id)): Path<(ConversationId, RunId)>,
    headers: HeaderMap,
    ApiJson(_body): ApiJson<Empty>,
) -> Result<Response, GatewayError> {
    let route = format!("/api/v1/conversations/{id}/runs/{run_id}/cancel");
    let action = Action::new(&headers, route, ClientFrame::RunCancel { run_id })?;
    dispatch(&state, &user, &id, action).await
}
