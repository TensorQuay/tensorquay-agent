//! Each open agent team's state for the reconciler's decisions ([`super::decide`]), read from the store. Liveness reads
//! only what is stored: a run's state from the transcript's lifecycle events, progress from its latest stored event,
//! and what the lead was already told from the gateway's notices in the team's log.
use super::decide::{MemberView, Run, TeamState, TeamView, Told};
use crate::GatewayError;
use sqlx::PgConnection;
use uuid::Uuid;

/// One open team: its ids beside its state.
pub(crate) struct OpenTeam {
    pub id: Uuid,
    pub channel: Uuid,
    pub view: TeamView,
}

/// Every team that hasn't ended, with its stored state.
///
/// # Errors
/// Storage failure.
pub(crate) async fn open_teams(
    connection: &mut PgConnection,
) -> Result<Vec<OpenTeam>, GatewayError> {
    let teams = sqlx::query!(
        "SELECT t.id, t.channel_id, t.lead_conversation, t.state, \
         EXTRACT(EPOCH FROM t.deadline_at)::bigint AS \"deadline_at!\", \
         (EXTRACT(EPOCH FROM t.deadline_at - t.created_at) / 60)::int AS \"deadline_minutes!\", \
         EXISTS (SELECT 1 FROM channel_messages n WHERE n.channel_id = t.channel_id AND n.type = 'notice' \
                 AND n.body->>'kind' = 'deadline_near') AS \"deadline_notified!\", \
         EXTRACT(EPOCH FROM GREATEST(t.created_at, \
             (SELECT MAX(c.created_at) FROM channel_messages c \
              WHERE c.channel_id = t.channel_id AND c.author_kind <> 'gateway'), \
             (SELECT MAX(e.created_at) FROM events e WHERE e.lifecycle IS NOT NULL AND (e.conversation_id = \
              t.lead_conversation OR e.conversation_id IN \
              (SELECT m.conversation_id FROM agent_team_members m WHERE m.team_id = t.id)))))::bigint \
              AS \"last_activity_at!\" \
         FROM agent_teams t WHERE t.state <> 'ended' ORDER BY t.created_at"
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut open = Vec::with_capacity(teams.len());
    for team in teams {
        let state = match team.state.as_str() {
            "running" => TeamState::Running,
            "stopping" => TeamState::Stopping,
            _ => TeamState::Ended,
        };
        let members = members(connection, team.id, team.channel_id).await?;
        let lead_active = active(connection, team.lead_conversation).await?;
        let any_run_active = lead_active
            || members.iter().any(|member| {
                // A member that hasn't run yet is on its way: its first turn is going out or waits for a slot, the
                // lead's files or its copy.
                matches!(member.run, Run::Active { .. })
                    || (matches!(member.run, Run::None)
                        && matches!(member.state.as_str(), "starting" | "queued" | "working"))
            });
        open.push(OpenTeam {
            id: team.id,
            channel: team.channel_id,
            view: TeamView {
                state,
                deadline_at: team.deadline_at,
                deadline_minutes: u32::try_from(team.deadline_minutes.max(1)).unwrap_or(1),
                deadline_notified: team.deadline_notified,
                last_activity_at: team.last_activity_at,
                any_run_active,
                members,
            },
        });
    }
    Ok(open)
}

/// Whether the conversation's latest run is active.
async fn active(connection: &mut PgConnection, conversation: Uuid) -> Result<bool, GatewayError> {
    let latest = sqlx::query_scalar!(
        "SELECT lifecycle FROM events WHERE conversation_id = $1 AND lifecycle IS NOT NULL ORDER BY seq DESC LIMIT 1",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?
    .flatten();
    Ok(matches!(
        latest.as_deref(),
        Some("run.started" | "run.resumed")
    ))
}

/// A team's members in the order they were started, each with its run and what the lead was already told.
async fn members(
    connection: &mut PgConnection,
    team: Uuid,
    channel: Uuid,
) -> Result<Vec<MemberView>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT m.conversation_id, m.name, m.state, lc.lifecycle AS \"lifecycle?\", \
         EXTRACT(EPOCH FROM lc.created_at)::bigint AS \"lifecycle_at?\", \
         lc.frame->'event'->>'reason' AS \"failed_reason?\", \
         (SELECT EXTRACT(EPOCH FROM MAX(e.created_at))::bigint FROM events e \
          WHERE e.conversation_id = m.conversation_id AND e.seq >= lc.seq) AS \"progress_at?\", \
         EXISTS (SELECT 1 FROM channel_messages r WHERE r.channel_id = $2 AND r.type = 'report' \
                 AND r.author_id = m.conversation_id::text AND r.seq > COALESCE((SELECT MAX(a.seq) \
                 FROM channel_messages a WHERE a.channel_id = $2 AND a.type = 'assign' \
                 AND a.to_conversation = m.conversation_id), 0)) AS \"reported!\", \
         EXISTS (SELECT 1 FROM channel_messages w WHERE w.channel_id = $2 AND w.seq > m.last_read_seq \
                 AND (w.to_conversation = m.conversation_id OR w.to_kind IN ('members', 'team'))) AS \"waiting!\", \
         EXISTS (SELECT 1 FROM channel_messages n WHERE n.channel_id = $2 AND n.type = 'notice' \
                 AND n.body->>'kind' = 'member_idle' AND n.body->>'member' = m.name \
                 AND n.created_at >= lc.created_at) AS \"idle_notified!\", \
         EXISTS (SELECT 1 FROM channel_messages n WHERE n.channel_id = $2 AND n.type = 'notice' \
                 AND n.body->>'kind' = 'member_stuck' AND n.body->>'member' = m.name AND n.created_at >= \
                 (SELECT MAX(e.created_at) FROM events e WHERE e.conversation_id = m.conversation_id \
                  AND e.seq >= lc.seq)) AS \"stuck_notified!\", \
         EXISTS (SELECT 1 FROM channel_messages n WHERE n.channel_id = $2 AND n.type = 'notice' \
                 AND n.body->>'kind' = 'member_failed' AND n.body->>'member' = m.name \
                 AND n.created_at >= lc.created_at) AS \"failed_notified!\" \
         FROM agent_team_members m LEFT JOIN LATERAL (SELECT seq, lifecycle, created_at, frame FROM events \
         WHERE conversation_id = m.conversation_id AND lifecycle IS NOT NULL ORDER BY seq DESC LIMIT 1) lc ON true \
         WHERE m.team_id = $1 ORDER BY m.created_at",
        team,
        channel
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let at = row.lifecycle_at.unwrap_or_default();
        let run = match row.lifecycle.as_deref() {
            Some("run.started" | "run.resumed") => Run::Active {
                last_progress_at: row.progress_at.unwrap_or(at),
            },
            Some("run.failed") => Run::Failed {
                reason: row
                    .failed_reason
                    .unwrap_or_else(|| "no reason was recorded".into()),
            },
            Some(_) => Run::Ended { at },
            None => Run::None,
        };
        views.push(MemberView {
            name: row.name,
            state: row.state,
            run,
            reported_since_assignment: row.reported,
            waiting_for_it: row.waiting,
            told: Told {
                idle: row.idle_notified,
                stuck: row.stuck_notified,
                failed: row.failed_notified,
            },
        });
    }
    Ok(views)
}
