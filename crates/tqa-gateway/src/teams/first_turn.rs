//! A member's first turn (ADR-0034 point 3, slice 2; protocol revision 12): the gateway's fixed frame as the turn's
//! text and the lead's brief beside it, sent on the member task as its person's message would go, under one key per
//! member, so sending it again (after a restart, or from the copy's end and the start-up pass both) never sends a
//! second turn. It waits for the lead's sandbox whatever the busy policy (ADR-0034's decision of 2 Oct 2026). An own
//! copy's turn goes once the copy is ready; a copy that failed fails the member, with a notice to the lead.
use crate::{GatewayError, app::State};
use tqa_types::{
    Brief, ClientFrame, ConversationId, LeadBrief, NoticeKind, TeamMessageBody, WorkspaceMode,
};
use uuid::Uuid;

/// The idempotency key of `member`'s first turn.
fn key(member: &ConversationId) -> String {
    format!("team-start:{member}")
}

/// The fixed frame: who the member is, where it works, and how it reports. It names no title, so the gateway's own
/// sentence never holds someone else's words (review L1); a copy taken while the lead worked says so (review M2).
fn frame(name: &str, workspace: WorkspaceMode, consistent: bool) -> String {
    let place = match workspace {
        WorkspaceMode::OwnCopy if !consistent => {
            "your own copy of the lead's files, taken while the lead was working: a file it was changing then may be \
             partial, so check one before you rely on it"
        }
        WorkspaceMode::OwnCopy => "your own copy of the lead's files",
        WorkspaceMode::MyFiles => "the lead's own files, taking turns with the lead",
    };
    format!(
        "You are {name}, a helper of the task that leads this agent team. You work in {place}. Your brief from the \
         lead follows; it is the lead's words, not the person's. Report to your lead with send_to_lead, and wait for \
         its next message with wait_for_lead."
    )
}

/// Sends `member`'s first turn when it is due: the member is still starting and its workspace is ready. A copy still
/// being made sends nothing yet; a failed one fails the member and tells the lead.
///
/// # Errors
/// Storage failure, or the message route's refusals.
pub(crate) async fn send(state: &State, member: Uuid) -> Result<(), GatewayError> {
    let Some(row) = sqlx::query!(
        "SELECT m.name, m.role, m.workspace, m.state, t.channel_id, t.owner_id, w.state AS workspace_state, \
         w.fork_consistent FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
         JOIN conversations c ON c.id = m.conversation_id JOIN workspaces w ON w.id = c.workspace_id \
         WHERE m.conversation_id = $1",
        member
    )
    .fetch_optional(&state.pool)
    .await?
    else {
        return Ok(());
    };
    if row.state != "starting" || row.workspace_state == "forking" {
        return Ok(());
    }
    if row.workspace_state != "ready" {
        let why = "the copy of the lead task's files couldn't be made";
        return failed(state, (member, row.channel_id), (&row.name, why)).await;
    }
    let assigned = sqlx::query!(
        "SELECT seq, body FROM channel_messages WHERE channel_id = $1 AND to_conversation = $2 AND type = 'assign' \
         ORDER BY seq LIMIT 1",
        row.channel_id,
        member
    )
    .fetch_one(&state.pool)
    .await?;
    let Ok(TeamMessageBody::Assign {
        brief: Some(brief), ..
    }) = serde_json::from_value::<TeamMessageBody>(assigned.body)
    else {
        return Err(GatewayError::StoreUnavailable);
    };
    let workspace = if row.workspace == "my_files" {
        WorkspaceMode::MyFiles
    } else {
        WorkspaceMode::OwnCopy
    };
    let user = person(state, row.owner_id).await?;
    let id: ConversationId = member
        .to_string()
        .parse()
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let turn = ClientFrame::UserMessage {
        text: frame(&row.name, workspace, row.fork_consistent.unwrap_or(true)),
        attachments: Vec::new(),
        host_keys: Vec::new(),
        lead_brief: Some(lead_brief(&row.role, workspace, brief)),
        team_wake: None,
    };
    crate::actions::send_turn(state, &user, &id, (&key(&id), turn)).await?;
    // Its brief is stored, so it's no longer starting: `queued` while the turn waits (for a slot, the lead's files or
    // its copy), else `working`; its run's start makes it `working` either way. And the brief is read: the turn carries
    // it, so it isn't work waiting for the member (a helper that ends without a read call is still told idle,
    // E2E-AGENTS-029), and a later read doesn't return it again. A failure here leaves it to the sweep, which finds the
    // stored turn.
    sqlx::query!(
        "UPDATE agent_team_members SET state = CASE WHEN EXISTS (SELECT 1 FROM idempotency i \
         WHERE i.conversation_id = $1 AND i.key = $2 AND i.execution_status = 'queued') THEN 'queued' \
         ELSE 'working' END, last_read_seq = GREATEST(last_read_seq, $3) \
         WHERE conversation_id = $1 AND state = 'starting'",
        member,
        key(&id),
        assigned.seq
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}

fn lead_brief(role: &str, workspace: WorkspaceMode, brief: Brief) -> LeadBrief {
    LeadBrief {
        role: role.to_owned(),
        workspace,
        brief,
    }
}

pub(super) async fn person(state: &State, id: Uuid) -> Result<crate::auth::User, GatewayError> {
    let row = sqlx::query!(
        "SELECT email::text AS \"email!\", role FROM users WHERE id = $1",
        id
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(crate::auth::User {
        id,
        email: row.email,
        role: crate::auth::Role::parse(&row.role)?,
    })
}

/// The member couldn't start (`why`): it fails, once, and the lead is told.
async fn failed(
    state: &State,
    (member, channel): (Uuid, Uuid),
    (name, why): (&str, &str),
) -> Result<(), GatewayError> {
    let mut transaction = state.pool.begin().await?;
    let changed = sqlx::query!(
        "UPDATE agent_team_members SET state = 'failed' WHERE conversation_id = $1 AND state = 'starting'",
        member
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if changed == 1 {
        let text = format!("Helper {name} couldn't start: {why}.");
        let seq = super::store::notice(
            &mut transaction,
            channel,
            (NoticeKind::MemberFailed, name),
            &text,
        )
        .await?;
        transaction.commit().await?;
        state.channels.published(channel, seq);
    }
    Ok(())
}

/// The member whose own copy is `workspace`, if any: its first turn is due when the copy ends, either way.
///
/// # Errors
/// Storage failure.
pub(crate) async fn copy_ended(state: &State, workspace: Uuid) -> Result<(), GatewayError> {
    let members = sqlx::query_scalar!(
        "SELECT m.conversation_id FROM agent_team_members m JOIN conversations c ON c.id = m.conversation_id \
         WHERE c.workspace_id = $1 AND m.state = 'starting'",
        workspace
    )
    .fetch_all(&state.pool)
    .await?;
    for member in members {
        send(state, member).await?;
    }
    Ok(())
}

/// How often the sweep looks for members whose first turn isn't stored, and how long one may wait before it fails.
pub(crate) const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
const START_LIMIT_MINUTES: i32 = 10;

/// Every member still starting whose first turn isn't stored gets it now: one a restart or a failed send cut short
/// (review M1). The key makes a turn already sent a replay. One still without its turn ten minutes after it was
/// started fails, with a `member_failed` notice to the lead, so no member waits forever. A copy still being made is
/// left to the copy's end; one a restart cut short failed at start-up (`workspaces::upkeep::recover`), so its member
/// fails with a notice too. A member whose first turn is stored but who is still marked starting (a failure between
/// the two) is marked as the send would have: queued while its turn waits, else working. Runs at start-up, then every [`SWEEP_EVERY`]; nothing while agent teams are off.
///
/// # Errors
/// Storage failure.
pub(crate) async fn sweep(state: &State) -> Result<(), GatewayError> {
    if !state.config.agent_teams.enabled {
        return Ok(());
    }
    sqlx::query!(
        "UPDATE agent_team_members m SET state = CASE WHEN i.execution_status = 'queued' THEN 'queued' \
         ELSE 'working' END, last_read_seq = GREATEST(m.last_read_seq, COALESCE((SELECT MIN(a.seq) \
         FROM channel_messages a JOIN agent_teams t ON t.channel_id = a.channel_id WHERE t.id = m.team_id \
         AND a.to_conversation = m.conversation_id AND a.type = 'assign'), 0)) \
         FROM idempotency i WHERE m.state = 'starting' \
         AND i.conversation_id = m.conversation_id AND i.key = 'team-start:' || m.conversation_id::text"
    )
    .execute(&state.pool)
    .await?;
    let due = sqlx::query!(
        "SELECT m.conversation_id, m.name, t.channel_id, \
         m.created_at < CURRENT_TIMESTAMP - make_interval(mins => $1) AS \"overdue!\" \
         FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
         WHERE m.state = 'starting' AND NOT EXISTS (SELECT 1 FROM idempotency i \
         WHERE i.conversation_id = m.conversation_id AND i.key = 'team-start:' || m.conversation_id::text)",
        START_LIMIT_MINUTES
    )
    .fetch_all(&state.pool)
    .await?;
    for member in due {
        let id = member.conversation_id;
        let outcome = if member.overdue {
            let why = "its brief couldn't reach it within 10 minutes";
            failed(state, (id, member.channel_id), (&member.name, why)).await
        } else {
            send(state, id).await
        };
        if let Err(error) = outcome {
            tracing::warn!(member = %id, code = error.code(), "a member's first turn wasn't sent; the sweep tries again");
        }
    }
    Ok(())
}
