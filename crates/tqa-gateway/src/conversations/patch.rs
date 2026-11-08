//! `PATCH /conversations/{id}`: rename, pin, or pick the route or the MCP servers for the next run (ARCHITECTURE §9).
use super::{Conversation, Record, idempotent::Keyed, service};
use crate::{GatewayError, app::State, auth::User};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tqa_sandbox::ReadinessSource;
use tqa_types::{ClientFrame, ConversationId};

/// The changeable conversation fields; at least one is required.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Patch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pinned: Option<bool>,
    /// The catalogue servers for the next run (ADR-0011); refused while a run is active, like a route change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mcp_servers: Option<Vec<String>>,
    /// The connectors for the next run (ADR-0031); refused while a run is active, like a route change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connectors: Option<Vec<String>>,
}

pub(super) async fn apply(
    state: &State,
    user: &User,
    id: &ConversationId,
    headers: &HeaderMap,
    input: &Patch,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    if input.title.is_none()
        && input.route.is_none()
        && input.pinned.is_none()
        && input.mcp_servers.is_none()
        && input.connectors.is_none()
    {
        return Err(GatewayError::BadRequest(
            "a title, a route, a pin, mcp_servers or connectors is required",
        ));
    }
    if let Some(servers) = input.mcp_servers.as_deref() {
        service::allowed_mcp_servers(state, servers)?;
    }
    if let Some(connectors) = input.connectors.as_deref() {
        service::allowed_connectors(state, connectors)?;
    }
    let title = input
        .title
        .as_deref()
        .map(service::valid_title)
        .transpose()?;
    if let Some(route) = input.route.as_deref() {
        service::allowed_route(state, route)?;
    }
    service::authorize(state, user, id, false).await?;
    let keyed = Keyed::new(
        user.id,
        headers,
        format!("/api/v1/conversations/{id}"),
        input,
    )?;
    let (status, body, changed) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some((status, body)) = keyed.begin(state, &mut transaction).await? {
            return Ok((status, body, false));
        }
        let uuid = super::uuid(id)?;
        // The row lock orders this change with event appends, so a run cannot start between the check and the update.
        let current = sqlx::query!("SELECT state,profile FROM conversations WHERE id=$1 FOR UPDATE", uuid)
            .fetch_one(&mut *transaction)
            .await?;
        if current.state != "active" {
            return Err(GatewayError::Conflict("conversation_deleted"));
        }
        if let Some(copy) = current.profile {
            within_profile(input, &serde_json::from_value(copy)?)?;
        }
        if (input.route.is_some() || input.mcp_servers.is_some() || input.connectors.is_some())
            && crate::events::run_activity(&mut *transaction, uuid)
                .await?
                .unfinished()
        {
            return Err(GatewayError::Conflict("run_active"));
        }
        let record = sqlx::query_as!(Record,
            "UPDATE conversations SET title=COALESCE($2,title),title_source=CASE WHEN $2::text IS NULL THEN title_source ELSE 'person' END,model_route=COALESCE($3,model_route),pinned=COALESCE($4,pinned),mcp_servers=COALESCE($5,mcp_servers),connectors=COALESCE($6,connectors),updated_at=CURRENT_TIMESTAMP WHERE id=$1 RETURNING id,owner_id,title,title_source,model_route,backend,state,template,pinned,created_at,updated_at,last_activity_at,workspace_id,mcp_servers,connectors,profile_id,profile_version,profile->>'name' AS profile_name,profile->>'display_name' AS profile_display_name,(profile->>'model_route') IS NOT NULL AS \"profile_sets_route?\",(profile->>'template') IS NOT NULL AS \"profile_sets_template?\"",
            uuid, title, input.route.as_deref(), input.pinned, input.mcp_servers.as_deref(), input.connectors.as_deref()).fetch_one(&mut *transaction).await?;
        // The servers and connectors share the runtime's bound (and one name space, which the file and the API keep apart).
        if record.mcp_servers.len() + record.connectors.len() > crate::config::MAX_MCP_SERVERS {
            return Err(GatewayError::BadRequest(
                "mcp_servers and connectors together name at most 8",
            ));
        }
        let body = serde_json::to_value(Conversation::try_from(record)?)?;
        keyed
            .record_about(&mut transaction, uuid, StatusCode::OK, &body)
            .await?;
        transaction.commit().await?;
        Ok((StatusCode::OK, body, input.route.is_some()))
    })
    .await?;
    if let (true, Some(route)) = (changed, input.route.as_deref()) {
        update_link(state, id, route).await;
    }
    Ok((status, Json(body)))
}

/// A conversation made from a profile only narrows its servers and connectors, and keeps the route it sets
/// (ADR-0012 decision 4).
///
/// # Errors
/// `422 set_by_profile` for a server or connector outside the profile's set, or another route than the profile's.
fn within_profile(input: &Patch, profile: &crate::profiles::Profile) -> Result<(), GatewayError> {
    let within = |asked: Option<&Vec<String>>, set: &[String]| {
        asked.is_none_or(|asked| asked.iter().all(|name| set.contains(name)))
    };
    if !within(input.mcp_servers.as_ref(), &profile.mcp_servers) {
        return Err(GatewayError::SetByProfile {
            field: "mcp_servers",
        });
    }
    if !within(input.connectors.as_ref(), &profile.connectors) {
        return Err(GatewayError::SetByProfile {
            field: "connectors",
        });
    }
    if let (Some(asked), Some(set)) = (input.route.as_deref(), profile.model_route.as_deref())
        && asked != set
    {
        return Err(GatewayError::SetByProfile { field: "route" });
    }
    Ok(())
}

/// Tells the live runtime of every active task whose route is `name`, or falls back to it, its route as it now is
/// (GW-I-157): a route an administrator changes applies from each task's next message, not its environment's next
/// start. Called after the change is committed and the settings refreshed; never fails the administrator's write.
pub(crate) async fn route_changed(state: &State, name: &str) {
    let settings = state.settings.current();
    let routes: Vec<String> = settings
        .registry
        .names()
        .filter(|route| {
            *route == name
                || settings
                    .registry
                    .get(route)
                    .is_some_and(|entry| entry.fallbacks.iter().any(|fallback| fallback == name))
        })
        .map(str::to_owned)
        .collect();
    let tasks = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT id::text AS \"id!\",model_route FROM conversations WHERE state='active' AND model_route = ANY($1)",
            &routes
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await;
    let Ok(tasks) = tasks else {
        tracing::info!(
            route = name,
            "a changed route reaches open tasks at their next registration"
        );
        return;
    };
    for task in tasks {
        if let Ok(id) = task.id.parse::<ConversationId>() {
            update_link(state, &id, &task.model_route).await;
        }
    }
}

/// Tells a live runtime about the new route. The stored route is authoritative: a runtime that misses this frame
/// receives the same route in the `session.config` of its next registration, and the relay accepts only it.
async fn update_link(state: &State, id: &ConversationId, route: &str) {
    if !state.links.is_ready(id).await {
        return;
    }
    let result = async {
        // One snapshot for the route and its fallbacks (ADR-0028).
        let settings = state.settings.current();
        let frame = ClientFrame::SessionUpdate {
            route: crate::link::route_snapshot(state, &settings.registry, route)?,
            fallback_routes: crate::link::fallback_snapshots(state, &settings.registry, route)?,
        };
        crate::io::bounded(
            state.config.io_timeout(),
            &state.cancel,
            state.links.deliver(id, frame),
        )
        .await
    }
    .await;
    if let Err(error) = result {
        tracing::info!(conversation_id = %id, code = error.code(), "route update reaches the runtime at its next registration");
    }
}
