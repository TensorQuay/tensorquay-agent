//! A task's place in an agent team, as the task routes answer it (`Conversation.team`, ADR-0034 the slice S6
//! contract). Only the task's own person gets every field; anyone else (an administrator's listing of other people's
//! tasks, SEC-005) gets the role, the team's id and state, a lead's counts and a member's lead id: never a name, the
//! lead's title, a helper's state or a question. While agent teams are off every task's `team` is null.
use std::collections::HashMap;

use uuid::Uuid;

pub use super::place_shape::{Lead, TeamPlace, Title};
use super::place_shape::{attention, can_stop, helper_state};
use crate::{Conversation, GatewayError, app::State};

/// Fills each task's `team` for `viewer`, in one read for all of them. Nothing is read while agent teams are off.
///
/// # Errors
/// A store failure.
pub(crate) async fn attach(
    state: &State,
    viewer: Uuid,
    conversations: &mut [Conversation],
) -> Result<(), GatewayError> {
    if !state.config.agent_teams.enabled || conversations.is_empty() {
        return Ok(());
    }
    let ids = conversations
        .iter()
        .map(|conversation| crate::conversations::uuid(&conversation.id))
        .collect::<Result<Vec<_>, _>>()?;
    let places = crate::io::bounded(
        state.config.io_timeout(),
        &state.cancel,
        places(state, &ids),
    )
    .await?;
    for (conversation, id) in conversations.iter_mut().zip(ids) {
        let own = conversation.owner_id == viewer;
        conversation.team = places.get(&id).cloned().map(|place| place.for_viewer(own));
    }
    Ok(())
}

/// The places of `ids` that are in a team, by task.
async fn places(state: &State, ids: &[Uuid]) -> Result<HashMap<Uuid, TeamPlace>, GatewayError> {
    let mut connection = state.pool.acquire().await?;
    let mut places = HashMap::new();
    let leads = sqlx::query!(
        "SELECT t.lead_conversation AS \"task!\", t.id, t.state, \
         (SELECT count(*) FROM agent_team_members m JOIN conversations c ON c.id = m.conversation_id \
          WHERE m.team_id = t.id AND c.state <> 'deleted') AS \"helpers!\", \
         (SELECT count(*) FROM agent_team_members m JOIN conversations c ON c.id = m.conversation_id \
          WHERE m.team_id = t.id AND c.state <> 'deleted' AND m.state = 'stuck') AS \"stuck!\", \
         (SELECT count(*) FROM agent_team_members m JOIN conversations c ON c.id = m.conversation_id \
          WHERE m.team_id = t.id AND c.state <> 'deleted' AND m.state = 'failed') AS \"failed!\", \
         t.state = 'running' AND EXISTS (SELECT 1 FROM channel_messages q WHERE q.channel_id = t.channel_id \
          AND q.type = 'person_question' \
          AND NOT EXISTS (SELECT 1 FROM channel_messages a WHERE a.channel_id = q.channel_id \
          AND a.type = 'person_answer' AND a.body->>'question_seq' = q.seq::text)) AS \"waiting!\" \
         FROM agent_teams t WHERE t.lead_conversation = ANY($1)",
        ids
    )
    .fetch_all(&mut *connection)
    .await?;
    for row in leads {
        let running = row.state == "running";
        places.insert(
            row.task,
            TeamPlace::Lead {
                id: row.id,
                state: row.state,
                helpers: count(row.helpers),
                attention: attention(running, count(row.stuck), count(row.failed)),
                waiting_for_you: Some(row.waiting),
            },
        );
    }
    let members = sqlx::query!(
        "SELECT m.conversation_id AS task, t.id, t.state, t.lead_conversation, c.title AS \"title?\", m.name, \
         m.state AS helper_state, m.failed_run FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
         JOIN conversations c ON c.id = t.lead_conversation WHERE m.conversation_id = ANY($1) \
         AND c.state <> 'deleted'",
        ids
    )
    .fetch_all(&mut *connection)
    .await?;
    for row in members {
        let stoppable = can_stop(row.state == "running", &row.helper_state, row.failed_run);
        places.insert(
            row.task,
            TeamPlace::Member {
                id: row.id,
                state: row.state,
                lead: Lead {
                    id: row.lead_conversation,
                    title: Some(Title { title: row.title }),
                },
                name: Some(row.name),
                helper_state: Some(helper_state(&row.helper_state)),
                can_stop: Some(stoppable),
            },
        );
    }
    Ok(places)
}

fn count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
