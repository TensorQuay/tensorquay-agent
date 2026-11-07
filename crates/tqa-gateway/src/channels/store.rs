//! The channel log in the database (ADR-0014): channels, members and messages. A post takes the channel's next seq by
//! incrementing `channels.last_seq` in its transaction, so seqs are contiguous under concurrent posts. Who may post
//! what is checked by the routes; this layer keeps the log's own invariants (an open channel, a parent before the
//! message, the author as given).
use crate::{GatewayError, InvalidReason};
use sqlx::{PgConnection, PgPool};
use tqa_types::{
    Author, AuthorKind, ChannelFilter, ChannelMessage, ChannelPost, ChannelRole, DecisionRule,
    MessageBody, MessageType,
};
use uuid::Uuid;

/// A channel's stored settings and state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Channel {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub purpose: String,
    pub rule: DecisionRule,
    pub ended: bool,
    pub last_seq: i64,
    pub created_at: time::OffsetDateTime,
    pub ended_at: Option<time::OffsetDateTime>,
}

/// A member: a user or a conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemberId {
    User(Uuid),
    Conversation(Uuid),
}

/// What a new channel is given.
pub(crate) struct NewChannel<'a> {
    pub owner_id: Uuid,
    pub name: &'a str,
    pub purpose: &'a str,
    pub rule: DecisionRule,
}

/// A page of messages: those after `since` that match, at most `limit`, and whether more match past them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Page {
    pub messages: Vec<ChannelMessage>,
    pub next_since: u64,
    pub more: bool,
}

const fn rule_text(rule: DecisionRule) -> &'static str {
    match rule {
        DecisionRule::Human => "human",
        DecisionRule::Majority => "majority",
    }
}

const fn role_text(role: ChannelRole) -> &'static str {
    match role {
        ChannelRole::Lead => "lead",
        ChannelRole::Member => "member",
        ChannelRole::Observer => "observer",
    }
}

fn role_of(text: &str) -> Result<ChannelRole, GatewayError> {
    match text {
        "lead" => Ok(ChannelRole::Lead),
        "member" => Ok(ChannelRole::Member),
        "observer" => Ok(ChannelRole::Observer),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

const fn kind_text(kind: AuthorKind) -> &'static str {
    match kind {
        AuthorKind::Conversation => "conversation",
        AuthorKind::User => "user",
        AuthorKind::Gateway => "gateway",
    }
}

fn kind_of(text: &str) -> Result<AuthorKind, GatewayError> {
    match text {
        "conversation" => Ok(AuthorKind::Conversation),
        "user" => Ok(AuthorKind::User),
        "gateway" => Ok(AuthorKind::Gateway),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

const fn type_text(kind: MessageType) -> &'static str {
    match kind {
        MessageType::Text => "text",
        MessageType::Proposal => "proposal",
        MessageType::Vote => "vote",
        MessageType::Decision => "decision",
        MessageType::Task => "task",
        MessageType::Claim => "claim",
        MessageType::Report => "report",
        MessageType::Done => "done",
    }
}

fn seq_of(value: i64) -> Result<u64, GatewayError> {
    u64::try_from(value).map_err(|_| GatewayError::StoreUnavailable)
}

fn db_seq(value: u64) -> Result<i64, GatewayError> {
    i64::try_from(value).map_err(|_| GatewayError::Invalid {
        field: "since",
        reason: InvalidReason::ParameterInvalid,
    })
}

/// Creates a channel, its creator its `lead` member, in one transaction.
///
/// # Errors
/// Storage failure.
pub(crate) async fn create(pool: &PgPool, new: &NewChannel<'_>) -> Result<Channel, GatewayError> {
    let mut transaction = pool.begin().await?;
    let id = Uuid::now_v7();
    let row = sqlx::query!(
        "INSERT INTO channels (id, owner_id, name, purpose, rule) VALUES ($1, $2, $3, $4, $5) \
         RETURNING created_at",
        id,
        new.owner_id,
        new.name,
        new.purpose,
        rule_text(new.rule)
    )
    .fetch_one(&mut *transaction)
    .await?;
    add_member(
        &mut transaction,
        id,
        MemberId::User(new.owner_id),
        (ChannelRole::Lead, Some(new.owner_id)),
    )
    .await?;
    transaction.commit().await?;
    Ok(Channel {
        id,
        owner_id: new.owner_id,
        name: new.name.to_owned(),
        purpose: new.purpose.to_owned(),
        rule: new.rule,
        ended: false,
        last_seq: 0,
        created_at: row.created_at,
        ended_at: None,
    })
}

/// Adds `member` with `role`, or changes its role when it is a member already. A channel has at most
/// [`tqa_types::MAX_CHANNEL_MEMBERS`], counted under the channel's row lock so two adds can't both take the last place;
/// the caller's transaction holds it.
///
/// # Errors
/// `404` for an unknown channel, `409 channel_full`, storage failure.
pub(crate) async fn add_member(
    connection: &mut PgConnection,
    channel: Uuid,
    member: MemberId,
    (role, added_by): (ChannelRole, Option<Uuid>),
) -> Result<(), GatewayError> {
    sqlx::query_scalar!("SELECT id FROM channels WHERE id = $1 FOR UPDATE", channel)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(GatewayError::NotFound)?;
    let (user, conversation) = match member {
        MemberId::User(id) => (Some(id), None),
        MemberId::Conversation(id) => (None, Some(id)),
    };
    let changed = sqlx::query!(
        "UPDATE channel_members SET role = $4 WHERE channel_id = $1 \
         AND user_id IS NOT DISTINCT FROM $2 AND conversation_id IS NOT DISTINCT FROM $3",
        channel,
        user,
        conversation,
        role_text(role)
    )
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if changed == 0 {
        let members = sqlx::query_scalar!(
            "SELECT count(*) AS \"count!\" FROM channel_members WHERE channel_id = $1",
            channel
        )
        .fetch_one(&mut *connection)
        .await?;
        if usize::try_from(members).unwrap_or(usize::MAX) >= tqa_types::MAX_CHANNEL_MEMBERS {
            return Err(GatewayError::Conflict("channel_full"));
        }
        sqlx::query!(
            "INSERT INTO channel_members (channel_id, user_id, conversation_id, role, added_by) VALUES ($1, $2, $3, $4, $5)",
            channel,
            user,
            conversation,
            role_text(role),
            added_by
        )
        .execute(connection)
        .await?;
    }
    Ok(())
}

/// The member's role, or `None` when it isn't a member.
///
/// # Errors
/// Storage failure.
pub(crate) async fn role(
    connection: &mut PgConnection,
    channel: Uuid,
    member: MemberId,
) -> Result<Option<ChannelRole>, GatewayError> {
    let (user, conversation) = match member {
        MemberId::User(id) => (Some(id), None),
        MemberId::Conversation(id) => (None, Some(id)),
    };
    sqlx::query_scalar!(
        "SELECT role FROM channel_members WHERE channel_id = $1 \
         AND user_id IS NOT DISTINCT FROM $2 AND conversation_id IS NOT DISTINCT FROM $3",
        channel,
        user,
        conversation
    )
    .fetch_optional(connection)
    .await?
    .map(|text| role_of(&text))
    .transpose()
}

/// Refuses a post past the channel's day (ADR-0014): at most [`tqa_types::MAX_CHANNEL_MESSAGES_PER_DAY`] messages in the
/// last 24 hours, counted under the channel's row lock, which the caller holds. The gateway's own decisions are not
/// posts and are not refused, so a proposal can always close; they still count towards the day.
///
/// # Errors
/// `429 channel_rate` with the seconds until the window's oldest message ages out (a lower bound when decisions took
/// the count past the limit), storage failure.
pub(crate) async fn within_the_day(
    connection: &mut PgConnection,
    channel: Uuid,
) -> Result<(), GatewayError> {
    let day = sqlx::query!(
        "SELECT count(*) AS \"count!\", GREATEST(1, CEIL(EXTRACT(EPOCH FROM min(created_at) + INTERVAL '24 hours' \
         - CURRENT_TIMESTAMP)))::bigint AS retry FROM channel_messages \
         WHERE channel_id = $1 AND created_at > CURRENT_TIMESTAMP - INTERVAL '24 hours'",
        channel
    )
    .fetch_one(&mut *connection)
    .await?;
    if day.count < i64::from(tqa_types::MAX_CHANNEL_MESSAGES_PER_DAY) {
        return Ok(());
    }
    Err(GatewayError::ChannelRate(
        day.retry
            .and_then(|retry| u64::try_from(retry).ok())
            .unwrap_or(1),
    ))
}

/// Appends a validated post by `author` and returns the stored message. An ended channel refuses it
/// (`channel_ended`), and a parent must be an earlier message of the channel. Runs in the caller's transaction, which
/// must be one: a refusal after the seq is taken relies on the rollback to leave no gap.
///
/// # Errors
/// `404` for an unknown channel, `409 channel_ended`, `400 message_invalid` naming `parent`, storage failure.
pub(crate) async fn append(
    connection: &mut PgConnection,
    channel: Uuid,
    author: &Author,
    post: &ChannelPost,
) -> Result<ChannelMessage, GatewayError> {
    append_body(connection, channel, author, (post.parent, &post.body)).await
}

/// [`append`] for any body, including those only the gateway writes (`decision`, `claim`).
///
/// # Errors
/// As [`append`].
pub(crate) async fn append_body(
    connection: &mut PgConnection,
    channel: Uuid,
    author: &Author,
    (parent, body): (Option<u64>, &MessageBody),
) -> Result<ChannelMessage, GatewayError> {
    let state = sqlx::query_scalar!(
        "SELECT state FROM channels WHERE id = $1 FOR UPDATE",
        channel
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if state == "ended" {
        return Err(GatewayError::Conflict("channel_ended"));
    }
    if author.kind != AuthorKind::Gateway {
        within_the_day(connection, channel).await?;
    }
    let seq = sqlx::query_scalar!(
        "UPDATE channels SET last_seq = last_seq + 1 WHERE id = $1 RETURNING last_seq",
        channel
    )
    .fetch_one(&mut *connection)
    .await?;
    let parent = parent.map(db_seq).transpose()?;
    if parent.is_some_and(|parent| parent >= seq) {
        return Err(GatewayError::Invalid {
            field: "parent",
            reason: InvalidReason::MessageInvalid,
        });
    }
    let (task, proposal) = match body {
        MessageBody::Claim { task_seq } | MessageBody::Report { task_seq, .. } => {
            (Some(db_seq(*task_seq)?), None)
        }
        MessageBody::Vote { proposal_seq, .. } | MessageBody::Decision { proposal_seq, .. } => {
            (None, Some(db_seq(*proposal_seq)?))
        }
        _ => (None, None),
    };
    if task.is_some_and(|task| task >= seq) || proposal.is_some_and(|proposal| proposal >= seq) {
        return Err(GatewayError::Invalid {
            field: if task.is_some() {
                "body.task_seq"
            } else {
                "body.proposal_seq"
            },
            reason: InvalidReason::MessageInvalid,
        });
    }
    let json = serde_json::to_value(body).map_err(GatewayError::from)?;
    let created = sqlx::query_scalar!(
        "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, parent, task_seq, \
         proposal_seq, body) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING created_at",
        channel,
        seq,
        kind_text(author.kind),
        author.id,
        author.name,
        type_text(body.message_type()),
        parent,
        task,
        proposal,
        json
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(ChannelMessage {
        seq: seq_of(seq)?,
        ts: timestamp(created)?,
        author: author.clone(),
        parent: parent.map(seq_of).transpose()?,
        body: body.clone(),
    })
}

/// The messages after `since` that match `filter`, at most `limit`, in seq order.
///
/// # Errors
/// Storage failure.
pub(crate) async fn read(
    connection: &mut PgConnection,
    channel: Uuid,
    (since, filter, limit): (u64, &ChannelFilter, u32),
) -> Result<Page, GatewayError> {
    let types: Vec<String> = filter
        .types
        .iter()
        .map(|kind| type_text(*kind).to_owned())
        .collect();
    let rows = sqlx::query!(
        "SELECT seq, author_kind, author_id, author_name, parent, body, created_at FROM channel_messages \
         WHERE channel_id = $1 AND seq > $2 \
         AND (cardinality($3::text[]) = 0 OR type = ANY($3)) \
         AND ($4::bigint IS NULL OR parent = $4) \
         AND ($5::bigint IS NULL OR task_seq = $5 OR (seq = $5 AND type = 'task')) \
         AND ($6::text IS NULL OR author_kind = $6) \
         ORDER BY seq LIMIT $7",
        channel,
        db_seq(since)?,
        &types,
        filter.parent.map(db_seq).transpose()?,
        filter.task_seq.map(db_seq).transpose()?,
        filter.author_kind.map(kind_text),
        i64::from(limit) + 1
    )
    .fetch_all(connection)
    .await?;
    let more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    let mut messages = Vec::with_capacity(rows.len());
    for row in rows
        .into_iter()
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
    {
        messages.push(ChannelMessage {
            seq: seq_of(row.seq)?,
            ts: timestamp(row.created_at)?,
            author: Author {
                kind: kind_of(&row.author_kind)?,
                id: row.author_id,
                name: row.author_name,
            },
            parent: row.parent.map(seq_of).transpose()?,
            body: serde_json::from_value(row.body).map_err(|_| GatewayError::StoreUnavailable)?,
        });
    }
    let next_since = messages.last().map_or(since, |message| message.seq);
    Ok(Page {
        messages,
        next_since,
        more,
    })
}

fn rule_of(text: &str) -> Result<DecisionRule, GatewayError> {
    match text {
        "human" => Ok(DecisionRule::Human),
        "majority" => Ok(DecisionRule::Majority),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

fn timestamp(at: time::OffsetDateTime) -> Result<String, GatewayError> {
    at.format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| GatewayError::StoreUnavailable)
}

mod claims;
mod decisions;
mod members;

pub(crate) use claims::{Claimed, claim, tasks};
pub(crate) use decisions::{ballots, close_open, decide, due_proposals, post, proposal, voters};
pub(crate) use members::{
    Listed, MemberView, RunStatus, conversation_author, conversation_owner, delete, end, exists,
    listed, members, memberships, remove_member, visible,
};

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
