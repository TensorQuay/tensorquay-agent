//! The people's reads of an agent team (ADR-0034, the slice S6 contract): `GET …/team` and `GET …/team/messages`, by
//! the lead's task or any member's. The team is its lead task's person's: they read everything, an ended team too;
//! anyone else, an administrator included, gets the task's own refusal, `403` (SEC-005: a team's names, briefs and
//! messages are content).
use serde::Serialize;
use serde_json::Value;
use sqlx::PgConnection;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use super::{
    roles::Role,
    store::{self, Caller},
};
use crate::{GatewayError, app::State, auth::User};
use tqa_types::ConversationId;

/// The longest `last_message` text, in characters.
const LAST_MESSAGE_CHARS: usize = 300;

/// The team a task is in, as its person reads it.
pub(super) struct Found {
    pub(super) id: Uuid,
    pub(super) channel: Uuid,
    pub(super) lead: Uuid,
    state: String,
    stop_reason: Option<String>,
    started_at: OffsetDateTime,
    deadline_at: OffsetDateTime,
    token_budget: i64,
    spent_tokens: i64,
    /// The lead's read place, as its wake reads from it: its finished runs' place, or what a run still going read.
    /// The same SQL as `wake::team`'s `placed.after` for the lead: a change to one is a change to both (GW-I-319).
    lead_read_seq: i64,
    /// The team's messages after that place that are for the lead (to it or to the whole team), never its own, nor
    /// the person's message withdrawn before it reached the lead (as `wake::team` leaves it out).
    lead_unread: i64,
}

/// The team of task `id` for `user`, its person: in any state.
///
/// # Errors
/// `403` for another person's task, an administrator's included, `404` for one that doesn't exist, `404 no_team` for
/// one in no team; store failure.
pub(super) async fn found(
    state: &State,
    user: &User,
    id: &ConversationId,
) -> Result<Found, GatewayError> {
    // As the task answers its own person; anyone else, an administrator included, gets its refusal (403).
    crate::conversations::authorize(state, user, id, false).await?;
    let task = crate::conversations::uuid(id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_as!(
            Found,
            "SELECT t.id, t.channel_id AS channel, t.lead_conversation AS lead, t.state, t.stop_reason, \
             t.created_at AS started_at, t.deadline_at, t.token_budget, t.spent_tokens, \
             r.place AS \"lead_read_seq!\", \
             (SELECT count(*) FROM channel_messages c WHERE c.channel_id = t.channel_id AND c.seq > r.place \
              AND c.to_kind IN ('lead', 'team') \
              AND NOT (c.author_kind = 'conversation' AND c.author_id = t.lead_conversation::text) \
              AND NOT (c.type = 'person_message' AND c.body->>'member' = '' AND NOT EXISTS (SELECT 1 \
              FROM idempotency i WHERE i.team_copy_channel = c.channel_id AND i.team_copy_seq = c.seq \
              AND i.execution_status IN ('queued', 'delivered')))) \
              AS \"lead_unread!\" \
             FROM agent_teams t CROSS JOIN LATERAL (SELECT GREATEST(t.lead_last_read_seq, \
              COALESCE((SELECT max(s) FROM agent_team_reads a, unnest(a.seqs) AS s \
              WHERE a.reader = t.lead_conversation), 0)) AS place) r \
             WHERE t.lead_conversation = $1 OR EXISTS (SELECT 1 FROM agent_team_members m \
             WHERE m.team_id = t.id AND m.conversation_id = $1) \
             AND EXISTS (SELECT 1 FROM conversations l WHERE l.id = t.lead_conversation AND l.state <> 'deleted')",
            task
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?
    .ok_or(GatewayError::Missing("no_team"))
}

#[derive(Serialize)]
pub(super) struct Team {
    id: Uuid,
    state: String,
    stop_reason: Option<String>,
    lead: LeadId,
    started_at: String,
    deadline_at: String,
    budget: Budget,
    waiting_for_you: Option<Question>,
    helpers: Vec<Helper>,
    version: u64,
}

#[derive(Serialize)]
struct LeadId {
    id: Uuid,
    /// Where the lead has read to: the place its wake reads from (`teams::wake`'s `after`).
    read_seq: i64,
    /// The team's messages after `read_seq` that are for the lead (GW-I-319).
    unread: i64,
}

#[derive(Serialize)]
struct Budget {
    tokens: i64,
    spent: i64,
}

#[derive(Serialize)]
struct Question {
    seq: i64,
    text: String,
    at: String,
}

#[derive(Serialize)]
struct Helper {
    conversation: Uuid,
    name: String,
    role: String,
    workspace: String,
    state: &'static str,
    /// Its person's Stop helper would stop it now (`place_shape::can_stop`).
    can_stop: bool,
    position: Option<u32>,
    tokens: i64,
    /// Part of `tokens` is the relay's estimate: a step cancelled before its server reported usage.
    tokens_estimated: bool,
    activity: Option<Activity>,
    last_message: Option<LastMessage>,
}

/// A helper's current step and its open tool call; the transcript route (`read_helper`) reads it too.
#[derive(Serialize)]
pub(super) struct Activity {
    pub(super) step: i64,
    pub(super) tool: Option<String>,
    pub(super) since: String,
    /// When the step, or its open call, began.
    #[serde(skip)]
    pub(super) began: OffsetDateTime,
    /// The run it is in, and the seq of its step's start.
    #[serde(skip)]
    pub(super) run: String,
    #[serde(skip)]
    pub(super) from_seq: i64,
}

#[derive(Serialize)]
struct LastMessage {
    seq: i64,
    #[serde(rename = "type")]
    kind: String,
    text: String,
    at: String,
}

fn rfc3339(at: OffsetDateTime) -> Result<String, GatewayError> {
    at.format(&Rfc3339)
        .map_err(|_| GatewayError::StoreUnavailable)
}

/// `GET …/team`: the team as its person reads it.
///
/// # Errors
/// As [`found`]; store failure.
pub(super) async fn team(
    state: &State,
    user: &User,
    id: &ConversationId,
) -> Result<Team, GatewayError> {
    let found = found(state, user, id).await?;
    let (question, rows) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        read_team(&mut connection, &found).await
    })
    .await?;
    let running = found.state == "running";
    let mut helpers = Vec::with_capacity(rows.len());
    for row in rows {
        let state_word = super::place_shape::helper_state(&row.state);
        let position = if state_word == "queued" {
            queue_position(state, row.conversation).await
        } else {
            None
        };
        helpers.push(Helper {
            conversation: row.conversation,
            name: row.name,
            role: row.role,
            workspace: row.workspace,
            state: state_word,
            can_stop: super::place_shape::can_stop(running, &row.state, row.failed_run),
            position,
            tokens: row.tokens,
            tokens_estimated: row.tokens_estimated,
            activity: row.activity,
            last_message: row.last_message,
        });
    }
    Ok(Team {
        id: found.id,
        state: found.state,
        stop_reason: found.stop_reason,
        lead: LeadId {
            id: found.lead,
            read_seq: found.lead_read_seq,
            unread: found.lead_unread,
        },
        started_at: rfc3339(found.started_at)?,
        deadline_at: rfc3339(found.deadline_at)?,
        budget: Budget {
            tokens: found.token_budget,
            spent: found.spent_tokens,
        },
        // A question is open only while the team runs: once it stops, an answer is refused (`team_stopped`).
        waiting_for_you: question.filter(|_| running),
        helpers,
        version: state.team_changes.version(found.id),
    })
}

/// A helper as stored, with its activity and last message.
struct Stored {
    conversation: Uuid,
    name: String,
    role: String,
    workspace: String,
    state: String,
    failed_run: bool,
    tokens: i64,
    tokens_estimated: bool,
    activity: Option<Activity>,
    last_message: Option<LastMessage>,
}

/// The open question and the helpers, deleted ones left out, in the order they joined.
async fn read_team(
    connection: &mut PgConnection,
    found: &Found,
) -> Result<(Option<Question>, Vec<Stored>), GatewayError> {
    let question = sqlx::query!(
            "SELECT q.seq, q.body->>'text' AS \"text!\", q.created_at FROM channel_messages q \
             WHERE q.channel_id = $1 AND q.type = 'person_question' AND NOT EXISTS (SELECT 1 FROM channel_messages a \
             WHERE a.channel_id = q.channel_id AND a.type = 'person_answer' AND a.body->>'question_seq' = q.seq::text) \
             ORDER BY q.seq DESC LIMIT 1",
            found.channel
        )
        .fetch_optional(&mut *connection)
        .await?;
    let rows = sqlx::query!(
            "SELECT m.conversation_id, m.name, m.role, m.workspace, m.state, m.failed_run, \
             (SELECT COALESCE(sum(COALESCE(u.prompt_tokens, 0) + COALESCE(u.completion_tokens, 0)), 0) FROM usage u \
              WHERE u.conversation_id = m.conversation_id AND u.created_at >= $2)::bigint AS \"tokens!\", \
             EXISTS (SELECT 1 FROM usage u WHERE u.conversation_id = m.conversation_id AND u.created_at >= $2 \
              AND u.estimated) AS \"tokens_estimated!\" \
             FROM agent_team_members m JOIN conversations c ON c.id = m.conversation_id \
             WHERE m.team_id = $1 AND c.state <> 'deleted' ORDER BY m.created_at",
            found.id,
            found.started_at
        )
        .fetch_all(&mut *connection)
        .await?;
    let mut helpers = Vec::with_capacity(rows.len());
    for row in rows {
        helpers.push(Stored {
            activity: activity(connection, row.conversation_id).await?,
            last_message: last_message(connection, found.channel, row.conversation_id).await?,
            conversation: row.conversation_id,
            name: row.name,
            role: row.role,
            workspace: row.workspace,
            state: row.state,
            failed_run: row.failed_run,
            tokens: row.tokens,
            tokens_estimated: row.tokens_estimated,
        });
    }
    let question = question
        .map(|row| {
            Ok::<_, GatewayError>(Question {
                seq: row.seq,
                text: row.text,
                at: rfc3339(row.created_at)?,
            })
        })
        .transpose()?;
    Ok((question, helpers))
}

/// A queued helper's place in the slot queue, counted from 1; none when it isn't waiting for a slot.
async fn queue_position(state: &State, conversation: Uuid) -> Option<u32> {
    let id: ConversationId = conversation.to_string().parse().ok()?;
    let position = state.manager.queue_position(&id).await.ok()??;
    super::place_shape::slot_place(position)
}

/// The helper's current step and its open tool call, from its run's events; none while no run is active.
pub(super) async fn activity(
    connection: &mut PgConnection,
    helper: Uuid,
) -> Result<Option<Activity>, GatewayError> {
    let Some(run) = store::current_run(connection, helper).await? else {
        return Ok(None);
    };
    let Some(step) = sqlx::query!(
        "SELECT seq, step AS \"step!\", ts FROM events WHERE conversation_id = $1 AND run_id = $2 \
         AND step IS NOT NULL AND frame->'event'->>'type' = 'step.started' ORDER BY seq DESC LIMIT 1",
        helper,
        run
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    // A call started and not ended in the step, named by its tool.call; none while the model thinks or writes.
    let call = sqlx::query!(
        "SELECT c.frame->'event'->>'name' AS name, s.ts FROM events s JOIN events c ON c.conversation_id = s.conversation_id \
         AND c.run_id = s.run_id AND c.frame->'event'->>'type' = 'tool.call' \
         AND c.frame->'event'->>'call_id' = s.frame->'event'->>'call_id' \
         WHERE s.conversation_id = $1 AND s.run_id = $2 AND s.step = $3 AND s.frame->'event'->>'type' = 'tool.start' \
         AND NOT EXISTS (SELECT 1 FROM events e WHERE e.conversation_id = s.conversation_id AND e.run_id = s.run_id \
         AND e.frame->'event'->>'type' = 'tool.end' AND e.frame->'event'->>'call_id' = s.frame->'event'->>'call_id') \
         ORDER BY s.seq DESC LIMIT 1",
        helper,
        run,
        step.step
    )
    .fetch_optional(&mut *connection)
    .await?;
    let (tool, since) = match call {
        Some(call) => (call.name, call.ts),
        None => (None, step.ts),
    };
    Ok(Some(Activity {
        step: step.step,
        tool,
        since: rfc3339(since)?,
        began: since,
        run,
        from_seq: step.seq,
    }))
}

/// The helper's latest message to the lead, its text cut to 300 characters.
async fn last_message(
    connection: &mut PgConnection,
    channel: Uuid,
    helper: Uuid,
) -> Result<Option<LastMessage>, GatewayError> {
    let row = sqlx::query!(
        "SELECT seq, type AS kind, body->>'text' AS text, created_at FROM channel_messages \
         WHERE channel_id = $1 AND author_kind = 'conversation' AND author_id = $2 AND to_kind = 'lead' \
         ORDER BY seq DESC LIMIT 1",
        channel,
        helper.to_string()
    )
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| {
        Ok(LastMessage {
            seq: row.seq,
            kind: row.kind,
            text: row
                .text
                .unwrap_or_default()
                .chars()
                .take(LAST_MESSAGE_CHARS)
                .collect(),
            at: rfc3339(row.created_at)?,
        })
    })
    .transpose()
}

/// `GET …/team/messages`: every message of the team after `after`, at most `limit`, in the S1 read shape; a person's
/// own messages carry `by`.
///
/// # Errors
/// As [`found`]; store failure.
pub(super) async fn messages(
    state: &State,
    user: &User,
    id: &ConversationId,
    (after, limit): (u64, u32),
) -> Result<Value, GatewayError> {
    let found = found(state, user, id).await?;
    let caller = Caller {
        team: found.id,
        channel: found.channel,
        lead: found.lead,
        role: Role::Lead,
        name: "lead".to_owned(),
    };
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let after = i64::try_from(after).unwrap_or(i64::MAX);
        let mut seqs: Vec<i64> = sqlx::query_scalar!(
            "SELECT seq FROM channel_messages c WHERE channel_id = $1 AND seq > $2 \
             AND NOT (c.type = 'person_message' AND c.body->>'member' = '' AND NOT EXISTS (SELECT 1 FROM idempotency i \
             WHERE i.team_copy_channel = c.channel_id AND i.team_copy_seq = c.seq \
             AND i.execution_status IN ('queued', 'delivered'))) ORDER BY seq LIMIT $3",
            found.channel,
            after,
            i64::from(limit) + 1
        )
        .fetch_all(&mut *connection)
        .await?;
        let more = seqs.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        seqs.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        let messages = store::messages(&mut connection, &caller, &seqs).await?;
        let people: std::collections::HashMap<i64, String> = sqlx::query!(
            "SELECT seq, author_id FROM channel_messages WHERE channel_id = $1 AND seq = ANY($2) AND author_kind = 'user'",
            found.channel,
            &seqs
        )
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .map(|row| (row.seq, row.author_id))
        .collect();
        let mut items = Vec::with_capacity(messages.len());
        for message in messages {
            let seq = i64::try_from(message.seq).unwrap_or(i64::MAX);
            let mut item = serde_json::to_value(&message)?;
            if let Some(by) = people.get(&seq) {
                item["by"] = Value::String(by.clone());
            }
            items.push(item);
        }
        let last_seq = seqs.last().copied().unwrap_or(after);
        Ok(serde_json::json!({ "items": items, "last_seq": last_seq, "more": more }))
    })
    .await
}
