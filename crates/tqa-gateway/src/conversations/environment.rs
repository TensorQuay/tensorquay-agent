//! `PUT /conversations/{id}/environment`: choose the template for the next run (ARCHITECTURE §4.7).
use super::{Conversation, Record, idempotent::Keyed, service};
use crate::{GatewayError, app::State, auth::User};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tqa_types::ConversationId;

/// The requested template.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChangeEnvironment {
    template: String,
}

pub(super) async fn switch(
    state: &State,
    user: &User,
    id: &ConversationId,
    headers: &HeaderMap,
    input: &ChangeEnvironment,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    let snapshot = state.settings.current();
    let template = crate::environments::choose(&snapshot, Some(&input.template))?;
    let digest = template.environment().digest.clone();
    service::authorize(state, user, id, false).await?;
    let keyed = Keyed::new(
        user.id,
        headers,
        format!("/api/v1/conversations/{id}/environment"),
        input,
    )?;
    let uuid = super::uuid(id)?;
    let (body, switched) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some((_, body)) = keyed.begin(state, &mut transaction).await? {
            return Ok((body, false));
        }
        // The row lock orders the switch with event appends and accepted actions.
        let row = sqlx::query!("SELECT state,template,profile->>'template' AS by_profile FROM conversations WHERE id=$1 FOR UPDATE", uuid)
            .fetch_one(&mut *transaction)
            .await?;
        if row.state != "active" {
            return Err(GatewayError::Conflict("conversation_deleted"));
        }
        // A profile that sets a template keeps it (ADR-0012 decision 7).
        crate::profiles::set("template", Some(&input.template), row.by_profile.as_deref())?;
        let switched = row.template != input.template;
        if switched && busy(&mut transaction, uuid).await? {
            return Err(GatewayError::Conflict("run_active"));
        }
        let record = sqlx::query_as!(Record,
            "UPDATE conversations SET template=$2,template_digest=CASE WHEN template=$2 THEN template_digest ELSE $3 END,template_version=CASE WHEN template=$2 THEN template_version ELSE NULL END,updated_at=CASE WHEN template=$2 THEN updated_at ELSE CURRENT_TIMESTAMP END WHERE id=$1 RETURNING id,owner_id,title,title_source,model_route,backend,state,template,pinned,created_at,updated_at,last_activity_at,workspace_id,mcp_servers,connectors,profile_id,profile_version,profile->>'name' AS profile_name,profile->>'display_name' AS profile_display_name,(profile->>'model_route') IS NOT NULL AS \"profile_sets_route?\",(profile->>'template') IS NOT NULL AS \"profile_sets_template?\"",
            uuid, input.template, digest).fetch_one(&mut *transaction).await?;
        let body = serde_json::to_value(Conversation::try_from(record)?)?;
        keyed
            .record_about(&mut transaction, uuid, StatusCode::OK, &body)
            .await?;
        transaction.commit().await?;
        Ok((body, switched))
    })
    .await?;
    if switched {
        // Files and history stay; the running sandbox retires and the next action starts the new template.
        state
            .manager
            .retire_for_switch(id, crate::manager::Teardown::EnvironmentSwitch)?;
    }
    Ok((StatusCode::OK, Json(body)))
}

/// A run is active, or an accepted message is still waiting for its sandbox.
async fn busy(connection: &mut sqlx::PgConnection, id: uuid::Uuid) -> Result<bool, GatewayError> {
    if crate::events::run_activity(&mut *connection, id)
        .await?
        .unfinished()
    {
        return Ok(true);
    }
    let queued = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM idempotency WHERE conversation_id=$1 AND execution_status='queued')",
        id
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(queued.unwrap_or(true))
}
