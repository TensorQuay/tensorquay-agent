//! Claims on tasks, up to each task's limit, and the task board (ADR-0014 point 8).
use super::{append_body, db_seq, kind_of, kind_text, read, seq_of};
use crate::GatewayError;
use sqlx::PgConnection;
use tqa_types::{Author, ChannelFilter, ChannelMessage, MessageBody};
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
/// `404` (the channel or the seq), `409 channel_ended`, `429 channel_rate`, `not_a_task` or `already_claimed`, storage failure.
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
         WHERE channel_id = $1 AND type IN ('task', 'claim', 'report') ORDER BY seq LIMIT 5000",
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
            _ => {}
        }
    }
    Ok(tasks)
}
