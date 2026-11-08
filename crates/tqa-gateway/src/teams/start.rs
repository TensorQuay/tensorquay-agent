//! A member joins its lead's agent team (ADR-0034 point 3, DEV-PLAN Phase 10 slice 2): the team is created with the
//! first member, the member cap is checked, and the member is named. One transaction, under the lead's conversation
//! row lock, so a lead's starts take turns: the cap and the name can't race (GW-I-106, GW-I-196).
use super::cap::Cap;
use crate::{GatewayError, config::AgentTeams};
use sqlx::PgConnection;
use tqa_types::WorkspaceMode;
use uuid::Uuid;

/// Who joins whose team.
pub(crate) struct Joining<'a> {
    pub(crate) lead: Uuid,
    /// The member's conversation, already created in this transaction.
    pub(crate) member: Uuid,
    pub(crate) role: &'a str,
    pub(crate) workspace: WorkspaceMode,
}

/// The team the member joined, and its name there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Joined {
    pub(crate) team: Uuid,
    pub(crate) channel: Uuid,
    pub(crate) name: String,
    /// The lead's first member: the team was created with it.
    pub(crate) first: bool,
}

/// How often a name taken meanwhile is tried again. Under the lead's lock no other start can take one, so this is a
/// backstop (re-review note on `lower(name)`): a unique violation never becomes a 500.
const NAME_ATTEMPTS: usize = 3;

/// Adds `joining.member` to the lead's open team, creating the team with its first member.
///
/// # Errors
/// `404` for an unknown lead, `403 not_lead` when the lead is a member of any team, ended or not (depth 1),
/// `409 team_stopped` or `team_ended`, `409 member_limit` at the cap, storage failure.
pub(crate) async fn join(
    connection: &mut PgConnection,
    joining: &Joining<'_>,
    cap: Cap,
    teams: &AgentTeams,
) -> Result<Joined, GatewayError> {
    let owner = sqlx::query_scalar!(
        "SELECT owner_id FROM conversations WHERE id = $1 FOR UPDATE",
        joining.lead
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    // Depth 1: a conversation that was ever a member never leads.
    if sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM agent_team_members WHERE conversation_id = $1) AS \"member!\"",
        joining.lead
    )
    .fetch_one(&mut *connection)
    .await?
    {
        return Err(GatewayError::Denied("not_lead"));
    }
    let (team, channel, first) = open_team(connection, joining.lead, owner, teams).await?;
    // A done helper keeps its seat until it is stopped: it may be given more work.
    let live = sqlx::query_scalar!(
        "SELECT count(*) AS \"live!\" FROM agent_team_members WHERE team_id = $1 \
         AND state <> 'stopped' AND NOT (state = 'failed' AND NOT failed_run)",
        team
    )
    .fetch_one(&mut *connection)
    .await?;
    if live >= i64::from(cap.limit) {
        return Err(GatewayError::MemberLimit {
            limit: cap.limit,
            by: cap.by,
        });
    }
    let workspace = match joining.workspace {
        WorkspaceMode::OwnCopy => "own_copy",
        WorkspaceMode::MyFiles => "my_files",
    };
    for _ in 0..NAME_ATTEMPTS {
        let taken = sqlx::query_scalar!(
            "SELECT name FROM agent_team_members WHERE team_id = $1",
            team
        )
        .fetch_all(&mut *connection)
        .await?;
        let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
        let name = super::validate::unique_name(joining.role, &taken);
        let inserted = sqlx::query_scalar!(
            "INSERT INTO agent_team_members (conversation_id, team_id, name, role, workspace) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (team_id, lower(name)) DO NOTHING RETURNING name",
            joining.member,
            team,
            name,
            joining.role,
            workspace
        )
        .fetch_optional(&mut *connection)
        .await?;
        if let Some(name) = inserted {
            return Ok(Joined {
                team,
                channel,
                name,
                first,
            });
        }
    }
    Err(GatewayError::StoreUnavailable)
}

/// A started member, as `start_member` answers it, and the copy to make once its transaction commits.
pub(crate) struct Started {
    /// The answer's `member`.
    pub(crate) member: serde_json::Value,
    pub(crate) channel: Uuid,
    /// The brief's seq.
    pub(crate) seq: u64,
    /// An own copy's source, the copy, and its quota.
    pub(crate) fork: Option<(Uuid, Uuid, u64)>,
}

/// `start_member`'s work in the caller's transaction: the caller may lead (depth 1), the profile it names, the member
/// task and its workspace, the team and the cap, the member's name as its title, and the brief as an `assign`.
///
/// # Errors
/// As the route: `not_lead`, `unknown_reference`, `profile_unusable`, `team_stopped`, `team_ended`, `member_limit`,
/// the fork's own codes, `channel_rate`, storage failure.
pub(crate) async fn start_member(
    connection: &mut PgConnection,
    state: &crate::app::State,
    (leader, owner): (Uuid, Uuid),
    request: &tqa_types::StartMember,
) -> Result<Started, GatewayError> {
    let caller = super::store::caller(connection, leader).await?;
    super::roles::allowed(
        caller.map(|caller| caller.role),
        super::roles::Route::StartMember,
    )?;
    let profile = match &request.profile {
        Some(name) => Some(profile_named(connection, state, owner, name).await?),
        None => None,
    };
    // The administrator's limits in force (ADR-0034 S6, ADR-0028): the cap from the next helper, the budget and the
    // deadline from the next team; the file's values are only their seeds.
    let limits = state.settings.current().limits;
    let cap = super::cap::cap(limits.team_max_members, limits.slots, limits.per_user);
    let teams = AgentTeams {
        max_members: limits.team_max_members,
        token_budget: limits.team_token_budget,
        deadline_minutes: limits.team_deadline_minutes,
        ..state.config.agent_teams.clone()
    };
    let member = crate::conversations::member::create(
        connection,
        state,
        (leader, &request.role),
        profile,
        request.workspace,
    )
    .await?;
    let joining = Joining {
        lead: leader,
        member: member.conversation,
        role: &request.role,
        workspace: request.workspace,
    };
    let joined = join(connection, &joining, cap, &teams).await?;
    retitle(connection, member.conversation, &joined.name).await?;
    let seq = super::store::assign_brief(
        connection,
        joined.channel,
        (leader, member.conversation),
        &request.brief,
    )
    .await?;
    let mut shown = serde_json::json!({
        "name": joined.name,
        "conversation": member.conversation,
        "workspace": request.workspace,
        "workspace_id": member.workspace,
        // On the lead's files the member waits for the lead's sandbox to hand over its files.
        "state": if request.workspace == tqa_types::WorkspaceMode::MyFiles { "queued" } else { "starting" },
    });
    // A copy of files the lead's sandbox had open may hold half-written ones: the lead is told.
    if let Some(consistent) = member.consistent {
        shown["consistent"] = serde_json::json!(consistent);
    }
    if let Some(profile) = &request.profile {
        shown["profile"] = serde_json::json!(profile);
    }
    if !member.left_out.is_empty() {
        shown["left_out"] = serde_json::json!(member.left_out);
    }
    Ok(Started {
        member: shown,
        channel: joined.channel,
        seq,
        fork: member
            .fork
            .map(|(source, quota)| (source, member.workspace, quota)),
    })
}

/// The member task's title: its name in the team.
///
/// # Errors
/// Storage failure.
pub(crate) async fn retitle(
    connection: &mut PgConnection,
    member: Uuid,
    name: &str,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE conversations SET title = $2 WHERE id = $1",
        member,
        name
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// A profile `start_member` names, by its name: the person's own first, then a shared one, usable now.
///
/// # Errors
/// `422 unknown_reference` (`profile`) for none the person may use, `409 profile_unusable`, storage failure.
pub(crate) async fn profile_named(
    connection: &mut PgConnection,
    state: &crate::app::State,
    owner: Uuid,
    name: &str,
) -> Result<crate::profiles::Chosen, GatewayError> {
    let id = sqlx::query_scalar!(
        "SELECT id FROM agent_profiles WHERE name = $1 AND (owner_id = $2 OR owner_id IS NULL) \
         ORDER BY owner_id IS NULL LIMIT 1",
        name,
        owner
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::UnknownReference { field: "profile" })?;
    crate::profiles::chosen(connection, state, owner, id).await
}

/// The team's log channel and its row: the budget and the deadline from the limits in force.
/// The lead's open team, or a new one: `(team, channel, first)`. A lead leads one team: once it has ended, its team
/// tools answer `team_ended` (ADR-0034 point 10).
///
/// # Errors
/// `409 team_stopped` while it stops, `409 team_ended`, storage failure.
async fn open_team(
    connection: &mut PgConnection,
    lead: Uuid,
    owner: Uuid,
    teams: &AgentTeams,
) -> Result<(Uuid, Uuid, bool), GatewayError> {
    let open = sqlx::query!(
        "SELECT id, channel_id, state FROM agent_teams WHERE lead_conversation = $1 AND state <> 'ended'",
        lead
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(match open {
        Some(row) if row.state != "running" => return Err(GatewayError::Conflict("team_stopped")),
        Some(row) => (row.id, row.channel_id, false),
        // A task leads one team: once it has ended, its team tools answer team_ended (ADR-0034 point 10).
        None if super::store::ended(connection, lead).await?.is_some() => {
            return Err(GatewayError::TeamEnded(None));
        }
        None => {
            let (team, channel) = create(connection, lead, owner, teams).await?;
            (team, channel, true)
        }
    })
}

async fn create(
    connection: &mut PgConnection,
    lead: Uuid,
    owner: Uuid,
    teams: &AgentTeams,
) -> Result<(Uuid, Uuid), GatewayError> {
    let (team, channel) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO channels (id, owner_id, name, rule, kind) VALUES ($1, $2, 'Agent team', 'human', 'agent_team')",
        channel,
        owner
    )
    .execute(&mut *connection)
    .await?;
    sqlx::query!(
        "INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, token_budget, deadline_at) \
         VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP + make_interval(mins => $6))",
        team,
        channel,
        lead,
        owner,
        i64::try_from(teams.token_budget).unwrap_or(i64::MAX),
        i32::try_from(teams.deadline_minutes).unwrap_or(i32::MAX)
    )
    .execute(&mut *connection)
    .await?;
    Ok((team, channel))
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
