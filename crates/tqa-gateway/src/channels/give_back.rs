//! Giving back a place on a to-do (18.3.0; ADR-0014's amendment of 28 Nov 2026), beside the claim in `api`.

use super::store;
use crate::{GatewayError, app::State, auth::Authz};
use axum::{
    Json,
    extract::{Path, State as AppState},
};
use std::sync::Arc;

/// `POST /api/v1/channels/{id}/tasks/{seq}/give-back`: a person gives back their place on a to-do before reporting on
/// it (18.3.0, GW-I-401): the place is free again and the log says so; the channel's agents are woken as for a new
/// to-do (GW-I-402). People only: an agent finishes or reports.
///
/// # Errors
/// `404`, `403 not_member` or `not_claimant`, `409 channel_ended`, `not_a_task` or `already_reported`, `429
/// channel_rate`, storage failure.
pub(crate) async fn give_back(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((id, seq)): Path<(String, u64)>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let id = super::api::channel_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let seen = store::visible(&mut transaction, super::api::caller(&user), id).await?;
        if seen.role.is_none() {
            return Err(GatewayError::Denied("not_member"));
        }
        let notice = store::give_back(
            &mut transaction,
            id,
            seq,
            (&super::api::person(&user), &user.email),
        )
        .await?;
        transaction.commit().await?;
        state.channels.published(id, notice.seq);
        super::wake::soon(&state, Some(id));
        Ok(Json(serde_json::to_value(notice)?))
    })
    .await
}
