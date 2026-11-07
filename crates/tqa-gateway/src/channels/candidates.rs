//! `GET /api/v1/channels/candidates?q&limit&channel` (GW-I-119): one search for a channel's new members, over people
//! and the caller's own recent tasks. Tasks are the caller's active conversations, newest activity first, matched on
//! the title, each with its run status (the members list's projection). People are, for a member, only those they
//! already work with (a workspace shared either way, a channel in common), by email prefix, plus an exact email match of
//! any active account, since nothing may enumerate accounts; an administrator matches every account by prefix. The
//! caller is never a candidate. A member's exact lookup (a query with `@`) is what could find a stranger, so it shares
//! the by-email share and member rate; the rest reveals nothing new and is not limited. With `channel`, each candidate
//! says whether it is already a member.
use super::{api, store};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::{Authz, Role},
};
use axum::{
    Json,
    extract::{Query, State as AppState},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 50;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CandidateQuery {
    #[serde(default)]
    q: String,
    limit: Option<u32>,
    channel: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Person {
    user_id: Uuid,
    email: String,
    already_member: bool,
}

#[derive(Serialize)]
pub(crate) struct Task {
    conversation_id: Uuid,
    title: String,
    run_status: store::RunStatus,
    already_member: bool,
}

#[derive(Serialize)]
pub(crate) struct Candidates {
    people: Vec<Person>,
    tasks: Vec<Task>,
}

/// The candidates for the caller's query.
///
/// # Errors
/// `400` for a query over 100 characters or a limit outside 1-50, `404` for a channel the caller can't see, `429` past
/// the exact-lookup rate, storage failure.
pub(crate) async fn candidates(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Query(query): Query<CandidateQuery>,
) -> Result<Json<Candidates>, GatewayError> {
    let q = crate::search::text(Some(&query.q))?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(GatewayError::Invalid {
            field: "limit",
            reason: InvalidReason::LimitOutOfRange,
        });
    }
    let channel = query.channel.as_deref().map(api::channel_id).transpose()?;
    let admin = user.role == Role::Admin;
    let exact = q.contains('@');
    if exact && !admin {
        crate::workspaces::share_rate::SHARE_RATE.check(user.id, std::time::Instant::now())?;
    }
    let pattern = crate::search::literal(&q);
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        if let Some(channel) = channel {
            store::visible(&mut connection, api::caller(&user), channel).await?;
        }
        let search = Search {
            user: user.id,
            q: &q,
            pattern: &pattern,
            admin,
            exact,
            channel,
            limit: i64::from(limit),
        };
        let tasks = tasks(&mut connection, &search).await?;
        let people = people(&mut connection, &search).await?;
        Ok(Json(Candidates { people, tasks }))
    })
    .await
}

/// What one search looks for.
struct Search<'a> {
    user: Uuid,
    q: &'a str,
    pattern: &'a str,
    admin: bool,
    exact: bool,
    channel: Option<Uuid>,
    limit: i64,
}

/// The caller's own active tasks matching the title, newest activity first, with their run status.
async fn tasks(
    connection: &mut sqlx::PgConnection,
    search: &Search<'_>,
) -> Result<Vec<Task>, GatewayError> {
    Ok(sqlx::query!(
        "SELECT c.id, c.title, latest.lifecycle AS \"latest?\", \
         ($3::uuid IS NOT NULL AND EXISTS (SELECT 1 FROM channel_members m WHERE m.channel_id = $3 \
                 AND m.conversation_id = c.id)) AS \"member!\" \
         FROM conversations c LEFT JOIN LATERAL (SELECT e.lifecycle FROM events e WHERE e.conversation_id = c.id \
                 AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1) latest ON true \
         WHERE c.owner_id = $1 AND c.state = 'active' AND ($2 = '' OR c.title ILIKE '%' || $2 || '%') \
         ORDER BY c.last_activity_at DESC, c.id LIMIT $4",
        search.user,
        search.pattern,
        search.channel,
        search.limit
    )
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(|row| Task {
        conversation_id: row.id,
        title: row.title,
        run_status: store::RunStatus::of(row.latest.as_deref()),
        already_member: row.member,
    })
    .collect())
}

/// The people the caller may add: known ones (all, for an administrator) by prefix, and anyone by exact email.
async fn people(
    connection: &mut sqlx::PgConnection,
    search: &Search<'_>,
) -> Result<Vec<Person>, GatewayError> {
    Ok(sqlx::query!(
        "WITH known AS ( \
             SELECT w.owner_id AS id FROM workspaces w JOIN workspace_shares s ON s.workspace_id = w.id \
             WHERE s.user_id = $1 AND w.state <> 'deleted' \
             UNION SELECT s.user_id FROM workspace_shares s JOIN workspaces w ON w.id = s.workspace_id \
             WHERE w.owner_id = $1 AND w.state <> 'deleted' \
             UNION SELECT theirs.user_id FROM channel_members mine \
             JOIN channel_members theirs ON theirs.channel_id = mine.channel_id \
             WHERE mine.user_id = $1 AND theirs.user_id IS NOT NULL) \
         SELECT u.id, u.email::text AS \"email!\", \
         ($5::uuid IS NOT NULL AND EXISTS (SELECT 1 FROM channel_members m WHERE m.channel_id = $5 \
                 AND m.user_id = u.id)) AS \"member!\" \
         FROM users u WHERE u.disabled_at IS NULL AND u.id <> $1 AND ( \
             ($4 AND u.email = $2::text::citext) \
             OR (($3 OR u.id IN (SELECT id FROM known)) AND ($2 = '' OR u.email::text ILIKE $6 || '%'))) \
         ORDER BY u.email LIMIT $7",
        search.user,
        search.q,
        search.admin,
        search.exact,
        search.channel,
        search.pattern,
        search.limit
    )
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(|row| Person {
        user_id: row.id,
        email: row.email,
        already_member: row.member,
    })
    .collect())
}
