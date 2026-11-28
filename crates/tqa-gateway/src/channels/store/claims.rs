//! Claims on tasks, up to each task's limit, and the task board (ADR-0014 point 8).
use super::{append_body, db_seq, kind_of, kind_text, read, seq_of};
use crate::GatewayError;
use sqlx::PgConnection;
use tqa_types::{
    Author, AuthorKind, ChannelFilter, ChannelMessage, ChannelNoticeKind, MessageBody,
};
use uuid::Uuid;

/// What a claim did: a new claim, or the claimant's existing one (a retry).
#[derive(Debug)]
pub(crate) enum Claimed {
    New(ChannelMessage),
    Held(ChannelMessage),
}

/// Claims task `task` for `author`, atomically up to the task's limit (ADR-0014 point 8): the channel row is locked
/// as for a post, so claims and posts to one channel are serialized. A claimant who holds a claim gets it back. Runs
/// in the caller's transaction.
///
/// # Errors
/// `404` (the channel or the seq), `409 channel_ended`, `429 channel_rate`, `not_a_task`, `one_at_a_time` (an agent
/// holding an unreported claim on another to-do) or `already_claimed`, storage failure.
pub(crate) async fn claim(
    connection: &mut PgConnection,
    channel: Uuid,
    task: u64,
    author: &Author,
) -> Result<Claimed, GatewayError> {
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
    let task_row = sqlx::query!(
        "SELECT type, body FROM channel_messages WHERE channel_id = $1 AND seq = $2",
        channel,
        db_seq(task)?
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    let MessageBody::Task { claims, .. } =
        serde_json::from_value(task_row.body).map_err(|_| GatewayError::StoreUnavailable)?
    else {
        return Err(GatewayError::Conflict("not_a_task"));
    };
    let held = sqlx::query_scalar!(
        "SELECT claim_seq FROM channel_claims WHERE channel_id = $1 AND task_seq = $2 AND claimant_kind = $3 \
         AND claimant_id = $4",
        channel,
        db_seq(task)?,
        kind_text(author.kind),
        author.id
    )
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(seq) = held {
        return message(connection, channel, seq_of(seq)?)
            .await
            .map(Claimed::Held);
    }
    one_at_a_time(connection, channel, task, author).await?;
    let counts = sqlx::query!(
        "SELECT (SELECT count(*) FROM channel_claims WHERE channel_id = $1 AND task_seq = $2) AS \"claimed!\", \
         (SELECT count(*) FROM channel_members WHERE channel_id = $1 AND role IN ('lead', 'member')) AS \"voting!\"",
        channel,
        db_seq(task)?
    )
    .fetch_one(&mut *connection)
    .await?;
    let limit = claims.of(u32::try_from(counts.voting).unwrap_or(u32::MAX));
    if counts.claimed >= i64::from(limit) {
        return Err(GatewayError::Conflict("already_claimed"));
    }
    let stored = append_body(
        connection,
        channel,
        author,
        (None, &MessageBody::Claim { task_seq: task }),
    )
    .await?;
    sqlx::query!(
        "INSERT INTO channel_claims (channel_id, task_seq, claimant_kind, claimant_id, claim_seq) VALUES ($1, $2, $3, $4, $5)",
        channel,
        db_seq(task)?,
        kind_text(author.kind),
        author.id,
        db_seq(stored.seq)?
    )
    .execute(connection)
    .await?;
    Ok(Claimed::New(stored))
}

/// Gives back `author`'s place on task `task` before they report on it (18.3.0, GW-I-401): the claim goes, the place is
/// free again, and a gateway notice says who gave it back and wakes the channel's agents as a new to-do does
/// (`wake::mark`). Locked as a claim is. Runs in the caller's transaction; the notice is returned.
///
/// # Errors
/// `404` (the channel or the seq), `409 channel_ended`, `not_a_task` or `already_reported`, `403 not_claimant` (no
/// place held), `429 channel_rate`, storage failure.
pub(crate) async fn give_back(
    connection: &mut PgConnection,
    channel: Uuid,
    task: u64,
    (author, name): (&Author, &str),
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
    // A give-back writes a message, as a claim does: within the channel's day (ADR-0014).
    super::within_the_day(connection, channel).await?;
    let kind = sqlx::query_scalar!(
        "SELECT type FROM channel_messages WHERE channel_id = $1 AND seq = $2",
        channel,
        db_seq(task)?
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if kind != "task" {
        return Err(GatewayError::Conflict("not_a_task"));
    }
    let reported = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM channel_messages WHERE channel_id = $1 AND task_seq = $2 AND type = 'report' \
         AND author_kind = $3 AND author_id = $4) AS \"reported!\"",
        channel,
        db_seq(task)?,
        kind_text(author.kind),
        author.id
    )
    .fetch_one(&mut *connection)
    .await?;
    if reported {
        return Err(GatewayError::Conflict("already_reported"));
    }
    let removed = sqlx::query!(
        "DELETE FROM channel_claims WHERE channel_id = $1 AND task_seq = $2 AND claimant_kind = $3 AND claimant_id = $4",
        channel,
        db_seq(task)?,
        kind_text(author.kind),
        author.id
    )
    .execute(&mut *connection)
    .await?;
    if removed.rows_affected() == 0 {
        return Err(GatewayError::Denied("not_claimant"));
    }
    append_body(
        connection,
        channel,
        &super::gateway_author(),
        (
            None,
            &MessageBody::Notice {
                kind: ChannelNoticeKind::PlaceGivenBack,
                member: Some(name.to_owned()),
                // The person's id, so the board's replay of the log removes this place (`tasks`).
                member_id: Some(author.id.clone()),
                code: None,
                refusal: None,
                resets_at: None,
                task_seq: Some(task),
                text: format!("{name} gave back their place on to-do #{task}; it is free again."),
            },
        ),
    )
    .await
}

/// An agent holds one open to-do at a time in a channel (ADR-0014's amendment, A13): a claim it has on another to-do
/// there, with no report of its own on it whatever the report's status, refuses a new one. People are not limited.
///
/// # Errors
/// `409 one_at_a_time`, storage failure.
async fn one_at_a_time(
    connection: &mut PgConnection,
    channel: Uuid,
    task: u64,
    author: &Author,
) -> Result<(), GatewayError> {
    if author.kind != AuthorKind::Conversation {
        return Ok(());
    }
    if let Some(open) = sqlx::query_scalar!(
            "SELECT c.task_seq FROM channel_claims c WHERE c.channel_id = $1 AND c.claimant_kind = 'conversation' \
             AND c.claimant_id = $2 AND c.task_seq <> $3 AND NOT EXISTS (SELECT 1 FROM channel_messages r \
             WHERE r.channel_id = c.channel_id AND r.type = 'report' AND r.task_seq = c.task_seq \
             AND r.author_kind = 'conversation' AND r.author_id = c.claimant_id) ORDER BY c.task_seq LIMIT 1",
            channel,
            author.id,
            db_seq(task)?
        )
        .fetch_optional(&mut *connection)
        .await?
    {
        let open = seq_of(open)?;
        return Err(GatewayError::ConflictSaid {
            code: "one_at_a_time",
            said: format!(
                "You hold to-do {open} in this channel and have not reported on it: post your report on it \
                 (channel_post, type report, task_seq {open}), then claim another."
            ),
        });
    }
    Ok(())
}

/// One message by seq.
///
/// # Errors
/// `404`, storage failure.
pub(crate) async fn message(
    connection: &mut PgConnection,
    channel: Uuid,
    seq: u64,
) -> Result<ChannelMessage, GatewayError> {
    let page = read(
        connection,
        channel,
        (seq.saturating_sub(1), &ChannelFilter::default(), 1),
        true,
    )
    .await?;
    page.messages
        .into_iter()
        .find(|message| message.seq == seq)
        .ok_or(GatewayError::NotFound)
}

/// A task on the board: its title and resolved limit, its claims and its reports, in seq order.
#[derive(Debug)]
pub(crate) struct TaskState {
    pub task_seq: u64,
    pub title: String,
    pub limit: u32,
    pub claims: Vec<(Author, u64)>,
    pub reports: Vec<(Author, u64, tqa_types::ReportStatus)>,
}

/// The channel's tasks (at most 500, oldest first) with their claims and reports.
///
/// # Errors
/// Storage failure.
pub(crate) async fn tasks(
    connection: &mut PgConnection,
    channel: Uuid,
) -> Result<Vec<TaskState>, GatewayError> {
    let voting = sqlx::query_scalar!(
        "SELECT count(*) AS \"voting!\" FROM channel_members WHERE channel_id = $1 AND role IN ('lead', 'member')",
        channel
    )
    .fetch_one(&mut *connection)
    .await?;
    let voting = u32::try_from(voting).unwrap_or(u32::MAX);
    let rows = sqlx::query!(
        "SELECT seq, type, task_seq, author_kind, author_id, author_name, body FROM channel_messages \
         WHERE channel_id = $1 AND (type IN ('task', 'claim', 'report') OR (type = 'notice' AND task_seq IS NOT NULL)) \
         ORDER BY seq LIMIT 5000",
        channel
    )
    .fetch_all(connection)
    .await?;
    let mut tasks: Vec<TaskState> = Vec::new();
    for row in rows {
        let seq = seq_of(row.seq)?;
        let author = Author {
            kind: kind_of(&row.author_kind)?,
            id: row.author_id,
            name: row.author_name,
        };
        let body: MessageBody =
            serde_json::from_value(row.body).map_err(|_| GatewayError::StoreUnavailable)?;
        match body {
            MessageBody::Task { title, claims, .. } if tasks.len() < 500 => tasks.push(TaskState {
                task_seq: seq,
                title,
                limit: claims.of(voting),
                claims: Vec::new(),
                reports: Vec::new(),
            }),
            MessageBody::Claim { task_seq } => {
                if let Some(task) = tasks.iter_mut().find(|task| task.task_seq == task_seq) {
                    task.claims.push((author, seq));
                }
            }
            MessageBody::Report {
                task_seq, status, ..
            } => {
                if let Some(task) = tasks.iter_mut().find(|task| task.task_seq == task_seq) {
                    task.reports.push((author, seq, status));
                }
            }
            // A place given back (GW-I-401): that person's claim goes, so the place is free again.
            MessageBody::Notice {
                kind: ChannelNoticeKind::PlaceGivenBack,
                task_seq: Some(task_seq),
                member_id: Some(person),
                ..
            } => given_back(&mut tasks, task_seq, &person),
            _ => {}
        }
    }
    Ok(tasks)
}

/// A place given back (GW-I-401): `person`'s claim on to-do `task_seq` leaves the board.
fn given_back(tasks: &mut [TaskState], task_seq: u64, person: &str) {
    if let Some(task) = tasks.iter_mut().find(|task| task.task_seq == task_seq) {
        task.claims
            .retain(|(claimant, _)| !(claimant.kind == AuthorKind::User && claimant.id == person));
    }
}
