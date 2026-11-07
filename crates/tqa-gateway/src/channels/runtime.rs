//! `/runtime/channels…` (ADR-0014 points 4 and 6; G4): a run reads and posts to the channels its conversation belongs
//! to, with its runtime token, from its sandbox's address. Membership is the authority: a conversation that isn't a
//! member gets `403 not_member`, checked on every call, so a removal takes effect at once. The author is always the
//! run's conversation.
use super::{
    api::{self, Page},
    store::{self, MemberId},
};
use crate::{GatewayError, app::State, conversations::idempotent::Keyed};
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tqa_types::{Author, AuthorKind, ChannelRole, DecisionRule};
use uuid::Uuid;

/// A channel as its member run sees it.
#[derive(Serialize)]
pub(crate) struct Membership {
    id: Uuid,
    name: String,
    purpose: String,
    role: ChannelRole,
    rule: DecisionRule,
    state: &'static str,
}

#[derive(Serialize)]
pub(crate) struct Memberships {
    channels: Vec<Membership>,
}

/// The run's conversation, from its token and the sandbox's address.
async fn conversation(
    state: &State,
    headers: &HeaderMap,
    peer: SocketAddr,
) -> Result<Uuid, GatewayError> {
    let token = crate::auth::token::bearer(headers)?;
    let (conversation, _) = state
        .runtime_tokens
        .resolve(&token, peer.ip())
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    Uuid::parse_str(conversation.as_str()).map_err(|_| GatewayError::Unauthorized)
}

/// The conversation's role in the channel: `404` for an unknown channel, `403 not_member` when it isn't one.
async fn member_role(
    connection: &mut sqlx::PgConnection,
    channel: Uuid,
    conversation: Uuid,
) -> Result<ChannelRole, GatewayError> {
    if let Some(role) =
        store::role(connection, channel, MemberId::Conversation(conversation)).await?
    {
        return Ok(role);
    }
    if store::exists(connection, channel).await? {
        Err(GatewayError::Denied("not_member"))
    } else {
        Err(GatewayError::NotFound)
    }
}

/// `GET /runtime/channels`: the channels the run's conversation belongs to.
///
/// # Errors
/// `401`, storage failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<Memberships>, GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let channels = store::memberships(&mut connection, conversation).await?;
        Ok(Json(Memberships {
            channels: channels
                .into_iter()
                .map(|(channel, role)| Membership {
                    id: channel.id,
                    name: channel.name,
                    purpose: channel.purpose,
                    role,
                    rule: channel.rule,
                    state: if channel.ended { "ended" } else { "open" },
                })
                .collect(),
        }))
    })
    .await
}

/// `GET /runtime/channels/{id}/messages?since&limit&types&parent&task_seq&author_kind&wait`. With `wait` (1 to 300
/// seconds) it returns as soon as a matching message is past `since`, or an empty page when the wait runs out; the
/// sandbox sits idle meanwhile, and a caller that goes away ends the wait.
///
/// # Errors
/// `401`, `403 not_member` (also when removed during the wait), `404`, `400` naming a parameter, storage failure.
pub(crate) async fn read(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> Result<Json<Page>, GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    let id = api::channel_id(&id)?;
    let query = api::read_query(raw.as_deref())?;
    // Subscribed before the first read, so a post committed in between still wakes the wait.
    let mut woken = state.channels.subscribe(id);
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_secs(u64::from(query.wait.unwrap_or(0)));
    loop {
        let page = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            let mut connection = state.pool.acquire().await?;
            member_role(&mut connection, id, conversation).await?;
            store::read(
                &mut connection,
                id,
                (query.since, &query.filter, query.limit),
            )
            .await
        })
        .await?;
        if !page.messages.is_empty() || query.wait.is_none() {
            return Ok(Json(Page {
                messages: page.messages,
                next_since: page.next_since,
                more: page.more,
            }));
        }
        tokio::select! {
            biased;
            () = state.cancel.cancelled() => return Err(GatewayError::StoreUnavailable),
            () = tokio::time::sleep_until(deadline) => {
                return Ok(Json(Page { messages: Vec::new(), next_since: query.since, more: false }));
            }
            changed = woken.changed() => {
                if changed.is_err() {
                    woken = state.channels.subscribe(id);
                }
            }
        }
    }
}

/// `POST /runtime/channels/{id}/messages`: the run posts as its conversation. An `Idempotency-Key` is optional here;
/// the tools send one per call.
///
/// # Errors
/// `401`, `403 not_member`, `observer` or `not_claimant`, `404`, `409 channel_ended`, `429 channel_rate`, `400` naming the field, storage
/// failure.
pub(crate) async fn post(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(raw_id): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    let id = api::channel_id(&raw_id)?;
    let post = api::parse_post(&bytes)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let (owner, name) = store::conversation_author(&mut transaction, conversation)
            .await?
            .ok_or(GatewayError::Unauthorized)?;
        // The key is scoped to the conversation: another conversation of the same owner may send the same key.
        let keyed = if headers.contains_key("idempotency-key") {
            let route = format!("POST /runtime/channels/{id}/messages as {conversation}");
            Some(Keyed::for_conversation(
                owner,
                conversation,
                &headers,
                route,
                &post,
            )?)
        } else {
            None
        };
        if let Some(keyed) = &keyed
            && let Some((status, body)) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok((status, Json(body)));
        }
        if !member_role(&mut transaction, id, conversation)
            .await?
            .may_post()
        {
            return Err(GatewayError::Denied("observer"));
        }
        let author = Author {
            kind: AuthorKind::Conversation,
            id: conversation.to_string(),
            name,
        };
        let appended = store::post(&mut transaction, id, &author, &post).await?;
        let body = serde_json::to_value(appended.first().ok_or(GatewayError::StoreUnavailable)?)?;
        if let Some(keyed) = &keyed {
            keyed
                .record(&mut transaction, StatusCode::CREATED, &body)
                .await?;
        }
        transaction.commit().await?;
        if let Some(last) = appended.last() {
            state.channels.published(id, last.seq);
        }
        Ok((StatusCode::CREATED, Json(body)))
    })
    .await
}

/// `POST /runtime/channels/{id}/tasks/{seq}/claim`: the run claims the task for its conversation, up to the task's
/// limit; a retry returns its claim (`200`).
///
/// # Errors
/// `401`, `403 not_member` or `observer`, `404`, `409 channel_ended`, `429 channel_rate`, `not_a_task` or `already_claimed`, storage
/// failure.
pub(crate) async fn claim(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path((raw_id, seq)): Path<(String, u64)>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    let id = api::channel_id(&raw_id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if !member_role(&mut transaction, id, conversation)
            .await?
            .may_post()
        {
            return Err(GatewayError::Denied("observer"));
        }
        let (_, name) = store::conversation_author(&mut transaction, conversation)
            .await?
            .ok_or(GatewayError::Unauthorized)?;
        let author = Author {
            kind: AuthorKind::Conversation,
            id: conversation.to_string(),
            name,
        };
        let outcome = store::claim(&mut transaction, id, seq, &author).await?;
        transaction.commit().await?;
        if let store::Claimed::New(message) = &outcome {
            state.channels.published(id, message.seq);
        }
        api::claimed(outcome)
    })
    .await
}
