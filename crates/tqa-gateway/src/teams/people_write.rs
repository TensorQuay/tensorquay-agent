//! The people's writes to an agent team (ADR-0034, the slice S6 contract): answering the lead's question, Stop on the
//! lead's task or a member's, and the rule for deleting a lead with helpers. Every stop goes through the one stop path
//! (`teams::stop`): these routes record the person's stop and add no stop logic of their own.
use std::sync::Arc;

use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgConnection;
use tqa_types::{ConversationId, TeamMessageBody, TeamMessageType};
use uuid::Uuid;

use super::people_read::{Found, found};
use super::roles::Role;
use super::store::{self, Caller};
use crate::{
    GatewayError, InvalidReason, app::State, auth::User, conversations::idempotent::Keyed,
};

/// How the lead's reads name the person: the lead acts for one person, so its model needs no personal data (ADR-0034
/// S6); the people's read names them by their id, which the page shows as "You".
const PERSON: &str = "your person";

/// `POST …/team/answers`: the person answers the lead's open question.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Answer {
    question_seq: u64,
    text: String,
}

/// Stores the answer as a `person_answer` to the lead and wakes it (ADR-0034 point 4); answers `{seq}`. A retry with
/// the same key answers as the first did.
///
/// # Errors
/// `400` naming `text` (empty, over 16,000 bytes, NUL) or `question_seq` (0); as [`found`]; `409 team_stopped`
/// unless the team runs; `422 unknown_reference` for a seq that isn't this team's `person_question`; `409
/// already_answered`; `429 channel_rate`; a missing or reused key; store failure.
pub(super) async fn answer(
    state: &Arc<State>,
    user: &User,
    id: &ConversationId,
    (headers, answer): (&HeaderMap, &Answer),
) -> Result<Value, GatewayError> {
    super::people_rules::answer(answer.question_seq, &answer.text)?;
    let team = found(state, user, id).await?;
    let keyed = Keyed::new(
        user.id,
        headers,
        format!("POST /api/v1/conversations/{id}/team/answers"),
        answer,
    )?;
    let (body, seq) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some((_, cached)) = keyed.begin(state, &mut transaction).await? {
            return Ok((cached, None));
        }
        running(&mut transaction, team.id).await?;
        let question = i64::try_from(answer.question_seq).map_err(|_| invalid("question_seq"))?;
        let row = sqlx::query!(
            "SELECT EXISTS (SELECT 1 FROM channel_messages WHERE channel_id = $1 AND seq = $2 \
             AND type = 'person_question') AS \"asked!\", EXISTS (SELECT 1 FROM channel_messages \
             WHERE channel_id = $1 AND type = 'person_answer' AND body->>'question_seq' = $2::text) AS \"answered!\"",
            team.channel,
            question
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !row.asked {
            return Err(GatewayError::UnknownReference {
                field: "question_seq",
            });
        }
        if row.answered {
            return Err(GatewayError::Conflict("already_answered"));
        }
        let body = TeamMessageBody::PersonAnswer {
            text: answer.text.clone(),
            question_seq: answer.question_seq,
            by: PERSON.to_owned(),
        };
        let seq = person_message(&mut transaction, team.channel, user, &body).await?;
        let answered = json!({ "seq": seq });
        keyed
            .record(&mut transaction, StatusCode::CREATED, &answered)
            .await?;
        transaction.commit().await?;
        Ok((answered, Some(seq)))
    })
    .await?;
    if let Some(seq) = seq {
        state.channels.published(team.channel, seq);
        super::wake::soon(state, team.id);
    }
    Ok(body)
}

/// `POST …/team/stop`: on the lead's task the whole team, on a member's that helper; a repeat answers the same.
///
/// # Errors
/// As [`found`]; `409 team_stopped` for a team that has ended (for a helper, unless the team runs); `409
/// member_gone` for a helper that failed; a missing or reused key; store failure.
pub(super) async fn stop(
    state: &Arc<State>,
    user: &User,
    id: &ConversationId,
    headers: &HeaderMap,
) -> Result<Value, GatewayError> {
    let team = found(state, user, id).await?;
    let task = crate::conversations::uuid(id)?;
    let keyed = Keyed::new(
        user.id,
        headers,
        format!("POST /api/v1/conversations/{id}/team/stop"),
        &json!({}),
    )?;
    let (body, after) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some((_, cached)) = keyed.begin(state, &mut transaction).await? {
            return Ok((cached, After::Nothing));
        }
        let (body, after) = if task == team.lead {
            stop_team(&mut transaction, team.id).await?
        } else {
            stop_helper(&mut transaction, &team, task).await?
        };
        keyed
            .record(&mut transaction, StatusCode::ACCEPTED, &body)
            .await?;
        transaction.commit().await?;
        Ok((body, after))
    })
    .await?;
    match after {
        After::Finish => {
            if let Err(error) = super::stop::finish(state, team.id).await {
                // The reconciler finishes a stopping team: nudged, it tries again now rather than at its next pass.
                tracing::warn!(code = error.code(), "a person's stop wasn't finished now");
                state.reconciler.wake();
            }
        }
        After::Helper(member, notice) => {
            if let Some(seq) = notice {
                state.channels.published(team.channel, seq);
            }
            super::stop::cancel_member(state, member).await?;
        }
        After::Nothing => {}
    }
    Ok(body)
}

/// What a stop does once committed.
enum After {
    Nothing,
    /// The team's stop: carry it to `ended`.
    Finish,
    /// A helper's stop: its notice to publish, then its run to cancel.
    Helper(Uuid, Option<u64>),
}

/// The team's stop, recorded as the person's (`stopped`); one already stopping answers the same.
async fn stop_team(
    connection: &mut PgConnection,
    team: Uuid,
) -> Result<(Value, After), GatewayError> {
    let state = sqlx::query_scalar!(
        "SELECT state FROM agent_teams WHERE id = $1 FOR UPDATE",
        team
    )
    .fetch_one(&mut *connection)
    .await?;
    if state == "ended" {
        return Err(GatewayError::Conflict("team_stopped"));
    }
    sqlx::query!(
        "UPDATE agent_teams SET state = 'stopping', stop_reason = 'stopped' WHERE id = $1 AND state = 'running'",
        team
    )
    .execute(&mut *connection)
    .await?;
    Ok((json!({ "team": { "state": "stopping" } }), After::Finish))
}

/// A helper's stop, through the lead's `stop_member`, its notice saying the person stopped it.
async fn stop_helper(
    connection: &mut PgConnection,
    team: &Found,
    member: Uuid,
) -> Result<(Value, After), GatewayError> {
    let name = sqlx::query_scalar!(
        "SELECT name FROM agent_team_members WHERE conversation_id = $1 AND team_id = $2",
        member,
        team.id
    )
    .fetch_one(&mut *connection)
    .await?;
    let caller = Caller {
        team: team.id,
        channel: team.channel,
        lead: team.lead,
        role: Role::Lead,
        name: "lead".to_owned(),
    };
    // The lead's notice says its person stopped it (the copy review's C8).
    let stopped = store::stop_member(connection, &caller, (&name, store::Stopper::Person)).await?;
    Ok((
        json!({ "member": { "name": stopped.name, "state": "stopped" } }),
        After::Helper(stopped.conversation, stopped.notice),
    ))
}

/// Refuses unless the team runs.
async fn running(connection: &mut PgConnection, team: Uuid) -> Result<(), GatewayError> {
    let state = sqlx::query_scalar!(
        "SELECT state FROM agent_teams WHERE id = $1 FOR UPDATE",
        team
    )
    .fetch_one(&mut *connection)
    .await?;
    if state == "running" {
        Ok(())
    } else {
        Err(GatewayError::Conflict("team_stopped"))
    }
}

/// Stores a person's message to the lead in the team's log at its next seq, within the channel's day.
async fn person_message(
    connection: &mut PgConnection,
    channel: Uuid,
    user: &User,
    body: &TeamMessageBody,
) -> Result<u64, GatewayError> {
    crate::channels::store::within_the_day(connection, channel).await?;
    let seq = sqlx::query_scalar!(
        "UPDATE channels SET last_seq = last_seq + 1 WHERE id = $1 RETURNING last_seq",
        channel
    )
    .fetch_one(&mut *connection)
    .await?;
    sqlx::query!(
        "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, body) \
         VALUES ($1, $2, 'user', $3, $4, $5, 'lead', $6)",
        channel,
        seq,
        user.id.to_string(),
        PERSON,
        store::type_text(body.message_type()),
        serde_json::to_value(body)?
    )
    .execute(&mut *connection)
    .await?;
    u64::try_from(seq).map_err(|_| GatewayError::StoreUnavailable)
}

/// Before the lead's task `id` is deleted: with helpers (member tasks not deleted, in any team state) it is refused
/// `409 has_helpers` unless `with_helpers`; with it, the team stops (`lead_deleted`, through the one stop path) and the
/// helpers' tasks are answered for the caller to delete first. Answers nothing to delete for any other task.
///
/// # Errors
/// `409 has_helpers`; store failure.
pub(crate) async fn before_delete(
    state: &State,
    id: &ConversationId,
    with_helpers: bool,
) -> Result<Vec<ConversationId>, GatewayError> {
    // Whatever the switch says: it stops new team activity, not what deleting a team made before removes (M3 of the
    // slice 6 review; "delete means gone", ADR-0034 point 4).
    let lead = crate::conversations::uuid(id)?;
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT t.id AS team, m.conversation_id AS helper FROM agent_teams t \
             JOIN agent_team_members m ON m.team_id = t.id JOIN conversations c ON c.id = m.conversation_id \
             WHERE t.lead_conversation = $1 AND c.state <> 'deleted'",
            lead
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let Some(team) = rows.first().map(|row| row.team) else {
        return Ok(Vec::new());
    };
    if !with_helpers {
        return Err(GatewayError::Conflict("has_helpers"));
    }
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "UPDATE agent_teams SET state = 'stopping', stop_reason = 'lead_deleted' WHERE id = $1 AND state = 'running'",
            team
        )
        .execute(&state.pool)
        .await?;
        Ok(())
    })
    .await?;
    // The team stops before any task goes: a removal that fails after it leaves a stopped team whose tasks the
    // person's next delete removes (the stop is idempotent).
    super::stop::finish(state, team).await?;
    rows.into_iter()
        .map(|row| {
            row.helper
                .to_string()
                .parse()
                .map_err(|_| GatewayError::StoreUnavailable)
        })
        .collect()
}

fn invalid(field: &'static str) -> GatewayError {
    GatewayError::Invalid {
        field,
        reason: InvalidReason::ParameterInvalid,
    }
}

/// A copy, for the lead, of what the person typed to the lead itself while its team runs and its run is active
/// (GW-I-307, real-world use before 17.0): a `person_message` naming no member, which the lead reads at its next check
/// of the team instead of after its whole team. In the message's own transaction; the message names the copy, so it is
/// delivered when read and the copy is never read once the message no longer waits. `None` for any other task, or
/// when the day's channel limit is reached: the message then waits as before.
///
/// # Errors
/// Storage failure.
pub(crate) async fn copy_to_lead(
    connection: &mut PgConnection,
    (conversation, user): (Uuid, &User),
    (text, key): (&str, &str),
) -> Result<Option<(Uuid, Uuid, u64)>, GatewayError> {
    let Some(team) = sqlx::query!(
        "SELECT id AS team, channel_id AS channel FROM agent_teams \
         WHERE lead_conversation = $1 AND owner_id = $2 AND state = 'running'",
        conversation,
        user.id
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let body = TeamMessageBody::PersonMessage {
        text: super::people_rules::cut(text, TeamMessageType::PersonMessage.max_text_bytes()),
        member: String::new(),
        by: PERSON.to_owned(),
    };
    let seq = match person_message(connection, team.channel, user, &body).await {
        Ok(seq) => seq,
        Err(GatewayError::ChannelRate(_)) => {
            tracing::warn!(team = %team.team, "a person's message to a lead wasn't copied for its next check: the day's limit");
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    sqlx::query!(
        "UPDATE idempotency SET team_copy_channel = $3, team_copy_seq = $4 WHERE key = $1 AND user_id = $2",
        key,
        user.id,
        team.channel,
        i64::try_from(seq).map_err(|_| GatewayError::StoreUnavailable)?
    )
    .execute(&mut *connection)
    .await?;
    Ok(Some((team.team, team.channel, seq)))
}

/// A copy, for the lead, of what the person typed to a helper of a running team: a `person_message` naming the helper
/// (the S6 contract, "Writing to a helper"). In the message's own transaction, so a replayed key copies nothing twice.
/// The team and the copy's seq, to publish and wake the lead once committed; `None` for any other task. A day's channel
/// limit reached skips the copy, never the person's message.
///
/// # Errors
/// Storage failure.
pub(crate) async fn copy_for_lead(
    connection: &mut PgConnection,
    (conversation, user): (Uuid, &User),
    text: &str,
) -> Result<Option<(Uuid, Uuid, u64)>, GatewayError> {
    let Some(helper) = sqlx::query!(
        "SELECT t.id AS team, t.channel_id AS channel, m.name FROM agent_team_members m \
         JOIN agent_teams t ON t.id = m.team_id WHERE m.conversation_id = $1 AND t.owner_id = $2 \
         AND t.state = 'running'",
        conversation,
        user.id
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let body = TeamMessageBody::PersonMessage {
        text: super::people_rules::cut(text, TeamMessageType::PersonMessage.max_text_bytes()),
        member: helper.name,
        by: PERSON.to_owned(),
    };
    match person_message(connection, helper.channel, user, &body).await {
        Ok(seq) => Ok(Some((helper.team, helper.channel, seq))),
        Err(GatewayError::ChannelRate(_)) => {
            tracing::warn!(team = %helper.team, "a person's message to a helper wasn't copied for the lead: the day's limit");
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
