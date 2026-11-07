//! `/api/v1/channels` (ADR-0014; G3): people create channels, manage members, post and read. Visibility follows
//! point 5: a channel a caller may not see is `404`, so ids don't leak; seeing a channel is not posting to it.
use super::store::{self, Listed, MemberId, MemberView, NewChannel};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::{Authz, Role, User},
    conversations::idempotent::Keyed,
    request::ApiJson,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tqa_types::{
    Author, AuthorKind, ChannelFilter, ChannelMessage, ChannelPost, ChannelRole, DecisionRule,
    MAX_CHANNEL_READ,
};
use uuid::Uuid;

/// The most channels one list returns.
const MAX_LISTED: i64 = 100;
/// A read's page when the caller names none.
const DEFAULT_READ: u32 = 50;

/// A channel as the API shows it.
#[derive(Serialize)]
pub(crate) struct ChannelView {
    id: Uuid,
    name: String,
    purpose: String,
    rule: DecisionRule,
    state: &'static str,
    owner: Owner,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    ended_at: Option<time::OffsetDateTime>,
    role: Option<ChannelRole>,
    member_count: i64,
    last_seq: i64,
}

#[derive(Serialize)]
struct Owner {
    id: Uuid,
    name: String,
}

impl From<Listed> for ChannelView {
    fn from(listed: Listed) -> Self {
        let channel = listed.channel;
        Self {
            id: channel.id,
            name: channel.name,
            purpose: channel.purpose,
            rule: channel.rule,
            state: if channel.ended { "ended" } else { "open" },
            owner: Owner {
                id: channel.owner_id,
                name: listed.owner_name,
            },
            created_at: channel.created_at,
            ended_at: channel.ended_at,
            role: listed.role,
            member_count: listed.member_count,
            last_seq: channel.last_seq,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Channels {
    items: Vec<ChannelView>,
    next_cursor: Option<String>,
}

/// A member as the API shows it.
#[derive(Serialize)]
pub(crate) struct Member {
    kind: &'static str,
    id: Uuid,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    role: ChannelRole,
    #[serde(with = "time::serde::rfc3339")]
    added_at: time::OffsetDateTime,
    /// An agent's run (GW-I-113); absent for a person.
    #[serde(skip_serializing_if = "Option::is_none")]
    run_status: Option<store::RunStatus>,
    /// Whether the caller may open an agent member's task page; absent for a person.
    #[serde(skip_serializing_if = "Option::is_none")]
    can_open: Option<bool>,
}

impl From<MemberView> for Member {
    fn from(view: MemberView) -> Self {
        let (kind, id) = match view.id {
            MemberId::User(id) => ("user", id),
            MemberId::Conversation(id) => ("conversation", id),
        };
        Self {
            kind,
            id,
            name: view.name,
            owner_name: view.owner_name,
            title: view.title,
            role: view.role,
            added_at: view.added_at,
            run_status: view.run_status,
            can_open: view.can_open,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Members {
    items: Vec<Member>,
}

/// A page of messages.
#[derive(Serialize)]
pub(crate) struct Page {
    pub(super) messages: Vec<ChannelMessage>,
    pub(super) next_since: u64,
    pub(super) more: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewChannelBody {
    name: String,
    #[serde(default)]
    purpose: String,
    rule: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleBody {
    role: ChannelRole,
}

pub(super) fn caller(user: &User) -> (Uuid, bool) {
    (user.id, user.role == Role::Admin)
}

fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}

pub(super) fn channel_id(value: &str) -> Result<Uuid, GatewayError> {
    Uuid::parse_str(value).map_err(|_| GatewayError::NotFound)
}

fn validated(body: &NewChannelBody) -> Result<DecisionRule, GatewayError> {
    let name = body.name.chars().count();
    if !(1..=80).contains(&name)
        || body.name.trim().is_empty()
        || body.name.chars().any(char::is_control)
    {
        return Err(invalid("name", InvalidReason::NameInvalid));
    }
    if body.purpose.chars().count() > 2000 {
        return Err(invalid("purpose", InvalidReason::ValueTooLong));
    }
    match body.rule.as_str() {
        "human" => Ok(DecisionRule::Human),
        "majority" => Ok(DecisionRule::Majority),
        _ => Err(invalid("rule", InvalidReason::RuleInvalid)),
    }
}

/// `POST /api/v1/channels {name, purpose?, rule}`: a channel whose creator is its `lead` member.
///
/// # Errors
/// `400` naming a field, storage failure.
pub(crate) async fn create(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    ApiJson(body): ApiJson<NewChannelBody>,
) -> Result<(StatusCode, Json<ChannelView>), GatewayError> {
    let rule = validated(&body)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let new = NewChannel {
            owner_id: user.id,
            name: &body.name,
            purpose: &body.purpose,
            rule,
        };
        let created = store::create(&state.pool, &new).await?;
        tracing::info!(user_id = %user.id, channel_id = %created.id, "channel created");
        let mut connection = state.pool.acquire().await?;
        let view = store::visible(&mut connection, caller(&user), created.id).await?;
        Ok((StatusCode::CREATED, Json(view.into())))
    })
    .await
}

/// `GET /api/v1/channels`: the channels the caller may see, newest activity first.
///
/// # Errors
/// Storage failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Channels>, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let items = store::listed(&mut connection, caller(&user), None, MAX_LISTED).await?;
        Ok(Json(Channels {
            items: items.into_iter().map(ChannelView::from).collect(),
            next_cursor: None,
        }))
    })
    .await
}

/// `GET /api/v1/channels/{id}`.
///
/// # Errors
/// `404`, storage failure.
pub(crate) async fn get(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
) -> Result<Json<ChannelView>, GatewayError> {
    let id = channel_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        Ok(Json(
            store::visible(&mut connection, caller(&user), id)
                .await?
                .into(),
        ))
    })
    .await
}

/// `POST /api/v1/channels/{id}/end`: the owner or an administrator ends the channel; ending is final.
///
/// # Errors
/// `404`, `403 not_owner`, storage failure.
pub(crate) async fn end(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
) -> Result<Json<ChannelView>, GatewayError> {
    let id = channel_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let seen = store::visible(&mut transaction, caller(&user), id).await?;
        if seen.channel.owner_id != user.id && user.role != Role::Admin {
            return Err(GatewayError::Denied("not_owner"));
        }
        // Open proposals close first (ADR-0014 point 13), while the channel still takes the decisions.
        let last = if seen.channel.ended {
            None
        } else {
            store::close_open(&mut transaction, id).await?
        };
        store::end(&mut transaction, id).await?;
        let view = store::visible(&mut transaction, caller(&user), id).await?;
        transaction.commit().await?;
        // Wakes open streams too, so they send the channel's new state.
        state.channels.published(id, last.unwrap_or(0));
        tracing::info!(user_id = %user.id, channel_id = %id, "channel ended");
        Ok(Json(view.into()))
    })
    .await
}

/// `DELETE /api/v1/channels/{id}`: the owner or an administrator deletes the channel with its log; audited.
///
/// # Errors
/// `404`, `403 not_owner`, storage failure.
pub(crate) async fn delete(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
) -> Result<StatusCode, GatewayError> {
    let id = channel_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let seen = store::visible(&mut transaction, caller(&user), id).await?;
        let owner = seen.channel.owner_id == user.id;
        if !owner && user.role != Role::Admin {
            return Err(GatewayError::Denied("not_owner"));
        }
        store::delete(&mut transaction, id).await?;
        let item = id.to_string();
        if owner {
            crate::settings::audit_user(&mut transaction, user.id, "channels", &item, "delete")
                .await?;
        } else {
            let change = crate::settings::Change {
                area: "channels",
                actor: Some(user.id),
                item: &item,
                action: "delete",
                before: None,
                after: None,
                value_changed: false,
            };
            crate::settings::audit(&mut transaction, &change).await?;
        }
        transaction.commit().await?;
        tracing::info!(user_id = %user.id, channel_id = %id, "channel deleted");
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

/// `GET /api/v1/channels/{id}/members`.
///
/// # Errors
/// `404`, storage failure.
pub(crate) async fn members(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
) -> Result<Json<Members>, GatewayError> {
    let id = channel_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        store::visible(&mut connection, caller(&user), id).await?;
        let items = store::members(&mut connection, id, caller(&user)).await?;
        Ok(Json(Members {
            items: items.into_iter().map(Member::from).collect(),
        }))
    })
    .await
}

fn member_id(kind: &str, id: &str) -> Result<MemberId, GatewayError> {
    let id = Uuid::parse_str(id).map_err(|_| GatewayError::NotFound)?;
    match kind {
        "user" => Ok(MemberId::User(id)),
        "conversation" => Ok(MemberId::Conversation(id)),
        _ => Err(GatewayError::NotFound),
    }
}

/// Whether `user` may add, change or remove `member` (ADR-0014 point 4): the channel's owner any member; a
/// conversation's owner their own conversation, when they own or belong to the channel, never as `lead`; a user
/// themselves only to leave.
async fn may_manage(
    connection: &mut sqlx::PgConnection,
    (seen, user): (&Listed, &User),
    member: MemberId,
    role: Option<ChannelRole>,
) -> Result<(), GatewayError> {
    if seen.channel.owner_id == user.id {
        return Ok(());
    }
    match member {
        MemberId::Conversation(conversation)
            if seen.role.is_some() && role != Some(ChannelRole::Lead) =>
        {
            let owner = store::conversation_owner(connection, conversation).await?;
            if owner == Some(user.id) {
                return Ok(());
            }
        }
        MemberId::User(id) if id == user.id && role.is_none() => return Ok(()),
        _ => {}
    }
    Err(GatewayError::Denied("not_owner"))
}

/// `PUT /api/v1/channels/{id}/members/{kind}/{member} {role}`: adds a member or changes its role.
///
/// # Errors
/// `404` (the channel, or the user or conversation to add), `403 not_owner`, `409 channel_full`, storage failure.
pub(crate) async fn put_member(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((id, kind, member)): Path<(String, String, String)>,
    ApiJson(body): ApiJson<RoleBody>,
) -> Result<Json<Member>, GatewayError> {
    let id = channel_id(&id)?;
    let member = member_id(&kind, &member)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let seen = store::visible(&mut transaction, caller(&user), id).await?;
        may_manage(&mut transaction, (&seen, &user), member, Some(body.role)).await?;
        let exists = match member {
            MemberId::Conversation(conversation) => {
                store::conversation_owner(&mut transaction, conversation)
                    .await?
                    .is_some()
            }
            MemberId::User(target) => {
                sqlx::query_scalar!(
                    "SELECT EXISTS (SELECT 1 FROM users WHERE id = $1) AS \"exists!\"",
                    target
                )
                .fetch_one(&mut *transaction)
                .await?
            }
        };
        if !exists {
            return Err(GatewayError::NotFound);
        }
        store::add_member(&mut transaction, id, member, (body.role, Some(user.id))).await?;
        let listed = store::members(&mut transaction, id, caller(&user)).await?;
        transaction.commit().await?;
        listed
            .into_iter()
            .find(|view| view.id == member)
            .map(|view| Json(view.into()))
            .ok_or(GatewayError::StoreUnavailable)
    })
    .await
}

/// `DELETE /api/v1/channels/{id}/members/{kind}/{member}`.
///
/// # Errors
/// `404`, `403 not_owner`, storage failure.
pub(crate) async fn remove_member(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((id, kind, member)): Path<(String, String, String)>,
) -> Result<StatusCode, GatewayError> {
    let id = channel_id(&id)?;
    let member = member_id(&kind, &member)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let seen = store::visible(&mut connection, caller(&user), id).await?;
        may_manage(&mut connection, (&seen, &user), member, None).await?;
        if !store::remove_member(&mut connection, id, member).await? {
            return Err(GatewayError::NotFound);
        }
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

/// A read: `since`, the filter, `limit`, and `wait` in seconds when the caller asked to wait.
pub(super) struct ReadQuery {
    pub since: u64,
    pub filter: ChannelFilter,
    pub limit: u32,
    pub wait: Option<u32>,
}

/// The read query: `since`, `limit`, the filter as flat parameters, and `wait` (1 to 300 seconds).
pub(super) fn read_query(raw: Option<&str>) -> Result<ReadQuery, GatewayError> {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(raw.unwrap_or("").as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    let mut since = 0;
    let mut limit = DEFAULT_READ;
    let mut wait = None;
    let mut filter = serde_json::Map::new();
    let number = |value: &str, field: &'static str| {
        value
            .parse::<u64>()
            .map_err(|_| invalid(field, InvalidReason::ParameterInvalid))
    };
    for (key, value) in &pairs {
        match key.as_str() {
            "since" => since = number(value, "since")?,
            "limit" => {
                limit = u32::try_from(number(value, "limit")?)
                    .ok()
                    .filter(|limit| (1..=MAX_CHANNEL_READ).contains(limit))
                    .ok_or(invalid("limit", InvalidReason::LimitOutOfRange))?;
            }
            "types" => {
                let types: Vec<&str> = value.split(',').collect();
                filter.insert("types".into(), serde_json::json!(types));
            }
            "parent" => {
                filter.insert("parent".into(), serde_json::json!(number(value, "parent")?));
            }
            "task_seq" => {
                filter.insert(
                    "task_seq".into(),
                    serde_json::json!(number(value, "task_seq")?),
                );
            }
            "author_kind" => {
                filter.insert("author_kind".into(), serde_json::json!(value));
            }
            "wait" => {
                wait = Some(
                    u32::try_from(number(value, "wait")?)
                        .ok()
                        .filter(|seconds| {
                            (1..=tqa_types::MAX_CHANNEL_WAIT_SECONDS).contains(seconds)
                        })
                        .ok_or(invalid("wait", InvalidReason::LimitOutOfRange))?,
                );
            }
            _ => return Err(invalid("query", InvalidReason::ParameterInvalid)),
        }
    }
    let filter = ChannelFilter::parse(serde_json::Value::Object(filter).to_string().as_bytes())
        .map_err(|error| invalid(filter_field(&error.path), InvalidReason::ParameterInvalid))?;
    Ok(ReadQuery {
        since,
        filter,
        limit,
        wait,
    })
}

fn filter_field(path: &str) -> &'static str {
    match path {
        "parent" => "parent",
        "task_seq" => "task_seq",
        _ if path.contains("author_kind") => "author_kind",
        _ => "types",
    }
}

/// `GET /api/v1/channels/{id}/messages?since&limit&types&parent&task_seq&author_kind`.
///
/// # Errors
/// `404`, `400` naming a parameter, storage failure.
pub(crate) async fn read(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Result<Json<Page>, GatewayError> {
    let id = channel_id(&id)?;
    let query = read_query(raw.as_deref())?;
    // People wait on the stream.
    if query.wait.is_some() {
        return Err(invalid("wait", InvalidReason::ParameterInvalid));
    }
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        store::visible(&mut connection, caller(&user), id).await?;
        let page = store::read(
            &mut connection,
            id,
            (query.since, &query.filter, query.limit),
        )
        .await?;
        Ok(Json(Page {
            messages: page.messages,
            next_since: page.next_since,
            more: page.more,
        }))
    })
    .await
}

/// The field a refused post names, from its path in the message schema.
pub(super) fn post_field(path: &str) -> &'static str {
    const FIELDS: [&str; 13] = [
        "body.type",
        "body.text",
        "body.title",
        "body.window_seconds",
        "body.proposal_seq",
        "body.reason",
        "body.spec",
        "body.claims",
        "body.task_seq",
        "body.summary",
        "body.refs",
        "parent",
        "body",
    ];
    FIELDS
        .into_iter()
        .find(|field| *field == path)
        .unwrap_or("body")
}

/// A post from the request body, refused naming its field.
pub(super) fn parse_post(bytes: &[u8]) -> Result<ChannelPost, GatewayError> {
    ChannelPost::parse(bytes).map_err(|error| {
        if error.path == "$" {
            invalid("body", InvalidReason::BodyInvalid)
        } else {
            invalid(post_field(&error.path), InvalidReason::MessageInvalid)
        }
    })
}

/// The author a person posts as.
pub(crate) fn person(user: &User) -> Author {
    Author {
        kind: AuthorKind::User,
        id: user.id.to_string(),
        name: Some(user.email.clone()),
    }
}

/// `POST /api/v1/channels/{id}/messages` with an `Idempotency-Key`: a voting member posts as themselves.
///
/// # Errors
/// `404`, `403 not_member`, `observer` or `not_claimant`, `409 channel_ended`, `429 channel_rate`, `400` naming the field, storage
/// failure.
pub(crate) async fn post(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(raw_id): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    let id = channel_id(&raw_id)?;
    let post = parse_post(&bytes)?;
    let keyed = Keyed::new(
        user.id,
        &headers,
        format!("POST /api/v1/channels/{id}/messages"),
        &post,
    )?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some((status, body)) = keyed.begin(&state, &mut transaction).await? {
            return Ok((status, Json(body)));
        }
        let seen = store::visible(&mut transaction, caller(&user), id).await?;
        match seen.role {
            None => return Err(GatewayError::Denied("not_member")),
            Some(role) if !role.may_post() => return Err(GatewayError::Denied("observer")),
            Some(_) => {}
        }
        let appended = store::post(&mut transaction, id, &person(&user), &post).await?;
        let body = serde_json::to_value(appended.first().ok_or(GatewayError::StoreUnavailable)?)?;
        keyed
            .record(&mut transaction, StatusCode::CREATED, &body)
            .await?;
        transaction.commit().await?;
        if let Some(last) = appended.last() {
            state.channels.published(id, last.seq);
        }
        Ok((StatusCode::CREATED, Json(body)))
    })
    .await
}

/// The answer to a claim: `201` with the new claim, `200` with the claimant's existing one.
pub(super) fn claimed(
    outcome: store::Claimed,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    Ok(match outcome {
        store::Claimed::New(message) => (StatusCode::CREATED, Json(serde_json::to_value(message)?)),
        store::Claimed::Held(message) => (StatusCode::OK, Json(serde_json::to_value(message)?)),
    })
}

/// `POST /api/v1/channels/{id}/tasks/{seq}/claim`: a voting member claims the task, up to its limit.
///
/// # Errors
/// `404`, `403 not_member` or `observer`, `409 channel_ended`, `429 channel_rate`, `not_a_task` or `already_claimed`, storage failure.
pub(crate) async fn claim(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((id, seq)): Path<(String, u64)>,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    let id = channel_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let seen = store::visible(&mut transaction, caller(&user), id).await?;
        match seen.role {
            None => return Err(GatewayError::Denied("not_member")),
            Some(role) if !role.may_post() => return Err(GatewayError::Denied("observer")),
            Some(_) => {}
        }
        let outcome = store::claim(&mut transaction, id, seq, &person(&user)).await?;
        transaction.commit().await?;
        if let store::Claimed::New(message) = &outcome {
            state.channels.published(id, message.seq);
        }
        claimed(outcome)
    })
    .await
}

/// A task on the board.
#[derive(Serialize)]
pub(crate) struct TaskItem {
    task_seq: u64,
    title: String,
    limit: u32,
    claims: Vec<serde_json::Value>,
    reports: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub(crate) struct Tasks {
    items: Vec<TaskItem>,
}

/// `GET /api/v1/channels/{id}/tasks`: the task board.
///
/// # Errors
/// `404`, storage failure.
pub(crate) async fn tasks(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
) -> Result<Json<Tasks>, GatewayError> {
    let id = channel_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        store::visible(&mut connection, caller(&user), id).await?;
        let items = store::tasks(&mut connection, id).await?;
        Ok(Json(Tasks {
            items: items
                .into_iter()
                .map(|task| {
                    TaskItem {
                    task_seq: task.task_seq,
                    title: task.title,
                    limit: task.limit,
                    claims: task
                        .claims
                        .into_iter()
                        .map(|(author, seq)| serde_json::json!({"author": author, "seq": seq}))
                        .collect(),
                    reports: task
                        .reports
                        .into_iter()
                        .map(|(author, seq, status)| {
                            serde_json::json!({"author": author, "seq": seq, "status": status})
                        })
                        .collect(),
                }
                })
                .collect(),
        }))
    })
    .await
}

/// A proposal's state for the page: computed by the gateway, where the rules live.
#[derive(Serialize)]
pub(crate) struct ProposalState {
    proposal_seq: u64,
    rule: DecisionRule,
    state: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    window_ends_at: time::OffsetDateTime,
    voting_members: usize,
    tally: Tally,
    votes: Vec<serde_json::Value>,
    decision: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Tally {
    accept: usize,
    reject: usize,
}

/// `GET /api/v1/channels/{id}/proposals/{seq}`: the tally of each voting member's latest vote, the window's end and
/// the decision.
///
/// # Errors
/// `404`, `409 not_a_proposal`, storage failure.
pub(crate) async fn proposal(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((id, seq)): Path<(String, u64)>,
) -> Result<Json<ProposalState>, GatewayError> {
    let id = channel_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        store::visible(&mut connection, caller(&user), id).await?;
        let proposal = store::proposal(&mut connection, id, seq).await?;
        let voters = store::voters(&mut connection, id).await?;
        let ballots = store::ballots(&mut connection, id, seq).await?;
        // Each voting member's latest vote.
        let mut latest: Vec<&(super::decide::Ballot, tqa_types::Author)> = Vec::new();
        for entry in ballots.iter().rev() {
            let member = voters.iter().any(|voter| voter.key == entry.0.voter);
            if member && !latest.iter().any(|seen| seen.0.voter == entry.0.voter) {
                latest.push(entry);
            }
        }
        latest.reverse();
        let count = |choice| latest.iter().filter(|entry| entry.0.choice == choice).count();
        Ok(Json(ProposalState {
            proposal_seq: seq,
            rule: proposal.rule,
            state: if proposal.decision.is_some() { "decided" } else { "open" },
            window_ends_at: proposal.closes_at(),
            voting_members: voters.len(),
            tally: Tally {
                accept: count(tqa_types::VoteChoice::Accept),
                reject: count(tqa_types::VoteChoice::Reject),
            },
            votes: latest
                .iter()
                .map(|(ballot, author)| serde_json::json!({"author": author, "choice": ballot.choice, "seq": ballot.seq}))
                .collect(),
            decision: proposal
                .decision
                .map(|(seq, outcome)| serde_json::json!({"seq": seq, "outcome": outcome})),
        }))
    })
    .await
}

/// Decides the proposals whose window has closed, one transaction each; how many were decided. The gateway runs it
/// every few seconds (ADR-0014 point 9).
///
/// # Errors
/// Storage failure.
pub(crate) async fn sweep(state: &State) -> Result<usize, GatewayError> {
    let due = {
        let mut connection = state.pool.acquire().await?;
        store::due_proposals(&mut connection, None, false).await?
    };
    let mut decided = 0;
    for (channel, seq) in due {
        let mut transaction = state.pool.begin().await?;
        let decision = store::decide(&mut transaction, channel, seq, false).await?;
        transaction.commit().await?;
        if let Some(decision) = decision {
            state.channels.published(channel, decision.seq);
            decided += 1;
        }
    }
    Ok(decided)
}

/// How often the sweep decides proposals whose window closed.
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Starts the decision sweep.
pub(crate) fn start(state: &Arc<State>) {
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_EVERY);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;
                () = owned.cancel.cancelled() => break,
                _ = interval.tick() => if sweep(&owned).await.is_err() && !owned.cancel.is_cancelled() {
                    tracing::warn!("channel decision sweep failed");
                },
            }
        }
    });
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemberByEmail {
    email: String,
    role: ChannelRole,
}

/// `PUT /api/v1/channels/{id}/members {email, role}`: the owner adds a person by exact email, or changes their role,
/// as workspace shares do: an unknown or disabled address is `404 user_not_found`, and lookups share the same per-user
/// rate, so addresses can't be probed.
///
/// # Errors
/// `404` (the channel, or the address), `403 not_owner`, `400` for the owner's own address, `409 channel_full`,
/// `429`, storage failure.
pub(crate) async fn put_member_by_email(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<MemberByEmail>,
) -> Result<Json<Member>, GatewayError> {
    let id = channel_id(&id)?;
    crate::workspaces::share_rate::SHARE_RATE.check(user.id, std::time::Instant::now())?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let seen = store::visible(&mut transaction, caller(&user), id).await?;
        if seen.channel.owner_id != user.id {
            return Err(GatewayError::Denied("not_owner"));
        }
        let person = sqlx::query_scalar!(
            "SELECT id FROM users WHERE email=$1::text::citext AND disabled_at IS NULL",
            body.email.trim()
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(GatewayError::UserNotFound)?;
        if person == user.id {
            return Err(GatewayError::BadRequest(
                "the owner already leads the channel",
            ));
        }
        let member = MemberId::User(person);
        store::add_member(&mut transaction, id, member, (body.role, Some(user.id))).await?;
        let listed = store::members(&mut transaction, id, caller(&user)).await?;
        transaction.commit().await?;
        listed
            .into_iter()
            .find(|view| view.id == member)
            .map(|view| Json(view.into()))
            .ok_or(GatewayError::StoreUnavailable)
    })
    .await
}
