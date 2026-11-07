//! "Send after this" (GW-I-129..131): a message sent while the task's run is in progress waits on the gateway, as a
//! queued action marked `held`, and goes out when the run ends. Every message held meanwhile goes out as one user
//! turn, in order, the way people add "also…" follow-ups. A run that fails or is stopped drops them. Their author or
//! the task's owner may cancel one before it goes out. Clients learn of every change through an unsequenced
//! `message.queue` frame; the list endpoint is the durable view.
use crate::{
    GatewayError,
    app::State,
    auth::{Authz, User},
    conversations,
};
use axum::{
    Json,
    extract::{Path, State as AppState},
    http::StatusCode,
};
use serde::Serialize;
use std::sync::Arc;
use tqa_types::{
    ClientFrame, ConversationId, MAX_ATTACHMENTS, MAX_USER_MESSAGE_BYTES, ServerFrame,
};

/// What a turn's texts are joined with.
const SEPARATOR: &str = "\n\n";
/// The most messages the list shows.
const MAX_LISTED: i64 = 200;

/// The route a conversation's messages are recorded under (their idempotency rows).
pub(crate) fn route(id: &ConversationId) -> String {
    format!("/api/v1/conversations/{id}/messages")
}

/// One held message's part of a turn.
#[derive(Clone)]
pub(crate) struct Part {
    pub text: String,
    pub attachments: Vec<String>,
    /// An agent team member's first turn (revision 12): it goes as a turn of its own, never joined with another.
    pub lead_brief: Option<tqa_types::LeadBrief>,
    /// The gateway's wake of a team reader (revision 13): it too goes as a turn of its own, so its marker never covers
    /// a person's words.
    pub team_wake: Option<tqa_types::TeamWake>,
}

impl Part {
    /// Whether this part is a turn of its own: a brief or a wake.
    const fn alone(&self) -> bool {
        self.lead_brief.is_some() || self.team_wake.is_some()
    }
}

/// The first `parts` that fit one user turn, and the turn: texts joined in order with a blank line, attachments in
/// order, each once. The first always fits (each message was within the bounds on its own); the rest wait for the
/// next turn. A member's first turn, with its lead's brief, and the gateway's wake always go alone.
pub(crate) fn join(parts: &[Part]) -> (usize, ClientFrame) {
    if let Some(first) = parts.first().filter(|part| part.alone()) {
        return (
            1,
            ClientFrame::UserMessage {
                lead_brief: first.lead_brief.clone(),
                team_wake: first.team_wake,
                text: first.text.clone(),
                attachments: first.attachments.clone(),
                host_keys: Vec::new(),
            },
        );
    }
    let mut text = String::new();
    let mut attachments: Vec<String> = Vec::new();
    let mut taken = 0;
    for part in parts.iter().take_while(|part| !part.alone()) {
        let joined_len = if taken == 0 {
            part.text.len()
        } else {
            text.len() + SEPARATOR.len() + part.text.len()
        };
        let mut added: Vec<&String> = Vec::new();
        for attachment in &part.attachments {
            if !attachments.contains(attachment) && !added.contains(&attachment) {
                added.push(attachment);
            }
        }
        if taken > 0
            && (joined_len > MAX_USER_MESSAGE_BYTES
                || attachments.len() + added.len() > MAX_ATTACHMENTS)
        {
            break;
        }
        if taken > 0 {
            text.push_str(SEPARATOR);
        }
        text.push_str(&part.text);
        attachments.extend(added.into_iter().cloned());
        taken += 1;
    }
    (
        taken,
        ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: None,
            text,
            attachments,
            host_keys: Vec::new(),
        },
    )
}

/// Tells the task's open streams which messages wait and which were dropped; `sent` names a turn that just went out.
///
/// # Errors
/// A store failure, or a stream that can't take the notice.
pub(crate) async fn publish(
    pool: &sqlx::PgPool,
    events: &crate::events::EventStore,
    id: &ConversationId,
    sent: Option<Vec<String>>,
) -> Result<(), GatewayError> {
    let conversation = conversations::uuid(id)?;
    let rows = sqlx::query!(
        "SELECT ordinal AS \"ordinal!\", execution_status FROM idempotency WHERE conversation_id=$1 AND route=$2 \
         AND held AND (execution_status='queued' OR (execution_status='failed' AND execution_error IN ('run_failed','run_cancelled'))) \
         ORDER BY ordinal LIMIT $3",
        conversation,
        route(id),
        MAX_LISTED
    )
    .fetch_all(pool)
    .await?;
    let (mut held, mut dropped) = (Vec::new(), Vec::new());
    for row in rows {
        if row.execution_status == "queued" {
            held.push(row.ordinal.to_string());
        } else {
            dropped.push(row.ordinal.to_string());
        }
    }
    events.control(
        id,
        ServerFrame::MessageQueue {
            held,
            dropped,
            sent,
        },
    )
}

#[derive(Serialize)]
pub(crate) struct Author {
    id: uuid::Uuid,
    email: String,
}

#[derive(Serialize)]
pub(crate) struct Item {
    id: String,
    text: String,
    attachments: Vec<String>,
    author: Author,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    sent_at: Option<time::OffsetDateTime>,
    /// The gateway's wake of a team reader (GW-I-298): a client shows its notice, never the person's message.
    #[serde(skip_serializing_if = "Option::is_none")]
    team_wake: Option<tqa_types::TeamWake>,
    /// A team member's first turn, with its lead's brief (GW-I-300): a client shows its notice, not the frame.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    lead_brief: bool,
}

#[derive(Serialize)]
pub(crate) struct Items {
    items: Vec<Item>,
}

/// `GET /api/v1/conversations/{id}/messages/queued`: the messages that waited behind a run, oldest first: `held`
/// (still waiting), `dropped` (the run failed or was stopped) and `sent`, the last two kept for the idempotency
/// retention; and `waiting_for_slot`, a message not behind a run that has not gone out yet (the task waits for a
/// slot or its sandbox is starting), so every viewer can cancel it (GW-I-173). The gateway's wake of a team reader says
/// so (`team_wake`, GW-I-298), and a member's first turn too (`lead_brief`, GW-I-300), so a client shows its notice
/// rather than the person's message.
///
/// # Errors
/// `404` for a task the caller can't read; a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
) -> Result<Json<Items>, GatewayError> {
    conversations::authorize(&state, &user, &id, false).await?;
    let conversation = conversations::uuid(&id)?;
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT i.ordinal AS \"ordinal!\", i.user_id, u.email::text AS \"email!\", i.created_at, i.updated_at, \
             i.execution_status, i.execution_error, i.queued_frame, i.held FROM idempotency i JOIN users u ON u.id=i.user_id \
             WHERE i.conversation_id=$1 AND i.route=$2 AND (i.held OR i.execution_status='queued') \
             AND i.execution_status<>'cancelled' \
             ORDER BY i.ordinal LIMIT $3",
            conversation,
            route(&id),
            MAX_LISTED
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let items = rows
        .into_iter()
        .filter_map(|row| {
            let Ok(ClientFrame::UserMessage {
                text,
                attachments,
                team_wake,
                lead_brief,
                ..
            }) = ClientFrame::parse(row.queued_frame.as_deref()?)
            else {
                return None;
            };
            let (state, sent_at) = match row.execution_status.as_str() {
                "queued" if row.held => ("held", None),
                "queued" => ("waiting_for_slot", None),
                "sending" | "delivered" => ("sent", Some(row.updated_at)),
                _ => ("dropped", None),
            };
            Some(Item {
                id: row.ordinal.to_string(),
                text,
                attachments,
                author: Author {
                    id: row.user_id,
                    email: row.email,
                },
                created_at: row.created_at,
                state,
                reason: (state == "dropped")
                    .then_some(row.execution_error)
                    .flatten(),
                sent_at,
                team_wake,
                lead_brief: lead_brief.is_some(),
            })
        })
        .collect();
    Ok(Json(Items { items }))
}

/// `DELETE /api/v1/conversations/{id}/messages/queued/{message_id}`: cancels a held message or one waiting for a
/// slot (which withdraws the task from the slot queue), or dismisses a dropped one; only its author or the task's owner
/// may. A team's own turn (a wake or a member's first turn), waiting or dropped, is the gateway's: no one cancels or
/// dismisses it.
///
/// # Errors
/// `404` for an unknown message or a task the caller can't reach, `403` for anyone else, `409 message_sent` once it
/// went out or its slot was granted (its sandbox is starting), `409 team_turn` for an agent team's wake or a member's
/// first turn; a store failure.
pub(crate) async fn cancel(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((id, message)): Path<(ConversationId, String)>,
) -> Result<StatusCode, GatewayError> {
    conversations::authorize(&state, &user, &id, false).await?;
    let conversation = conversations::uuid(&id)?;
    let ordinal: i64 = message.parse().map_err(|_| GatewayError::NotFound)?;
    let waited_for_slot = cancel_row(&state, &user, &id, conversation, ordinal).await?;
    // Cancelled before it was granted a slot: the task leaves the slot queue (GW-I-173).
    if waited_for_slot && let Err(error) = state.manager.withdrawn(&id).await {
        tracing::warn!(conversation_id = %id, code = error.code(), "slot queue notice not delivered");
    }
    if let Err(error) = publish(&state.pool, &state.events, &id, None).await {
        tracing::warn!(conversation_id = %id, code = error.code(), "held messages notice not delivered");
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn cancel_row(
    state: &State,
    user: &User,
    id: &ConversationId,
    conversation: uuid::Uuid,
    ordinal: i64,
) -> Result<bool, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        // The admission lock, then the message's row: the admission's own order (admit_waiting_work takes the task's
        // row before the lock, which this needs no part of, and never a message's row). Under it a grant either
        // committed before this cancel, and its reservation is seen below, or comes after and finds the work gone.
        crate::db::queue(&mut transaction, crate::db::Queue::Key(74_271_012)).await?;
        let row = sqlx::query!(
            "SELECT i.key, i.user_id, i.execution_status, i.held, i.queued_frame, c.owner_id FROM idempotency i JOIN conversations c ON c.id=i.conversation_id \
             WHERE i.conversation_id=$1 AND i.route=$2 AND i.ordinal=$3 FOR UPDATE OF i",
            conversation,
            route(id),
            ordinal
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(GatewayError::NotFound)?;
        if row.user_id != user.id && row.owner_id != user.id {
            return Err(GatewayError::Forbidden("only its author or the task's owner may cancel a message"));
        }
        // A team's own turn is no one's to cancel (GW-I-299, GW-I-300): cancelled, its key would replay as cancelled,
        // and the reader wouldn't be woken again for what waits, or the member would never get its brief.
        if team_turn(row.queued_frame.as_deref()) {
            return Err(GatewayError::Conflict("team_turn"));
        }
        // A message not held behind a run waits for a slot (GW-I-173). Once the slot is granted its sandbox is
        // starting and the message will go out: a cancel that answers 204 means no sandbox starts for it.
        match row.execution_status.as_str() {
            "queued" if !row.held && granted(&mut transaction, conversation).await? => {
                return Err(GatewayError::Conflict("message_sent"));
            }
            "queued" => {}
            "failed" if row.held => {}
            // Claimed for a turn, or already sent: the agent gets it.
            "sending" | "delivered" => return Err(GatewayError::Conflict("message_sent")),
            _ => return Err(GatewayError::NotFound),
        }
        sqlx::query!(
            "UPDATE idempotency SET execution_status='cancelled',execution_error=NULL,updated_at=CURRENT_TIMESTAMP WHERE key=$1 AND user_id=$2",
            row.key,
            row.user_id
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        tracing::info!(conversation_id = %id, user_id = %user.id, held = row.held, "waiting message cancelled");
        Ok(!row.held)
    })
    .await
}

/// Whether a queued frame is the gateway's turn for an agent team's reader, not a person's message: its wake, or a
/// member's first turn with its lead's brief.
fn team_turn(frame: Option<&[u8]>) -> bool {
    matches!(
        frame.map(ClientFrame::parse),
        Some(Ok(ClientFrame::UserMessage {
            team_wake: Some(_),
            ..
        } | ClientFrame::UserMessage {
            lead_brief: Some(_),
            ..
        }))
    )
}

/// Whether the task holds a slot: its sandbox's record, written by the grant under the admission lock.
async fn granted(
    connection: &mut sqlx::PgConnection,
    conversation: uuid::Uuid,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM sandbox_instances WHERE conversation_id=$1) AS \"granted!\"",
        conversation
    )
    .fetch_one(connection)
    .await?)
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
