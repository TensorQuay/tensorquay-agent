//! `/api/v1/workspaces`. Personal API tokens may call every operation: this is user work, not account administration.
use super::{Access, access, valid_name};
use crate::{
    GatewayError,
    app::State,
    auth::{Authz, Role, User},
    request::ApiJson,
};
use axum::{
    Json,
    extract::{Path, Query, State as AppState},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Workspaces listed at once.
pub(super) const MAX_LISTED: i64 = 1000;

/// A workspace as its caller may see it; `name` is content and absent from admin metadata views.
#[derive(Serialize)]
pub(crate) struct Workspace {
    id: Uuid,
    owner_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    state: String,
    /// `owner`, `write` or `read`; absent in an administrator's metadata view.
    #[serde(skip_serializing_if = "Option::is_none")]
    access: Option<&'static str>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    last_used_at: Option<time::OffsetDateTime>,
    /// Bytes charged when last measured (allocated blocks, and at least each file's length; after a sandbox lets go,
    /// and after a fork), checked against the quota.
    size_bytes: Option<i64>,
    #[serde(with = "time::serde::rfc3339::option")]
    size_measured_at: Option<time::OffsetDateTime>,
    /// What it is (GW-I-215): `helper_copy` for an agent team member's own copy, else `standard`.
    kind: crate::workspaces::Kind,
    /// For a fork: its source, and whether the source had no live sandbox while it was copied.
    #[serde(skip_serializing_if = "Option::is_none")]
    fork: Option<Fork>,
    /// Who owns it, its quota, and whether a task holds it now.
    owner: Owner,
    quota_bytes: Option<u64>,
    lease: Option<Lease>,
    /// The owner's view only: people it is shared with, and active tasks bound to it.
    share_count: Option<i64>,
    task_count: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct Owner {
    id: Uuid,
    email: String,
}

/// The task holding the workspace's lease (manager/lease.rs `held`). `title` only for the caller's own task: tasks
/// are private, so a share holder learns only that the workspace is in use.
#[derive(Clone, Serialize)]
pub(crate) struct Lease {
    conversation_id: Uuid,
    title: Option<String>,
    running: bool,
    /// The holder's keep-alive pin: its environment is never retired for someone else or for a delete (GW-I-199).
    pinned: bool,
}

#[derive(Serialize)]
pub(crate) struct Fork {
    from: Uuid,
    consistent: bool,
}

struct Row {
    id: Uuid,
    owner_id: Uuid,
    name: String,
    state: String,
    access: Option<String>,
    created_at: time::OffsetDateTime,
    updated_at: time::OffsetDateTime,
    last_used_at: Option<time::OffsetDateTime>,
    size_bytes: Option<i64>,
    size_measured_at: Option<time::OffsetDateTime>,
    forked_from: Option<Uuid>,
    fork_consistent: Option<bool>,
    kind: String,
    /// Its own quota; none for a workspace created before quotas were a workspace's own.
    quota_bytes: Option<i64>,
    owner_email: String,
    share_count: i64,
    task_count: i64,
}

impl Row {
    /// The API form; `default_quota` is the installation's, for a workspace without its own.
    fn view(self, user: Uuid, default_quota: u64, lease: Option<Lease>) -> Workspace {
        let quota = self
            .quota_bytes
            .and_then(|bytes| u64::try_from(bytes).ok())
            .unwrap_or(default_quota);
        let access = if self.owner_id == user {
            Some(Access::Owner)
        } else {
            match self.access.as_deref() {
                Some("write") => Some(Access::Write),
                Some("read") => Some(Access::Read),
                _ => None,
            }
        };
        // Without access (an administrator's metadata view), the name is withheld.
        let visible = access.is_some();
        Workspace {
            id: self.id,
            owner_id: self.owner_id,
            name: visible.then_some(self.name),
            size_bytes: self.size_bytes,
            size_measured_at: self.size_measured_at,
            kind: crate::workspaces::Kind::stored(&self.kind),
            fork: self
                .forked_from
                .zip(self.fork_consistent)
                .filter(|_| visible)
                .map(|(from, consistent)| Fork { from, consistent }),
            owner: Owner {
                id: self.owner_id,
                email: self.owner_email,
            },
            quota_bytes: Some(quota),
            lease,
            share_count: (self.owner_id == user).then_some(self.share_count),
            task_count: (self.owner_id == user).then_some(self.task_count),
            state: self.state,
            access: access.map(Access::as_str),
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_used_at: self.last_used_at,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Workspaces {
    items: Vec<Workspace>,
    /// Where the next page starts; absent on the last page, and always without `limit` (GW-I-117).
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    #[serde(default)]
    all: bool,
    /// A page of 1 through 100; without it, the whole list up to its cap, as before.
    limit: Option<u32>,
    cursor: Option<String>,
    #[serde(default)]
    sort: super::listing::Sort,
    #[serde(default)]
    filter: super::listing::Filter,
    /// Search text matched in the names the caller may read, as literal text (GW-I-122, SEC-044).
    q: Option<String>,
}

/// A listed row with its sort keys, as the database computed them.
struct Listed {
    row: Row,
    sort_micros: Option<i64>,
    sort_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewWorkspace {
    name: String,
    /// Removed with cloning on create (ADR-0037 §7): read only so a request that still sends it is refused by name.
    #[serde(default)]
    git_url: Option<serde::de::IgnoredAny>,
    /// The profile the workspace is for; it must be one the caller may use.
    #[serde(default)]
    profile: Option<Uuid>,
    /// The environment it is for, whose disk it gets; the default environment when absent.
    #[serde(default)]
    template: Option<String>,
}

/// `PATCH` body: a new name.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Update {
    name: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteQuery {
    #[serde(default)]
    with_conversations: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShareBody {
    access: ShareAccess,
}

/// `PUT .../shares {email, access}`: share by exact email, never by listing accounts.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShareByEmail {
    email: String,
    access: ShareAccess,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ShareAccess {
    Read,
    Write,
}

#[derive(Serialize)]
pub(crate) struct Share {
    user_id: Uuid,
    email: String,
    access: String,
}

#[derive(Serialize)]
pub(crate) struct Shares {
    items: Vec<Share>,
}

pub(super) fn id(value: &str) -> Result<Uuid, GatewayError> {
    crate::auth::path_id(value)
}

async fn owned(state: &State, user: &User, workspace: Uuid) -> Result<(), GatewayError> {
    let mut connection = state.pool.acquire().await?;
    if access(&mut connection, user.id, workspace).await? == Access::Owner {
        Ok(())
    } else {
        Err(GatewayError::Forbidden("workspace ownership is required"))
    }
}

pub(super) async fn fetch(
    state: &State,
    user: &User,
    workspace: Uuid,
) -> Result<Workspace, GatewayError> {
    let row = sqlx::query_as!(Row,
        "SELECT w.id, w.owner_id, w.name, w.state, s.access AS \"access?\", w.created_at, w.updated_at, w.last_used_at, w.size_bytes, w.size_measured_at, w.forked_from, w.fork_consistent, w.kind, w.quota_bytes, o.email::text AS \"owner_email!\", (SELECT count(*) FROM workspace_shares x WHERE x.workspace_id=w.id) AS \"share_count!\", (SELECT count(*) FROM conversations c WHERE c.workspace_id=w.id AND c.state='active') AS \"task_count!\" FROM workspaces w JOIN users o ON o.id=w.owner_id LEFT JOIN workspace_shares s ON s.workspace_id=w.id AND s.user_id=$2 WHERE w.id=$1 AND w.state<>'deleted'",
        workspace, user.id).fetch_optional(&state.pool).await?.ok_or(GatewayError::NotFound)?;
    let mut leases = leases(state, user, &[workspace]).await?;
    Ok(row.view(
        user.id,
        state.config.workspaces.quota_bytes,
        leases.remove(&workspace),
    ))
}

/// The leases held on `workspaces`, as the caller may see them.
async fn leases(
    state: &State,
    user: &User,
    workspaces: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Lease>, GatewayError> {
    let mut connection = state.pool.acquire().await?;
    let held = crate::manager::lease::held(&mut connection, workspaces).await?;
    let holders: Vec<Uuid> = held.iter().map(|held| held.holder).collect();
    let tasks = sqlx::query!(
        "SELECT id, owner_id, title FROM conversations WHERE id = ANY($1)",
        &holders
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut leases = std::collections::HashMap::new();
    for held in held {
        let task = tasks.iter().find(|task| task.id == held.holder);
        let running = crate::events::run_activity(&mut *connection, held.holder).await?
            == crate::events::RunActivity::Active;
        leases.insert(
            held.workspace,
            Lease {
                conversation_id: held.holder,
                title: task
                    .filter(|task| task.owner_id == user.id)
                    .map(|task| task.title.clone()),
                running,
                pinned: held.pinned,
            },
        );
    }
    Ok(leases)
}

/// `GET /api/v1/workspaces`: own and shared workspaces; `all=true` (administrators) lists metadata only.
///
/// # Errors
/// `403` for `all` without the administrator role; a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Query(query): Query<ListQuery>,
) -> Result<Json<Workspaces>, GatewayError> {
    if query.all && user.role != Role::Admin {
        return Err(GatewayError::Forbidden(
            "administrator role is required to list all metadata",
        ));
    }
    let (rows, next_cursor) = page(&state, &user, &query).await?;
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut leases = crate::io::bounded(
        state.config.io_timeout(),
        &state.cancel,
        leases(&state, &user, &ids),
    )
    .await?;
    let quota = state.config.workspaces.quota_bytes;
    Ok(Json(Workspaces {
        items: rows
            .into_iter()
            .map(|row| {
                let lease = leases.remove(&row.id);
                row.view(user.id, quota, lease)
            })
            .collect(),
        next_cursor,
    }))
}

/// One page of the caller's workspaces for `query` (GW-I-117), and where the next starts.
///
/// # Errors
/// `400` for a page size out of range, search text over 100 characters, or a cursor this gateway did not issue for this
/// sort, filter and search, or a store failure.
async fn page(
    state: &State,
    user: &User,
    query: &ListQuery,
) -> Result<(Vec<Row>, Option<String>), GatewayError> {
    let q = crate::search::text(query.q.as_deref())?;
    let pattern = crate::search::literal(&q);
    let (limit, after, after_time) = super::listing::position(
        query.limit,
        query.cursor.as_deref(),
        query.sort,
        query.filter,
        &q,
    )?;
    let (sort, filter) = (query.sort.as_str(), query.filter.as_str());
    let mut listed = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let records = sqlx::query!(
            "WITH v AS (SELECT w.id, w.owner_id, w.name, w.state, s.access, w.created_at, w.updated_at, w.last_used_at, w.size_bytes, w.size_measured_at, w.forked_from, w.fork_consistent, w.kind, w.quota_bytes, o.email::text AS owner_email, (SELECT count(*) FROM workspace_shares x WHERE x.workspace_id=w.id) AS share_count, (SELECT count(*) FROM conversations c WHERE c.workspace_id=w.id AND c.state='active') AS task_count, CASE WHEN $3='recent' THEN COALESCE(w.last_used_at, w.created_at) WHEN $3='created' THEN w.created_at END AS sort_time, lower(w.name) AS sort_name, (w.lease_conversation_id IS NOT NULL AND w.lease_expires_at > CURRENT_TIMESTAMP) AS in_use, (s.user_id IS NOT NULL AND w.owner_id <> $1) AS shared_in FROM workspaces w JOIN users o ON o.id=w.owner_id LEFT JOIN workspace_shares s ON s.workspace_id=w.id AND s.user_id=$1 WHERE w.state<>'deleted' AND ($2 OR w.owner_id=$1 OR s.user_id IS NOT NULL)) \
             SELECT v.id AS \"id!\", v.owner_id AS \"owner_id!\", v.name AS \"name!\", v.state AS \"state!\", v.access AS \"access?\", v.created_at AS \"created_at!\", v.updated_at AS \"updated_at!\", v.last_used_at, v.size_bytes, v.size_measured_at, v.forked_from, v.fork_consistent, v.kind AS \"kind!\", v.quota_bytes, v.owner_email AS \"owner_email!\", v.share_count AS \"share_count!\", v.task_count AS \"task_count!\", (EXTRACT(EPOCH FROM v.sort_time) * 1000000)::bigint AS sort_micros, v.sort_name AS \"sort_name!\" FROM v \
             WHERE ($4='all' OR ($4='in_use' AND v.in_use) OR ($4='idle' AND NOT v.in_use) OR ($4='shared_with_me' AND v.shared_in)) \
             AND ($9 = '' OR ((v.owner_id = $1 OR v.access IS NOT NULL) AND v.name ILIKE '%' || $9 || '%')) \
             AND ($7::uuid IS NULL OR CASE WHEN $3='name' THEN (v.sort_name > $6 OR (v.sort_name = $6 AND v.id > $7)) ELSE (v.sort_time < $5 OR (v.sort_time = $5 AND v.id > $7)) END) \
             ORDER BY v.sort_time DESC, CASE WHEN $3='name' THEN v.sort_name END, v.id LIMIT $8",
            user.id,
            query.all,
            sort,
            filter,
            after_time,
            after.as_ref().and_then(|cursor| cursor.name.clone()),
            after.as_ref().map(|cursor| cursor.id),
            limit + 1,
            pattern
        )
        .fetch_all(&state.pool)
        .await?;
        Ok(records
            .into_iter()
            .map(|record| Listed {
                row: Row {
                    id: record.id,
                    owner_id: record.owner_id,
                    name: record.name,
                    state: record.state,
                    access: record.access,
                    created_at: record.created_at,
                    updated_at: record.updated_at,
                    last_used_at: record.last_used_at,
                    size_bytes: record.size_bytes,
                    size_measured_at: record.size_measured_at,
                    forked_from: record.forked_from,
                    fork_consistent: record.fork_consistent,
                    kind: record.kind,
                    quota_bytes: record.quota_bytes,
                    owner_email: record.owner_email,
                    share_count: record.share_count,
                    task_count: record.task_count,
                },
                sort_micros: record.sort_micros,
                sort_name: record.sort_name,
            })
            .collect::<Vec<_>>())
    })
    .await?;
    let more = query.limit.is_some() && listed.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    listed.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = more.then(|| listed.last()).flatten().map(|last| {
        super::listing::Cursor {
            sort: query.sort,
            filter: query.filter,
            micros: (query.sort != super::listing::Sort::Name)
                .then_some(last.sort_micros)
                .flatten(),
            name: (query.sort == super::listing::Sort::Name).then(|| last.sort_name.clone()),
            id: last.row.id,
            search: super::listing::search_key(&q),
        }
        .encode()
    });
    Ok((
        listed.into_iter().map(|listed| listed.row).collect(),
        next_cursor,
    ))
}

/// `POST /api/v1/workspaces {name, profile?, template?}`: an empty workspace. A task clones a repository into it with
/// plain git and a key (ADR-0037 §7).
///
/// # Errors
/// An invalid name, `400` (`git_url`, `removed`) for the removed clone on create, `422 unknown_reference` (`profile`)
/// for a profile the caller may not use, or a store or storage failure.
pub(crate) async fn create(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    ApiJson(body): ApiJson<NewWorkspace>,
) -> Result<(StatusCode, Json<Workspace>), GatewayError> {
    let name = valid_name(&body.name)?.to_owned();
    if body.git_url.is_some() {
        return Err(GatewayError::Invalid {
            field: "git_url",
            reason: crate::InvalidReason::Removed,
        });
    }
    profile_usable(&state, &user, body.profile).await?;
    let workspace = Uuid::now_v7();
    // On the microVM backend the workspace is an image the backend creates for its first sandbox.
    let path = if state.config.uses_block_images() {
        None
    } else {
        Some(state.storage.workspace(workspace).await?)
    };
    // Its disk is its environment's, and stays with it (the environment sizes design).
    let quota = crate::workspaces::quota_for(
        &state.config,
        crate::environments::choose(&state.settings.current(), body.template.as_deref())?,
    );
    let inserted = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "INSERT INTO workspaces (id, owner_id, name, quota_bytes) VALUES ($1,$2,$3,$4)",
            workspace,
            user.id,
            name,
            crate::workspaces::stored_quota(quota)?
        )
        .execute(&state.pool)
        .await?;
        fetch(&state, &user, workspace).await
    })
    .await;
    match inserted {
        Ok(view) => {
            tracing::info!(user_id = %user.id, workspace_id = %workspace, "workspace created");
            Ok((StatusCode::CREATED, Json(view)))
        }
        Err(error) => {
            if let Some(path) = &path {
                state.storage.remove_workspace(path).await?;
            }
            Err(error)
        }
    }
}

/// The profile the workspace is for, when named, must be one the caller may use.
async fn profile_usable(
    state: &State,
    user: &User,
    profile: Option<Uuid>,
) -> Result<(), GatewayError> {
    let Some(id) = profile else {
        return Ok(());
    };
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        crate::profiles::chosen(&mut connection, state, user.id, id).await
    })
    .await?;
    Ok(())
}

/// `GET /api/v1/workspaces/{id}`: the owner or a share holder.
///
/// # Errors
/// `404`, `403` without access, or a store failure.
pub(crate) async fn get(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(workspace): Path<String>,
) -> Result<Json<Workspace>, GatewayError> {
    let workspace = id(&workspace)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        access(&mut connection, user.id, workspace).await?;
        Ok(Json(fetch(&state, &user, workspace).await?))
    })
    .await
}

/// `PATCH /api/v1/workspaces/{id} {name}`: the owner renames the workspace.
///
/// # Errors
/// An invalid or empty body, `404`, `403` for anyone but the owner, or a store failure.
pub(crate) async fn update(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(workspace): Path<String>,
    ApiJson(body): ApiJson<Update>,
) -> Result<Json<Workspace>, GatewayError> {
    let workspace = id(&workspace)?;
    let name = valid_name(
        body.name
            .as_deref()
            .ok_or(GatewayError::BadRequest("a workspace update names a field"))?,
    )?
    .to_owned();
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        owned(&state, &user, workspace).await?;
        sqlx::query!(
            "UPDATE workspaces SET name=$2, updated_at=CURRENT_TIMESTAMP WHERE id=$1",
            workspace,
            name
        )
        .execute(&state.pool)
        .await?;
        Ok(Json(fetch(&state, &user, workspace).await?))
    })
    .await
}

/// `DELETE /api/v1/workspaces/{id}[?with_conversations=true]`: the owner deletes the workspace and its directory.
/// Never while a task's environment on it has a run going or is pinned; a kept, idle one is retired for it, as for a
/// new task's handover (GW-I-199). Bound active tasks refuse it unless `with_conversations`, and then only when they
/// all belong to the owner, who deletes them with it, and none of them leads an agent team with helpers: those go
/// in bulk, past the lead's own delete that stops its team, so the lead is deleted with its helpers first (GW-I-108).
///
/// # Errors
/// `404`, `403`, `409 workspace_in_use`, `409 workspace_not_ready` while it is being forked, `409 has_helpers`, or
/// a store or storage failure.
pub(crate) async fn delete(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(workspace): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, GatewayError> {
    let workspace = id(&workspace)?;
    let removed = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        owned(&state, &user, workspace).await?;
        let mut transaction = state.pool.begin().await?;
        let current = sqlx::query_scalar!("SELECT state FROM workspaces WHERE id=$1 FOR UPDATE", workspace).fetch_one(&mut *transaction).await?;
        // A fork still copying into this directory finishes (or fails) first.
        if current == "forking" {
            return Err(GatewayError::Conflict("workspace_not_ready"));
        }
        let Some(holders) = crate::manager::lease::retirable(&mut transaction, workspace, state.config.io_timeout() * 3).await? else {
            return Err(GatewayError::Conflict("workspace_in_use"));
        };
        let bound = sqlx::query!("SELECT id, owner_id FROM conversations WHERE workspace_id=$1 AND state='active' FOR UPDATE", workspace)
            .fetch_all(&mut *transaction).await?;
        let others = bound.iter().any(|task| task.owner_id != user.id);
        if !bound.is_empty() && (!query.with_conversations || others) {
            return Err(GatewayError::Conflict("workspace_in_use"));
        }
        let tasks: Vec<Uuid> = bound.iter().map(|task| task.id).collect();
        // A lead among them would go without its team stopping, its helpers working for a deleted lead: refused as
        // deleting that lead without its helpers is, whatever the agent-teams switch says now (GW-I-108).
        let leads_helpers = sqlx::query_scalar!(
            "SELECT EXISTS (SELECT 1 FROM agent_teams t JOIN agent_team_members m ON m.team_id = t.id \
             JOIN conversations c ON c.id = m.conversation_id \
             WHERE t.lead_conversation = ANY($1) AND c.state <> 'deleted') AS \"leads_helpers!\"",
            &tasks
        )
        .fetch_one(&mut *transaction)
        .await?;
        if leads_helpers {
            return Err(GatewayError::Conflict("has_helpers"));
        }
        sqlx::query!("UPDATE conversations SET state='deleted',updated_at=CURRENT_TIMESTAMP WHERE id=ANY($1)", &tasks)
            .execute(&mut *transaction).await?;
        sqlx::query!("DELETE FROM runtime_tokens WHERE conversation_id=ANY($1)", &tasks).execute(&mut *transaction).await?;
        crate::host_keys::end_tasks(&mut transaction, &tasks).await?;
        sqlx::query!("DELETE FROM workspace_shares WHERE workspace_id=$1", workspace).execute(&mut *transaction).await?;
        sqlx::query!("UPDATE workspaces SET state='deleted',updated_at=CURRENT_TIMESTAMP WHERE id=$1", workspace)
            .execute(&mut *transaction).await?;
        // Content-free: who deleted which workspace and which of their tasks went with it, by id.
        crate::settings::audit_user(&mut transaction, user.id, "workspaces", &workspace.to_string(), "delete").await?;
        for task in &tasks {
            crate::settings::audit_user(&mut transaction, user.id, "tasks", &task.to_string(), "delete").await?;
        }
        transaction.commit().await?;
        Ok((tasks, holders))
    }).await?;
    let (removed, holders) = removed;
    // A kept environment goes as for a handover; its files stay until it has let go, then the purge takes them.
    for holder in &holders {
        if let Ok(conversation) = holder.to_string().parse() {
            state
                .manager
                .retire_for_switch(&conversation, crate::manager::Teardown::WorkspaceDeleted)?;
        }
    }
    for task in removed {
        if let Ok(conversation) = task.to_string().parse() {
            crate::conversations::gone(&state, &conversation)?;
        }
    }
    // With no environment on it the files go at once; the purge removes the rest and makes sure of these.
    if holders.is_empty() {
        state.storage.remove_workspace_tree(workspace).await?;
    }
    state.purge.wake();
    tracing::info!(user_id = %user.id, workspace_id = %workspace, "workspace deleted");
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/workspaces/{id}/shares` (owner).
///
/// # Errors
/// `404`, `403`, or a store failure.
pub(crate) async fn shares(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(workspace): Path<String>,
) -> Result<Json<Shares>, GatewayError> {
    let workspace = id(&workspace)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        owned(&state, &user, workspace).await?;
        let items = sqlx::query_as!(Share,
            "SELECT s.user_id, u.email::text AS \"email!\", s.access FROM workspace_shares s JOIN users u ON u.id=s.user_id WHERE s.workspace_id=$1 ORDER BY s.created_at, s.user_id LIMIT $2",
            workspace, MAX_LISTED).fetch_all(&state.pool).await?;
        Ok(Json(Shares { items }))
    }).await
}

/// `PUT /api/v1/workspaces/{id}/shares/{user_id} {access}` (owner): grants or changes a share.
///
/// # Errors
/// `404` for an unknown workspace or user, `400` for sharing with oneself, `403`, or a store failure.
pub(crate) async fn share(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((workspace, member)): Path<(String, String)>,
    ApiJson(body): ApiJson<ShareBody>,
) -> Result<Json<Share>, GatewayError> {
    let (workspace, member) = (id(&workspace)?, id(&member)?);
    let level = match body.access {
        ShareAccess::Read => "read",
        ShareAccess::Write => "write",
    };
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        owned(&state, &user, workspace).await?;
        if member == user.id {
            return Err(GatewayError::BadRequest("the owner already has access"));
        }
        let email = sqlx::query_scalar!("SELECT email::text AS \"email!\" FROM users WHERE id=$1 AND disabled_at IS NULL", member)
            .fetch_optional(&state.pool).await?.ok_or(GatewayError::NotFound)?;
        sqlx::query!("INSERT INTO workspace_shares (workspace_id,user_id,access,created_by) VALUES ($1,$2,$3,$4) ON CONFLICT (workspace_id,user_id) DO UPDATE SET access=EXCLUDED.access, updated_at=CURRENT_TIMESTAMP",
            workspace, member, level, user.id).execute(&state.pool).await?;
        tracing::info!(user_id = %user.id, workspace_id = %workspace, member_id = %member, access = level, "workspace shared");
        Ok(Json(Share { user_id: member, email, access: level.to_owned() }))
    }).await
}

/// `PUT /api/v1/workspaces/{id}/shares {email, access}` (owner, GW-I-049): shares with the one active
/// account whose email matches exactly (case-insensitive). Unknown and inactive addresses run the same lookup and get
/// the same `404 user_not_found`, and attempts are limited per caller, so nothing enumerates accounts.
///
/// # Errors
/// `404` for an unknown workspace or `user_not_found`, `400` for sharing with oneself, `403`, `429 busy`, or a store
/// failure.
pub(crate) async fn share_by_email(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(workspace): Path<String>,
    ApiJson(body): ApiJson<ShareByEmail>,
) -> Result<Json<Share>, GatewayError> {
    let workspace = id(&workspace)?;
    let level = match body.access {
        ShareAccess::Read => "read",
        ShareAccess::Write => "write",
    };
    super::share_rate::SHARE_RATE.check(user.id, std::time::Instant::now())?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        owned(&state, &user, workspace).await?;
        let member = sqlx::query!(
            "SELECT id, email::text AS \"email!\" FROM users WHERE email=$1::text::citext AND disabled_at IS NULL",
            body.email.trim()
        )
        .fetch_optional(&state.pool)
        .await?
        .ok_or(GatewayError::UserNotFound)?;
        if member.id == user.id {
            return Err(GatewayError::BadRequest("the owner already has access"));
        }
        sqlx::query!("INSERT INTO workspace_shares (workspace_id,user_id,access,created_by) VALUES ($1,$2,$3,$4) ON CONFLICT (workspace_id,user_id) DO UPDATE SET access=EXCLUDED.access, updated_at=CURRENT_TIMESTAMP",
            workspace, member.id, level, user.id).execute(&state.pool).await?;
        tracing::info!(user_id = %user.id, workspace_id = %workspace, member_id = %member.id, access = level, "workspace shared by email");
        Ok(Json(Share { user_id: member.id, email: member.email, access: level.to_owned() }))
    }).await
}

/// `DELETE /api/v1/workspaces/{id}/shares/{user_id}` (owner).
///
/// # Errors
/// `404` for no such share, `403`, or a store failure.
pub(crate) async fn unshare(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((workspace, member)): Path<(String, String)>,
) -> Result<StatusCode, GatewayError> {
    let (workspace, member) = (id(&workspace)?, id(&member)?);
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        owned(&state, &user, workspace).await?;
        let removed = sqlx::query!("DELETE FROM workspace_shares WHERE workspace_id=$1 AND user_id=$2", workspace, member)
            .execute(&state.pool).await?;
        if removed.rows_affected() == 0 {
            return Err(GatewayError::NotFound);
        }
        tracing::info!(user_id = %user.id, workspace_id = %workspace, member_id = %member, "workspace unshared");
        Ok(StatusCode::NO_CONTENT)
    }).await
}
