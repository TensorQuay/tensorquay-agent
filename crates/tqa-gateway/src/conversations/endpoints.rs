use super::{Conversation, Record, service};
use crate::{
    GatewayError,
    app::State,
    auth::{Authz, Role},
    request::ApiJson,
};
use axum::{
    Json,
    extract::{Path, Query, State as AppState},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tqa_types::ConversationId;

#[path = "create.rs"]
mod creation;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Create {
    title: String,
    /// Required unless the profile sets one; absent keeps the fingerprint of clients that predate profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model_route: Option<String>,
    /// The profile to make the conversation from (ADR-0012 decision 5): the caller's own or a shared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<uuid::Uuid>,
    // Absent keeps the request fingerprint of clients that predate environments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template: Option<String>,
    /// A workspace the caller owns or holds a write share on; absent creates a new empty one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) workspace_id: Option<uuid::Uuid>,
}

pub(crate) async fn create(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    headers: axum::http::HeaderMap,
    ApiJson(input): ApiJson<Create>,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    creation::create(&state, user.id, &headers, &input).await
}

pub(crate) async fn patch(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    headers: axum::http::HeaderMap,
    ApiJson(input): ApiJson<super::patch::Patch>,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    super::patch::apply(&state, &user, &id, &headers, &input).await
}

pub(crate) async fn environment(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    headers: axum::http::HeaderMap,
    ApiJson(input): ApiJson<super::environment::ChangeEnvironment>,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    super::environment::switch(&state, &user, &id, &headers, &input).await
}

/// `GET /conversations/{id}`: the task as the list shows it, its run status included, so a page opened on it knows
/// whether it ran before its history replays (GW-I-120).
pub(crate) async fn get(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
) -> Result<Json<Listed>, GatewayError> {
    let mut conversation = [service::authorize(&state, &user, &id, true)
        .await?
        .for_viewer(user.id)];
    crate::teams::place::attach(&state, user.id, &mut conversation).await?;
    let run_status = run_statuses(&state, &conversation)
        .await?
        .remove(&super::uuid(&id)?)
        .unwrap_or(crate::channels::store::RunStatus::Idle);
    let [conversation] = conversation;
    Ok(Json(Listed {
        conversation,
        run_status,
    }))
}

/// `DELETE /conversations/{id}?with_helpers=`: a lead with helpers is refused `409 has_helpers` unless `with_helpers`
/// is true, which stops its agent team and deletes the helpers' tasks first (ADR-0034 point 5, the slice S6 contract).
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteQuery {
    #[serde(default)]
    with_helpers: bool,
}

pub(crate) async fn delete(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, GatewayError> {
    // A team's helpers are its lead's person's: only their delete may stop the team. Anyone else's delete, and a repeat
    // of one that already went (which answers as the first did, GW-I-184), are the removal's own to answer.
    if let Ok(task) = service::authorize(&state, &user, &id, false).await
        && task.owner_id == user.id
    {
        let helpers =
            crate::teams::people_write::before_delete(&state, &id, query.with_helpers).await?;
        for helper in helpers {
            service::remove(&state, &user, &helper).await?;
        }
    }
    service::remove(&state, &user, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct List {
    #[serde(default)]
    all: bool,
    /// The opaque position after the previous page's last row (089).
    cursor: Option<String>,
    limit: Option<u32>,
    /// Search text matched in the caller's own titles only, as literal text (GW-I-121, SEC-044).
    q: Option<String>,
    /// `top` leaves agent-team members out, their lead carrying them (ADR-0034 S6); ignored with `q`, and filtering
    /// nothing while agent teams are off.
    scope: Option<Scope>,
}

/// The list's scope.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Scope {
    /// Top-level tasks only: no agent-team member.
    Top,
}

/// A listed task: the conversation and its run status, for the sidebar's status dot (GW-I-120).
#[derive(Serialize)]
pub(crate) struct Listed {
    #[serde(flatten)]
    conversation: Conversation,
    run_status: crate::channels::store::RunStatus,
}

#[derive(Serialize)]
pub(crate) struct Page {
    items: Vec<Listed>,
    /// The cursor for the next page; null when this page is not full.
    next_cursor: Option<String>,
}

pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Query(query): Query<List>,
) -> Result<Json<Page>, GatewayError> {
    if query.all && user.role != Role::Admin {
        return Err(GatewayError::Forbidden(
            "administrator role is required to list all metadata",
        ));
    }
    let limit = query.limit.unwrap_or(100);
    if !(1..=state.config.limits.page_size).contains(&limit) {
        return Err(GatewayError::BadRequest(
            "page limit exceeds its supported bound",
        ));
    }
    let q = crate::search::text(query.q.as_deref())?;
    // Members are left out only for a list without a search, while agent teams are on (ADR-0034 S6).
    let top = query.scope == Some(Scope::Top) && q.is_empty() && state.config.agent_teams.enabled;
    let pattern = crate::search::literal(&q);
    let (activity, after) = match query.cursor.as_deref() {
        Some(cursor) => {
            let (activity, id) = super::cursor::decode(cursor)?;
            (Some(activity), Some(id))
        }
        None => (None, None),
    };
    // Most recently active first; ties by id, so a keyset position never skips or repeats a row (089). A search
    // matches only the caller's own titles, even when an administrator lists all: titles are content (SEC-044).
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query_as!(Record,
            "SELECT id,owner_id,title,title_source,model_route,backend,state,template,pinned,created_at,updated_at,last_activity_at,workspace_id,mcp_servers,connectors,profile_id,profile_version,profile->>'name' AS profile_name,profile->>'display_name' AS profile_display_name,(profile->>'model_route') IS NOT NULL AS \"profile_sets_route?\",(profile->>'template') IS NOT NULL AS \"profile_sets_template?\" FROM conversations \
             WHERE state<>'deleted' AND ($1 OR owner_id=$2) AND ($3::timestamptz IS NULL OR (last_activity_at,id) < ($3,$4::uuid)) \
             AND ($6 = '' OR (owner_id=$2 AND title ILIKE '%' || $6 || '%')) \
             AND NOT ($7 AND EXISTS (SELECT 1 FROM agent_team_members m WHERE m.conversation_id=conversations.id)) \
             ORDER BY last_activity_at DESC, id DESC LIMIT $5",
            query.all, user.id, activity, after, i64::from(limit), pattern, top).fetch_all(&state.pool).await.map_err(GatewayError::from)
    }).await?;
    let mut conversations: Vec<Conversation> = rows
        .into_iter()
        .map(|record| Conversation::try_from(record).map(|item| item.for_viewer(user.id)))
        .collect::<Result<_, _>>()?;
    crate::teams::place::attach(&state, user.id, &mut conversations).await?;
    let full = conversations.len()
        == usize::try_from(limit).map_err(|_| GatewayError::BadRequest("invalid limit"))?;
    let mut statuses = run_statuses(&state, &conversations).await?;
    let items: Vec<Listed> = conversations
        .into_iter()
        .map(|conversation| {
            let run_status = super::uuid(&conversation.id)
                .ok()
                .and_then(|id| statuses.remove(&id))
                .unwrap_or(crate::channels::store::RunStatus::Idle);
            Listed {
                conversation,
                run_status,
            }
        })
        .collect();
    let next_cursor = match items.last().map(|item| &item.conversation) {
        Some(last) if full => Some(super::cursor::encode(
            last.last_activity_at,
            super::uuid(&last.id)?,
        )),
        _ => None,
    };
    Ok(Json(Page { items, next_cursor }))
}

/// The page's run statuses in one query, as the channels' lifecycle projection derives them: the latest lifecycle
/// event (GW-I-120).
async fn run_statuses(
    state: &State,
    conversations: &[Conversation],
) -> Result<std::collections::HashMap<uuid::Uuid, crate::channels::store::RunStatus>, GatewayError>
{
    let ids = conversations
        .iter()
        .map(|conversation| super::uuid(&conversation.id))
        .collect::<Result<Vec<_>, _>>()?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        latest_statuses(&state.pool, &ids)
            .await
            .map_err(GatewayError::from)
    })
    .await
}

/// Each conversation's run status from its latest lifecycle event, in one query (GW-I-120).
pub(crate) async fn latest_statuses(
    pool: &sqlx::PgPool,
    ids: &[uuid::Uuid],
) -> Result<std::collections::HashMap<uuid::Uuid, crate::channels::store::RunStatus>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT c.id AS \"id!\", latest.lifecycle AS \"latest?\" \
             FROM unnest($1::uuid[]) AS c(id) LEFT JOIN LATERAL (SELECT e.lifecycle FROM events e \
                     WHERE e.conversation_id = c.id AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1) latest ON true",
        ids
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.id,
                crate::channels::store::RunStatus::of(row.latest.as_deref()),
            )
        })
        .collect())
}
