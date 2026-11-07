//! The people's side of agent teams (ADR-0034, the slice S6 contract): a team's state and messages for its person,
//! answering the lead's question, and Stop. Behind the install switch: off, every route answers `teams_disabled`.
//! The routes and their shapes are in `openapi.json`; the handlers are built route by route (slice 6).
use crate::{GatewayError, InvalidReason, app::State, auth::Authz};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tqa_types::ConversationId;

/// Off means no trace: every route answers `403 teams_disabled` while agent teams are off.
pub(crate) fn enabled(state: &State) -> Result<(), GatewayError> {
    if state.config.agent_teams.enabled {
        Ok(())
    } else {
        Err(GatewayError::Denied("teams_disabled"))
    }
}

/// `GET /api/v1/conversations/{id}/team`: the team of the lead's task or a member's, as its person reads it.
///
/// # Errors
/// `403 teams_disabled`; `403` for another person's task, `404 no_team` for one in no team; store failure.
pub(crate) async fn team(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
) -> Result<Response, GatewayError> {
    enabled(&state)?;
    Ok(Json(super::people_read::team(&state, &user, &id).await?).into_response())
}

/// `GET /api/v1/conversations/{id}/team/messages?after=&limit=`: every message of the team after `after` (0 by
/// default), at most `limit` (1 to 200, 100 by default).
///
/// # Errors
/// `403 teams_disabled`; `400` naming a bad parameter; `403` or `404 no_team` as for the team; store failure.
pub(crate) async fn messages(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    RawQuery(query): RawQuery,
) -> Result<Response, GatewayError> {
    enabled(&state)?;
    let page = super::people_rules::page(query.as_deref())?;
    Ok(Json(super::people_read::messages(&state, &user, &id, page).await?).into_response())
}

/// `POST /api/v1/conversations/{id}/team/answers` `{question_seq, text}`: the person answers the lead's question.
///
/// # Errors
/// `403 teams_disabled`; `400` for a malformed body; as [`super::people_write::answer`].
pub(crate) async fn answer(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    enabled(&state)?;
    let answer: super::people_write::Answer =
        serde_json::from_slice(&body).map_err(|_| GatewayError::Invalid {
            field: "body",
            reason: InvalidReason::BodyInvalid,
        })?;
    let seq = super::people_write::answer(&state, &user, &id, (&headers, &answer)).await?;
    Ok((StatusCode::CREATED, Json(seq)).into_response())
}

/// `POST /api/v1/conversations/{id}/team/stop` `{}`: on the lead's task the whole team, on a member's that helper.
///
/// # Errors
/// `403 teams_disabled`; as [`super::people_write::stop`].
pub(crate) async fn stop(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    enabled(&state)?;
    let stopped = super::people_write::stop(&state, &user, &id, &headers).await?;
    Ok((StatusCode::ACCEPTED, Json(stopped)).into_response())
}
