//! Who sees a channel and who belongs to it (ADR-0014 points 4 and 5).
use super::{Channel, MemberId, db_seq, kind_text, role_of, rule_of};
use crate::GatewayError;
use sqlx::PgConnection;
use tqa_types::{Author, ChannelRole};
use uuid::Uuid;

/// A channel as a caller sees it: its settings, its owner's name, its size, and the caller's role (none for an
/// administrator or a member conversation's owner who isn't a member).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    pub channel: Channel,
    pub owner_name: String,
    pub member_count: i64,
    pub role: Option<ChannelRole>,
}

/// The channels `user` may see (ADR-0014 point 5): those they own or belong to, those a conversation of theirs
/// belongs to, and every channel for an administrator; newest activity first. `only` narrows it to one channel.
///
/// # Errors
/// Storage failure.
pub(crate) async fn listed(
    connection: &mut PgConnection,
    (user, admin): (Uuid, bool),
    only: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Listed>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT c.id, c.owner_id, c.name, c.purpose, c.rule, c.state, c.last_seq, c.created_at, c.ended_at, \
         o.email::text AS \"owner_name!\", \
         (SELECT count(*) FROM channel_members m WHERE m.channel_id = c.id) AS \"member_count!\", \
         (SELECT m.role FROM channel_members m WHERE m.channel_id = c.id AND m.user_id = $1) AS role \
         FROM channels c JOIN users o ON o.id = c.owner_id \
         WHERE c.kind = 'channel' AND ($3::uuid IS NULL OR c.id = $3) AND ($2 OR c.owner_id = $1 \
           OR EXISTS (SELECT 1 FROM channel_members m WHERE m.channel_id = c.id AND m.user_id = $1) \
           OR EXISTS (SELECT 1 FROM channel_members m JOIN conversations v ON v.id = m.conversation_id \
                      WHERE m.channel_id = c.id AND v.owner_id = $1)) \
         ORDER BY COALESCE((SELECT max(x.created_at) FROM channel_messages x WHERE x.channel_id = c.id), c.created_at) DESC, \
           c.id DESC LIMIT $4",
        user,
        admin,
        only,
        limit
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(Listed {
                channel: Channel {
                    id: row.id,
                    owner_id: row.owner_id,
                    name: row.name,
                    purpose: row.purpose,
                    rule: rule_of(&row.rule)?,
                    ended: row.state == "ended",
                    last_seq: row.last_seq,
                    created_at: row.created_at,
                    ended_at: row.ended_at,
                },
                owner_name: row.owner_name,
                member_count: row.member_count,
                role: row.role.as_deref().map(role_of).transpose()?,
            })
        })
        .collect()
}

/// One channel as `user` sees it.
///
/// # Errors
/// `404` when it doesn't exist or `user` may not see it; storage failure.
pub(crate) async fn visible(
    connection: &mut PgConnection,
    (user, admin): (Uuid, bool),
    channel: Uuid,
) -> Result<Listed, GatewayError> {
    listed(connection, (user, admin), Some(channel), 1)
        .await?
        .pop()
        .ok_or(GatewayError::NotFound)
}

/// An agent member's run as the lifecycle projection sees it (ADR-0016), as the conversation page reads it: a run that
/// started and never ended (a crash) reads `running` there too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunStatus {
    /// No run has started.
    Idle,
    /// The latest run started or resumed and has not ended.
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    /// From the latest lifecycle event. A run that parked for an approval before ADR-0035 ended with it.
    pub(crate) fn of(latest: Option<&str>) -> Self {
        match latest {
            None => Self::Idle,
            Some("run.completed") => Self::Completed,
            Some("run.cancelled") => Self::Cancelled,
            Some("run.started" | "run.resumed") => Self::Running,
            // `run.failed`, and a run parked for an approval before ADR-0035, which ended with it.
            Some(_) => Self::Failed,
        }
    }
}

/// A member as listed: a user by email, a conversation by its agent profile's display name and its owner's email.
/// A conversation's title is shown only to its owner and administrators; its run status to everyone who sees the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemberView {
    pub id: MemberId,
    pub name: String,
    pub owner_name: Option<String>,
    pub title: Option<String>,
    pub role: ChannelRole,
    pub added_at: time::OffsetDateTime,
    /// A conversation's; never a user's.
    pub run_status: Option<RunStatus>,
    /// Whether the viewer may open a conversation member's task page: its owner, or an administrator (the private
    /// view); never set for a user.
    pub can_open: Option<bool>,
}

/// The channel's members, in the order they were added.
///
/// # Errors
/// Storage failure.
pub(crate) async fn members(
    connection: &mut PgConnection,
    channel: Uuid,
    (viewer, admin): (Uuid, bool),
) -> Result<Vec<MemberView>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT m.user_id, m.conversation_id, m.role, m.added_at, u.email::text AS user_email, v.title AS \"title?\", \
         v.owner_id AS \"conversation_owner?\", ou.email::text AS conversation_owner_email, \
         COALESCE(v.profile->>'display_name', v.profile->>'name') AS profile_name, \
         latest.lifecycle AS \"latest?\" \
         FROM channel_members m LEFT JOIN users u ON u.id = m.user_id \
         LEFT JOIN conversations v ON v.id = m.conversation_id LEFT JOIN users ou ON ou.id = v.owner_id \
         LEFT JOIN LATERAL (SELECT e.lifecycle FROM events e WHERE e.conversation_id = m.conversation_id \
                            AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1) latest ON true \
         WHERE m.channel_id = $1 ORDER BY m.added_at, m.user_id, m.conversation_id",
        channel
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let role = role_of(&row.role)?;
            match (row.user_id, row.conversation_id) {
                (Some(user), None) => Ok(MemberView {
                    id: MemberId::User(user),
                    name: row.user_email.unwrap_or_default(),
                    owner_name: None,
                    title: None,
                    role,
                    added_at: row.added_at,
                    run_status: None,
                    can_open: None,
                }),
                (None, Some(conversation)) => Ok(MemberView {
                    id: MemberId::Conversation(conversation),
                    name: row.profile_name.unwrap_or_else(|| "Agent".into()),
                    owner_name: row.conversation_owner_email,
                    // Titles are content (SEC-044): its owner's only, never an administrator's.
                    title: row.title.filter(|_| row.conversation_owner == Some(viewer)),
                    role,
                    added_at: row.added_at,
                    run_status: Some(RunStatus::of(row.latest.as_deref())),
                    can_open: Some(admin || row.conversation_owner == Some(viewer)),
                }),
                _ => Err(GatewayError::StoreUnavailable),
            }
        })
        .collect()
}

/// Removes a member; whether it was one.
///
/// # Errors
/// Storage failure.
pub(crate) async fn remove_member(
    connection: &mut PgConnection,
    channel: Uuid,
    member: MemberId,
) -> Result<bool, GatewayError> {
    let (user, conversation) = match member {
        MemberId::User(id) => (Some(id), None),
        MemberId::Conversation(id) => (None, Some(id)),
    };
    let removed = sqlx::query!(
        "DELETE FROM channel_members WHERE channel_id = $1 \
         AND user_id IS NOT DISTINCT FROM $2 AND conversation_id IS NOT DISTINCT FROM $3",
        channel,
        user,
        conversation
    )
    .execute(connection)
    .await?
    .rows_affected();
    Ok(removed > 0)
}

/// Ends an open channel; ending an ended one changes nothing.
///
/// # Errors
/// Storage failure.
pub(crate) async fn end(connection: &mut PgConnection, channel: Uuid) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE channels SET state = 'ended', ended_at = CURRENT_TIMESTAMP WHERE id = $1 AND state = 'open'",
        channel
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Deletes a channel with its members, messages and claims.
///
/// # Errors
/// Storage failure.
pub(crate) async fn delete(
    connection: &mut PgConnection,
    channel: Uuid,
) -> Result<(), GatewayError> {
    sqlx::query!("DELETE FROM channels WHERE id = $1", channel)
        .execute(connection)
        .await?;
    Ok(())
}

/// Whether `author` holds a claim on the task.
///
/// # Errors
/// Storage failure.
pub(crate) async fn is_claimant(
    connection: &mut PgConnection,
    channel: Uuid,
    task: u64,
    author: &Author,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM channel_claims WHERE channel_id = $1 AND task_seq = $2 \
         AND claimant_kind = $3 AND claimant_id = $4) AS \"claimed!\"",
        channel,
        db_seq(task)?,
        kind_text(author.kind),
        author.id
    )
    .fetch_one(connection)
    .await?)
}

/// A conversation's owner, when it exists.
///
/// # Errors
/// Storage failure.
pub(crate) async fn conversation_owner(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<Uuid>, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT owner_id FROM conversations WHERE id = $1",
        conversation
    )
    .fetch_optional(connection)
    .await?)
}

/// The channels `conversation` belongs to, with its role, oldest membership first.
///
/// # Errors
/// Storage failure.
pub(crate) async fn memberships(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<(Channel, ChannelRole)>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT c.id, c.owner_id, c.name, c.purpose, c.rule, c.state, c.last_seq, c.created_at, c.ended_at, m.role \
         FROM channel_members m JOIN channels c ON c.id = m.channel_id \
         WHERE m.conversation_id = $1 ORDER BY m.added_at, c.id",
        conversation
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok((
                Channel {
                    id: row.id,
                    owner_id: row.owner_id,
                    name: row.name,
                    purpose: row.purpose,
                    rule: rule_of(&row.rule)?,
                    ended: row.state == "ended",
                    last_seq: row.last_seq,
                    created_at: row.created_at,
                    ended_at: row.ended_at,
                },
                role_of(&row.role)?,
            ))
        })
        .collect()
}

/// A conversation's owner and its agent's display name (the profile's display name or name), when it exists.
///
/// # Errors
/// Storage failure.
pub(crate) async fn conversation_author(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<(Uuid, Option<String>)>, GatewayError> {
    Ok(sqlx::query!(
        "SELECT owner_id, COALESCE(profile->>'display_name', profile->>'name') AS name FROM conversations WHERE id = $1",
        conversation
    )
    .fetch_optional(connection)
    .await?
    .map(|row| (row.owner_id, row.name)))
}

/// Whether the channel exists.
///
/// # Errors
/// Storage failure.
pub(crate) async fn exists(
    connection: &mut PgConnection,
    channel: Uuid,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM channels WHERE id = $1) AS \"exists!\"",
        channel
    )
    .fetch_one(connection)
    .await?)
}
