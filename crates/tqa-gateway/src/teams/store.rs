//! Agent-team storage (ADR-0034, the slice S1 contract). A team's log is a channel of kind `agent_team`; its messages
//! carry an addressee, and each reader's last-read seq lives on its team or member row. A read's answer is recorded
//! under its call's key until that call's `tool.result` is stored, which advances the reader's seq in the same
//! transaction: delivery exactly once (GW-I-109, GW-I-195). Roles come from these rows, never from a request.
use super::{
    roles::{self, Addressee, Role},
    validate,
};
use crate::GatewayError;
use sqlx::PgConnection;
use tqa_types::{NoticeKind, TeamMessage, TeamMessageBody, TeamMessageType, TeamPost};
use uuid::Uuid;

pub(crate) use super::read_shape::messages;

/// The calling conversation's place in an open agent team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Caller {
    pub(crate) team: Uuid,
    pub(crate) channel: Uuid,
    pub(crate) lead: Uuid,
    pub(crate) role: Role,
    /// The member's name, or `lead`.
    pub(crate) name: String,
}

/// The conversation's role in a team that hasn't ended, from the stored rows; `None` when it is in none.
///
/// # Errors
/// Storage failure.
pub(crate) async fn caller(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<Caller>, GatewayError> {
    if let Some(row) = sqlx::query!(
        "SELECT id, channel_id FROM agent_teams WHERE lead_conversation = $1 AND state <> 'ended'",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?
    {
        return Ok(Some(Caller {
            team: row.id,
            channel: row.channel_id,
            lead: conversation,
            role: Role::Lead,
            name: "lead".to_owned(),
        }));
    }
    let member = sqlx::query!(
        "SELECT m.team_id, m.name, t.channel_id, t.lead_conversation FROM agent_team_members m \
         JOIN agent_teams t ON t.id = m.team_id WHERE m.conversation_id = $1 AND t.state <> 'ended'",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(member.map(|row| Caller {
        team: row.team_id,
        channel: row.channel_id,
        lead: row.lead_conversation,
        role: Role::Member,
        name: row.name,
    }))
}

/// The conversation's role in its latest agent team that has ended, for a caller in no live team (ADR-0034 point
/// 10): team tools then answer `team_ended`, and a lead's first one carries the end note. `None` when it was never in
/// a team.
///
/// # Errors
/// Storage failure.
pub(crate) async fn ended(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<Caller>, GatewayError> {
    if let Some(row) = sqlx::query!(
        "SELECT id, channel_id FROM agent_teams WHERE lead_conversation = $1 AND state = 'ended' \
         ORDER BY ended_at DESC NULLS LAST, id DESC LIMIT 1",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?
    {
        return Ok(Some(Caller {
            team: row.id,
            channel: row.channel_id,
            lead: conversation,
            role: Role::Lead,
            name: "lead".to_owned(),
        }));
    }
    let member = sqlx::query!(
        "SELECT m.team_id, m.name, t.channel_id, t.lead_conversation FROM agent_team_members m \
         JOIN agent_teams t ON t.id = m.team_id WHERE m.conversation_id = $1 AND t.state = 'ended'",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(member.map(|row| Caller {
        team: row.team_id,
        channel: row.channel_id,
        lead: row.lead_conversation,
        role: Role::Member,
        name: row.name,
    }))
}

/// Whether the lead of the ended `team` hasn't yet read its end note (the `ended` message): its next run is then
/// offered `read_reports` alone, which delivers it by the read rule, so a team that ended between the lead's runs is
/// still told once (ADR-0034 point 10).
///
/// # Errors
/// Storage failure.
pub(crate) async fn end_unread(
    connection: &mut PgConnection,
    team: Uuid,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
         WHERE t.id = $1 AND m.type = 'ended' AND m.to_kind = 'lead' AND m.seq > t.lead_last_read_seq) AS \"unread!\"",
        team
    )
    .fetch_one(connection)
    .await?)
}

/// The lead's `done`: in the caller's transaction, the summary stored as a `done` message to every member and the team
/// set `stopping` with `stop_reason` `done`, for the stop to carry out (ADR-0034 point 10). A team already stopping
/// for `done` answers as the first did; one stopping for another reason is `team_stopped`.
///
/// # Errors
/// `not_lead`, `409 team_stopped`, `429 channel_rate`, storage failure.
pub(crate) async fn done(
    connection: &mut PgConnection,
    caller: &Caller,
    author: Uuid,
    summary: &str,
) -> Result<Option<u64>, GatewayError> {
    roles::allowed(Some(caller.role), roles::Route::Done)?;
    let team = sqlx::query!(
        "SELECT state, stop_reason FROM agent_teams WHERE id = $1 FOR UPDATE",
        caller.team
    )
    .fetch_one(&mut *connection)
    .await?;
    match (team.state.as_str(), team.stop_reason.as_deref()) {
        ("running", _) => {}
        ("stopping", Some("done")) => return Ok(None),
        _ => return Err(GatewayError::Conflict("team_stopped")),
    }
    let message = Message {
        channel: caller.channel,
        author: Some((author, caller.name.as_str())),
        to: ("members", None),
    };
    let seq = insert(
        connection,
        &message,
        &TeamMessageBody::Done {
            text: summary.to_owned(),
        },
    )
    .await?;
    sqlx::query!(
        "UPDATE agent_teams SET state = 'stopping', stop_reason = 'done' WHERE id = $1",
        caller.team
    )
    .execute(&mut *connection)
    .await?;
    Ok(Some(seq))
}

/// A helper `stop_member` stopped: its name and task, and the notice's seq when this call stopped it.
pub(crate) struct Stopped {
    pub(crate) name: String,
    pub(crate) conversation: Uuid,
    pub(crate) notice: Option<u64>,
}

/// Who stopped a helper, which its `member_stopped` notice says: the lead weighs a person's Stop differently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stopper {
    /// The lead's `stop_member`.
    Lead,
    /// The person, pressing Stop on the helper's task (slice 6).
    Person,
}

/// Stops the team's helper `name`, in the caller's transaction, and tells the lead with a `member_stopped` notice saying
/// who stopped it; one already stopped answers the same with no second notice (ADR-0034, the S1 contract).
///
/// # Errors
/// `409 team_stopped` unless the team runs, `404 unknown_member`, `409 member_gone` for a helper that failed,
/// `429 channel_rate`, storage failure.
pub(crate) async fn stop_member(
    connection: &mut PgConnection,
    caller: &Caller,
    (name, by): (&str, Stopper),
) -> Result<Stopped, GatewayError> {
    let state = sqlx::query_scalar!(
        "SELECT state FROM agent_teams WHERE id = $1 FOR UPDATE",
        caller.team
    )
    .fetch_one(&mut *connection)
    .await?;
    if state != "running" {
        return Err(GatewayError::Conflict("team_stopped"));
    }
    let member = sqlx::query!(
        "SELECT conversation_id, name, state, failed_run FROM agent_team_members WHERE team_id = $1 \
         AND lower(name) = lower($2) FOR UPDATE",
        caller.team,
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::Missing("unknown_member"))?;
    match member.state.as_str() {
        "stopped" => {
            return Ok(Stopped {
                name: member.name,
                conversation: member.conversation_id,
                notice: None,
            });
        }
        // A done helper is stopped like any other, freeing its seat, and so is one whose run failed (it keeps its seat
        // until then); one that couldn't start has nothing to stop.
        "failed" if !member.failed_run => return Err(GatewayError::Conflict("member_gone")),
        _ => {}
    }
    sqlx::query!(
        "UPDATE agent_team_members SET state = 'stopped' WHERE conversation_id = $1",
        member.conversation_id
    )
    .execute(&mut *connection)
    .await?;
    let text = match by {
        Stopper::Lead => format!("{} was stopped. Its task stays.", member.name),
        // A person's Stop means "not this part": the lead doesn't hand the part to a new helper unless asked (the c16
        // live check, LC-08).
        Stopper::Person => format!(
            "{} was stopped by its person. Its task stays. Don't start another helper for its part unless your \
             person asks.",
            member.name
        ),
    };
    let seq = notice(
        connection,
        caller.channel,
        (NoticeKind::MemberStopped, &member.name),
        &text,
    )
    .await?;
    Ok(Stopped {
        name: member.name,
        conversation: member.conversation_id,
        notice: Some(seq),
    })
}

/// The task of the lead's helper `name`, in any state: `read_helper` reads a stopped helper's transcript too.
///
/// # Errors
/// `404 unknown_member`, storage failure.
pub(crate) async fn helper(
    connection: &mut PgConnection,
    team: Uuid,
    name: &str,
) -> Result<Uuid, GatewayError> {
    sqlx::query_scalar!(
        "SELECT conversation_id FROM agent_team_members WHERE team_id = $1 AND lower(name) = lower($2)",
        team,
        name
    )
    .fetch_optional(connection)
    .await?
    .ok_or(GatewayError::Missing("unknown_member"))
}

/// Stores `post` by the caller and returns its seq: in the caller's transaction, after its role, the team's state,
/// the addressee, the post's rules and the day's limit are checked, so a refusal stores nothing.
///
/// # Errors
/// `not_lead` / `members_only` (GW-U-069), `409 team_stopped`, `404 unknown_member`, `409 member_gone`,
/// `invalid_request` or `unknown_reference` (GW-U-125), `429 channel_rate`, storage failure.
pub(crate) async fn post(
    connection: &mut PgConnection,
    caller: &Caller,
    author: Uuid,
    (post, files): (&TeamPost, &[tqa_types::HandedFile]),
) -> Result<u64, GatewayError> {
    roles::allowed(Some(caller.role), roles::Route::Post(post.message_type()))?;
    let state = sqlx::query_scalar!(
        "SELECT state FROM agent_teams WHERE id = $1 FOR UPDATE",
        caller.team
    )
    .fetch_one(&mut *connection)
    .await?;
    if state != "running" {
        return Err(GatewayError::Conflict("team_stopped"));
    }
    let referenced = match post {
        TeamPost::Answer { question_seq, .. }
        | TeamPost::Decision {
            question_seq: Some(question_seq),
            ..
        } => Some((
            *question_seq,
            type_at(connection, caller.channel, *question_seq).await?,
        )),
        _ => None,
    };
    validate::validate_post(post, |seq| {
        referenced.and_then(|(referenced, kind)| if referenced == seq { kind } else { None })
    })?;
    let (to_kind, to_conversation) = match roles::addressee(post) {
        Addressee::Member(name) => (
            "member",
            Some(member_by_name(connection, caller.team, &name).await?),
        ),
        Addressee::Lead => ("lead", None),
        Addressee::Members => ("members", None),
        Addressee::Team => ("team", None),
        Addressee::Person => ("person", None),
    };
    let message = Message {
        channel: caller.channel,
        author: Some((author, &caller.name)),
        to: (to_kind, to_conversation),
    };
    insert(connection, &message, &body_of(post, files)).await
}

/// Where a message goes and who wrote it: a team conversation by its name, or the gateway (a `notice`).
struct Message<'a> {
    channel: Uuid,
    author: Option<(Uuid, &'a str)>,
    to: (&'static str, Option<Uuid>),
}

/// The gateway's report for `member` (`name`), to the lead (GW-I-311): written by the gateway, so the read and the
/// page say it's the gateway's, never the helper's words, while `take_files` finds the member it hands back for.
///
/// # Errors
/// The channel's day limit; a store failure.
pub(crate) async fn handed_back(
    connection: &mut PgConnection,
    channel: Uuid,
    (member, name): (Uuid, &str),
    body: &TeamMessageBody,
) -> Result<u64, GatewayError> {
    let message = Message {
        channel,
        author: None,
        to: ("lead", None),
    };
    insert_as(
        connection,
        &message,
        body,
        ("gateway", member.to_string(), name),
    )
    .await
}

/// Appends `body` to the team's log at its next seq, within the channel's day.
async fn insert(
    connection: &mut PgConnection,
    message: &Message<'_>,
    body: &TeamMessageBody,
) -> Result<u64, GatewayError> {
    let (kind, id, name) = match message.author {
        Some((conversation, name)) => ("conversation", conversation.to_string(), name),
        None => ("gateway", "gateway".to_owned(), "gateway"),
    };
    insert_as(connection, message, body, (kind, id, name)).await
}

/// [`insert`] with the author given: its kind, id and name.
async fn insert_as(
    connection: &mut PgConnection,
    message: &Message<'_>,
    body: &TeamMessageBody,
    (kind, id, name): (&str, String, &str),
) -> Result<u64, GatewayError> {
    crate::channels::store::within_the_day(connection, message.channel).await?;
    let json = serde_json::to_value(body).map_err(GatewayError::from)?;
    let seq = sqlx::query_scalar!(
        "UPDATE channels SET last_seq = last_seq + 1 WHERE id = $1 RETURNING last_seq",
        message.channel
    )
    .fetch_one(&mut *connection)
    .await?;
    sqlx::query!(
        "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, \
         to_conversation, body) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        message.channel,
        seq,
        kind,
        id,
        name,
        type_text(body.message_type()),
        message.to.0,
        message.to.1,
        json
    )
    .execute(&mut *connection)
    .await?;
    u64::try_from(seq).map_err(|_| GatewayError::StoreUnavailable)
}

/// Whether a read by `reader` under `key` would answer anything now: the answer already recorded under that key (a
/// repeat answers at once), or a message for it after its last-read seq and its outstanding answers. Records nothing,
/// so a wait can look before the read that records its answer (slice 3).
///
/// # Errors
/// Storage failure.
pub(crate) async fn has_new(
    connection: &mut PgConnection,
    caller: &Caller,
    (reader, key): (Uuid, &str),
) -> Result<bool, GatewayError> {
    let lead = caller.role == Role::Lead;
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM agent_team_reads WHERE reader = $4 AND call_key = $5) \
         OR EXISTS(SELECT 1 FROM channel_messages c WHERE c.channel_id = $1 \
         AND seq > GREATEST((SELECT CASE WHEN $3 THEN t.lead_last_read_seq ELSE m.last_read_seq END \
                             FROM agent_teams t LEFT JOIN agent_team_members m ON m.conversation_id = $4 \
                             WHERE t.id = $2), \
                            COALESCE((SELECT max(s) FROM agent_team_reads r, unnest(r.seqs) AS s WHERE r.reader = $4), 0)) \
         AND (($3 AND to_kind IN ('lead', 'team')) OR (NOT $3 AND ((to_kind = 'member' \
         AND to_conversation = $4) OR to_kind IN ('members', 'team')))) \
         AND NOT (c.type = 'person_message' AND c.body->>'member' = '' AND NOT EXISTS (SELECT 1 FROM idempotency i \
         WHERE i.team_copy_channel = c.channel_id AND i.team_copy_seq = c.seq \
         AND i.execution_status IN ('queued', 'delivered')))) AS \"new!\"",
        caller.channel,
        caller.team,
        lead,
        reader,
        key
    )
    .fetch_one(connection)
    .await?)
}

/// Why a waiting read should answer now rather than wait (the S1 contract's hints): `{"kind": "yield_files",
/// "member"}` for a lead whose `my_files` member waits for the lead's sandbox to hand over its files, `{"kind":
/// "yield_slot"}` when another task waits for a slot. Either way the tool tells the model to end its turn, so its
/// sandbox can go and the other can run (slice 3).
///
/// # Errors
/// Storage failure.
pub(crate) async fn hint(
    connection: &mut PgConnection,
    caller: &Caller,
    reader: Uuid,
) -> Result<Option<serde_json::Value>, GatewayError> {
    let row = sqlx::query!(
        "SELECT \
         (SELECT m.name FROM agent_team_members m JOIN idempotency i ON i.conversation_id = m.conversation_id \
                AND i.execution_status = 'queued' WHERE m.team_id = $1 AND m.workspace = 'my_files' \
                AND m.state NOT IN ('stopped', 'done') AND NOT (m.state = 'failed' AND NOT m.failed_run) \
                AND NOT EXISTS (SELECT 1 FROM sandbox_instances s WHERE s.conversation_id = m.conversation_id) \
                ORDER BY i.ordinal LIMIT 1) AS files, \
         EXISTS(SELECT 1 FROM idempotency i JOIN conversations c ON c.id = i.conversation_id \
                WHERE i.execution_status = 'queued' AND c.state = 'active' AND i.conversation_id <> $2 \
                AND NOT EXISTS (SELECT 1 FROM sandbox_instances s WHERE s.conversation_id = i.conversation_id)) \
           AS \"slot!\"",
        caller.team,
        reader
    )
    .fetch_one(connection)
    .await?;
    Ok(match row.files {
        Some(member) if caller.role == Role::Lead => {
            Some(serde_json::json!({"kind": "yield_files", "member": member}))
        }
        _ if row.slot => Some(serde_json::json!({"kind": "yield_slot"})),
        _ => None,
    })
}

/// Whether `conversation` was ever a member of an agent team, ended or not: such a task never leads (depth 1).
///
/// # Errors
/// Storage failure.
pub(crate) async fn was_member(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM agent_team_members WHERE conversation_id = $1) AS \"member!\"",
        conversation
    )
    .fetch_one(connection)
    .await?)
}

/// Whether `conversation` is or was an agent team's lead or member: its journal may hold its lead's brief or the
/// gateway's wakes. No work, and false, while agent teams are off and none was ever made.
///
/// # Errors
/// Storage failure.
pub(crate) async fn ever_in_team(
    state: &crate::app::State,
    conversation: &tqa_types::ConversationId,
) -> Result<bool, GatewayError> {
    let id = crate::conversations::uuid(conversation)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM agent_team_members WHERE conversation_id = $1) \
             OR EXISTS(SELECT 1 FROM agent_teams WHERE lead_conversation = $1) AS \"team!\"",
            id
        )
        .fetch_one(&state.pool)
        .await?)
    })
    .await
}

/// Stores the gateway's `notice` to the lead about `member` (the S1 contract's notice kinds).
///
/// # Errors
/// `429 channel_rate`, storage failure.
pub(crate) async fn notice(
    connection: &mut PgConnection,
    channel: Uuid,
    (kind, member): (tqa_types::NoticeKind, &str),
    text: &str,
) -> Result<u64, GatewayError> {
    let message = Message {
        channel,
        author: None,
        to: ("lead", None),
    };
    let body = TeamMessageBody::Notice {
        kind,
        member: Some(member.to_owned()),
        position: None,
        text: text.to_owned(),
    };
    insert(connection, &message, &body).await
}

/// A notice from the gateway to the lead about the whole team, with no member (`budget_80`).
///
/// # Errors
/// Storage failure.
pub(crate) async fn team_notice(
    connection: &mut PgConnection,
    channel: Uuid,
    kind: tqa_types::NoticeKind,
    text: &str,
) -> Result<u64, GatewayError> {
    let message = Message {
        channel,
        author: None,
        to: ("lead", None),
    };
    let body = TeamMessageBody::Notice {
        kind,
        member: None,
        position: None,
        text: text.to_owned(),
    };
    insert(connection, &message, &body).await
}

/// A notice from the gateway to one member about the whole team (`budget_80`, GW-I-304): it reads it as its lead's
/// messages are read, and is woken for it.
///
/// # Errors
/// Storage failure.
pub(crate) async fn member_notice(
    connection: &mut PgConnection,
    channel: Uuid,
    member: Uuid,
    kind: tqa_types::NoticeKind,
    text: &str,
) -> Result<u64, GatewayError> {
    let message = Message {
        channel,
        author: None,
        to: ("member", Some(member)),
    };
    let body = TeamMessageBody::Notice {
        kind,
        member: None,
        position: None,
        text: text.to_owned(),
    };
    insert(connection, &message, &body).await
}

/// Stores the gateway's end note to the lead (ADR-0034 point 10), in the transaction that ends the team.
///
/// # Errors
/// `429 channel_rate`, storage failure.
pub(crate) async fn end_note(
    connection: &mut PgConnection,
    channel: Uuid,
    body: &TeamMessageBody,
) -> Result<u64, GatewayError> {
    let message = Message {
        channel,
        author: None,
        to: ("lead", None),
    };
    insert(connection, &message, body).await
}

/// Stores the lead's brief as the team's next message, an `assign` to the new member (the S1 contract): the brief is
/// in the log like everything else, and it is the member's first message to read.
///
/// # Errors
/// `429 channel_rate`, storage failure.
pub(crate) async fn assign_brief(
    connection: &mut PgConnection,
    channel: Uuid,
    (leader, member): (Uuid, Uuid),
    brief: &tqa_types::Brief,
) -> Result<u64, GatewayError> {
    let message = Message {
        channel,
        author: Some((leader, "lead")),
        to: ("member", Some(member)),
    };
    let body = TeamMessageBody::Assign {
        text: brief.objective.clone(),
        brief: Some(brief.clone()),
    };
    insert(connection, &message, &body).await
}

/// The member a post names, by its name in any letter case: `unknown_member` when the team has none, `member_gone`
/// when it was stopped or couldn't start.
async fn member_by_name(
    connection: &mut PgConnection,
    team: Uuid,
    name: &str,
) -> Result<Uuid, GatewayError> {
    let member = sqlx::query!(
        "SELECT conversation_id, state, failed_run FROM agent_team_members WHERE team_id = $1 \
         AND lower(name) = lower($2)",
        team,
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::Missing("unknown_member"))?;
    // A helper whose run failed takes messages: the lead's next one starts it again (E2E-AGENTS-021).
    if member.state == "stopped" || (member.state == "failed" && !member.failed_run) {
        return Err(GatewayError::Conflict("member_gone"));
    }
    Ok(member.conversation_id)
}

/// The type of the team's message at `seq`, if there is one.
async fn type_at(
    connection: &mut PgConnection,
    channel: Uuid,
    seq: u64,
) -> Result<Option<TeamMessageType>, GatewayError> {
    let Ok(seq) = i64::try_from(seq) else {
        return Ok(None);
    };
    let kind = sqlx::query_scalar!(
        "SELECT type FROM channel_messages WHERE channel_id = $1 AND seq = $2",
        channel,
        seq
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(kind.and_then(|kind| type_of(&kind)))
}

/// A post as it is stored; a report lists the files read from the member's workspace (slice 5).
fn body_of(post: &TeamPost, files: &[tqa_types::HandedFile]) -> TeamMessageBody {
    match post.clone() {
        TeamPost::Assign { text, .. } => TeamMessageBody::Assign { text, brief: None },
        TeamPost::Question { text } => TeamMessageBody::Question { text },
        TeamPost::Decision { text, question_seq } => {
            TeamMessageBody::Decision { text, question_seq }
        }
        TeamPost::PersonQuestion { text } => TeamMessageBody::PersonQuestion { text },
        TeamPost::Answer { question_seq, text } => TeamMessageBody::Answer { text, question_seq },
        TeamPost::Report { text, .. } => TeamMessageBody::Report {
            text,
            files: files.to_vec(),
        },
        TeamPost::Ask { text } => TeamMessageBody::Ask { text },
    }
}

/// A read's answer: the messages, the last seq it covers, and whether more are waiting.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) messages: Vec<TeamMessage>,
    pub(crate) last_seq: u64,
    pub(crate) more: bool,
    /// The person's messages to the lead this read delivered (GW-I-307), by queue id, for its page's list once committed.
    pub(crate) sent: Vec<String>,
}

/// The caller's read under `key`: the recorded answer when there is one; otherwise the messages addressed to it after
/// its last-read seq and after every answer of its still outstanding, in order, at most `limit`, recorded for `run`.
/// The reader's row is locked first, so two reads at once take turns: neither answers the other's seqs (review H1),
/// and two with one key answer the same (M2). The seq doesn't move here (see [`result_stored`]).
///
/// # Errors
/// Storage failure.
pub(crate) async fn read(
    connection: &mut PgConnection,
    caller: &Caller,
    (reader, key, run): (Uuid, &str, &str),
    limit: u32,
) -> Result<Answer, GatewayError> {
    let last = lock_reader(connection, caller, reader).await?;
    let mut sent = Vec::new();
    let recorded = sqlx::query!(
        "SELECT seqs, more FROM agent_team_reads WHERE reader = $1 AND call_key = $2",
        reader,
        key
    )
    .fetch_optional(&mut *connection)
    .await?;
    // `start` is the seq the answer starts after: an empty answer reports it, so last_seq never goes behind an earlier
    // answer's (re-review N1).
    let (seqs, more, start) = if let Some(recorded) = recorded {
        (recorded.seqs, recorded.more, last)
    } else {
        let outstanding = sqlx::query_scalar!(
            "SELECT max(s) FROM agent_team_reads r, unnest(r.seqs) AS s WHERE r.reader = $1",
            reader
        )
        .fetch_one(&mut *connection)
        .await?;
        let after = outstanding.map_or(last, |outstanding| outstanding.max(last));
        let lead = caller.role == Role::Lead;
        let mut seqs = sqlx::query_scalar!(
            "SELECT seq FROM channel_messages c WHERE channel_id = $1 AND seq > $2 \
             AND (($3 AND to_kind IN ('lead', 'team')) OR (NOT $3 AND ((to_kind = 'member' \
             AND to_conversation = $4) OR to_kind IN ('members', 'team')))) \
             AND NOT (c.type = 'person_message' AND c.body->>'member' = '' AND NOT EXISTS (SELECT 1 FROM idempotency i \
             WHERE i.team_copy_channel = c.channel_id AND i.team_copy_seq = c.seq \
             AND i.execution_status IN ('queued', 'delivered'))) ORDER BY seq LIMIT $5",
            caller.channel,
            after,
            lead,
            reader,
            i64::from(limit) + 1
        )
        .fetch_all(&mut *connection)
        .await?;
        let more = seqs.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        seqs.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        sent = deliver(connection, caller.channel, &mut seqs).await?;
        sqlx::query!(
            "INSERT INTO agent_team_reads (reader, call_key, team_id, run_id, seqs, more) VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (reader, call_key) DO NOTHING",
            reader,
            key,
            caller.team,
            run,
            &seqs,
            more
        )
        .execute(&mut *connection)
        .await?;
        (seqs, more, after)
    };
    let messages = messages(connection, caller, &seqs).await?;
    let last_seq = seqs.iter().max().copied().unwrap_or(start);
    Ok(Answer {
        messages,
        last_seq: u64::try_from(last_seq).map_err(|_| GatewayError::StoreUnavailable)?,
        more,
        sent,
    })
}

/// The person's messages to the lead whose copies are among `seqs` are delivered: read by the lead, each starts no run
/// of its own and can't be cancelled (GW-I-307). A copy whose message went out as a run (or was cancelled or dropped)
/// in the meantime leaves `seqs`, so it is never read as well: once, never twice. Their queue ids.
async fn deliver(
    connection: &mut PgConnection,
    channel: Uuid,
    seqs: &mut Vec<i64>,
) -> Result<Vec<String>, GatewayError> {
    let sent = sqlx::query_scalar!(
        "UPDATE idempotency SET execution_status = 'delivered', updated_at = CURRENT_TIMESTAMP \
         WHERE team_copy_channel = $1 AND team_copy_seq = ANY($2) AND execution_status = 'queued' \
         RETURNING ordinal AS \"ordinal!\"",
        channel,
        &*seqs
    )
    .fetch_all(&mut *connection)
    .await?;
    let gone = sqlx::query_scalar!(
        "SELECT c.seq FROM channel_messages c WHERE c.channel_id = $1 AND c.seq = ANY($2) \
         AND c.type = 'person_message' AND c.body->>'member' = '' AND NOT EXISTS (SELECT 1 FROM idempotency i \
         WHERE i.team_copy_channel = c.channel_id AND i.team_copy_seq = c.seq AND i.execution_status = 'delivered')",
        channel,
        &*seqs
    )
    .fetch_all(&mut *connection)
    .await?;
    seqs.retain(|seq| !gone.contains(seq));
    Ok(sent.iter().map(ToString::to_string).collect())
}

/// Locks the reader's row (its member row, or its team's for the lead) for the transaction and returns its last-read
/// seq.
async fn lock_reader(
    connection: &mut PgConnection,
    caller: &Caller,
    reader: Uuid,
) -> Result<i64, GatewayError> {
    Ok(if caller.role == Role::Lead {
        sqlx::query_scalar!(
            "SELECT lead_last_read_seq FROM agent_teams WHERE id = $1 FOR UPDATE",
            caller.team
        )
        .fetch_one(&mut *connection)
        .await?
    } else {
        sqlx::query_scalar!(
            "SELECT last_read_seq FROM agent_team_members WHERE conversation_id = $1 FOR UPDATE",
            reader
        )
        .fetch_one(&mut *connection)
        .await?
    })
}

/// The conversation's current run: the run of its latest lifecycle event while that run hasn't ended.
///
/// # Errors
/// Storage failure.
pub(crate) async fn current_run(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<String>, GatewayError> {
    let latest = sqlx::query!(
        "SELECT run_id, lifecycle FROM events WHERE conversation_id = $1 AND lifecycle IS NOT NULL \
         ORDER BY seq DESC LIMIT 1",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(latest.and_then(|row| match row.lifecycle.as_deref() {
        Some("run.started" | "run.resumed") => row.run_id,
        _ => None,
    }))
}

/// The reads recorded under `keys` for `reader` have had their `tool.result`s stored, in the caller's transaction (the
/// one that stores the events): one statement takes them all, the reader's seq advances to their last, never back,
/// and the recorded answers at or below it go. `false`, with nothing changed, when none was recorded.
///
/// # Errors
/// Storage failure.
pub(crate) async fn result_stored(
    connection: &mut PgConnection,
    reader: Uuid,
    keys: &[String],
) -> Result<bool, GatewayError> {
    let rows = sqlx::query!(
        "DELETE FROM agent_team_reads WHERE reader = $1 AND call_key = ANY($2) RETURNING team_id, seqs",
        reader,
        keys
    )
    .fetch_all(&mut *connection)
    .await?;
    let Some(team) = rows.first().map(|row| row.team_id) else {
        return Ok(false);
    };
    let Some(last) = rows.iter().flat_map(|row| row.seqs.iter().copied()).max() else {
        return Ok(true);
    };
    let member = sqlx::query!(
        "UPDATE agent_team_members SET last_read_seq = GREATEST(last_read_seq, $2) WHERE conversation_id = $1",
        reader,
        last
    )
    .execute(&mut *connection)
    .await?;
    if member.rows_affected() == 0 {
        sqlx::query!(
            "UPDATE agent_teams SET lead_last_read_seq = GREATEST(lead_last_read_seq, $3) \
             WHERE id = $1 AND lead_conversation = $2",
            team,
            reader,
            last
        )
        .execute(&mut *connection)
        .await?;
    }
    // An answer is pruned once its last seq is read; an empty answer goes only with its own result or its run.
    sqlx::query!(
        "DELETE FROM agent_team_reads WHERE reader = $1 AND (SELECT max(s) FROM unnest(seqs) AS s) <= $2",
        reader,
        last
    )
    .execute(&mut *connection)
    .await?;
    Ok(true)
}

/// `run` has ended: the reader's remaining recorded answers from it go, and its seq stays (GW-U-126).
///
/// # Errors
/// Storage failure.
pub(crate) async fn run_ended(
    connection: &mut PgConnection,
    reader: Uuid,
    run: &str,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM agent_team_reads WHERE reader = $1 AND run_id = $2",
        reader,
        run
    )
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) const fn type_text(kind: TeamMessageType) -> &'static str {
    match kind {
        TeamMessageType::Assign => "assign",
        TeamMessageType::Question => "question",
        TeamMessageType::Answer => "answer",
        TeamMessageType::Report => "report",
        TeamMessageType::Ask => "ask",
        TeamMessageType::Decision => "decision",
        TeamMessageType::PersonQuestion => "person_question",
        TeamMessageType::PersonAnswer => "person_answer",
        TeamMessageType::PersonMessage => "person_message",
        TeamMessageType::Notice => "notice",
        TeamMessageType::Done => "done",
        TeamMessageType::Ended => "ended",
    }
}

fn type_of(text: &str) -> Option<TeamMessageType> {
    TeamMessageType::ALL
        .into_iter()
        .find(|kind| type_text(*kind) == text)
}

/// The names of the team's members still taking messages, in the order they were started (an `ask_all`'s answer).
///
/// # Errors
/// Storage failure.
pub(crate) async fn live_members(
    connection: &mut PgConnection,
    team: Uuid,
) -> Result<Vec<String>, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT name FROM agent_team_members WHERE team_id = $1 AND state <> 'stopped' \
         AND NOT (state = 'failed' AND NOT failed_run) ORDER BY created_at, name",
        team
    )
    .fetch_all(connection)
    .await?)
}
