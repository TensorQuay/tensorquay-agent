//! Rounds in the store (ADR-0034's rounds amendment, A1 and A4): whether a lead's run may open its team's next round,
//! opening it, and closing a round's record when it ends.
use super::store::post_gateway;
use crate::GatewayError;
use crate::config::AgentTeams;
use sqlx::PgConnection;
use uuid::Uuid;

/// Whether the ended `team` ended for good, not for one of a round's reasons: its lead task was deleted or its
/// person disabled (ADR-0034's rounds amendment, A1).
///
/// # Errors
/// Storage failure.
pub(crate) async fn ended_for_good(
    connection: &mut PgConnection,
    team: Uuid,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT COALESCE(stop_reason IN ('lead_deleted', 'person_disabled'), false) AS \"for_good!\" \
         FROM agent_teams WHERE id = $1",
        team
    )
    .fetch_one(connection)
    .await?)
}

/// Whether the lead's `run` may open its ended `team`'s next round (A1, decided on 18 Nov 2026 at review): a person's
/// message started the run (the last `message.user` before its `run.started` has no team wake and no lead brief; a
/// turn too large to keep inline is a person's, since the gateway's are short), the round had ended when the run
/// started (by the database's own time), and no round was opened by this run.
///
/// # Errors
/// Storage failure.
pub(crate) async fn may_open(
    connection: &mut PgConnection,
    (lead, team): (Uuid, Uuid),
    run: &str,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        r#"WITH started AS (
               SELECT seq, created_at FROM events
               WHERE conversation_id = $1 AND run_id = $3 AND lifecycle = 'run.started'
               ORDER BY seq LIMIT 1),
           turn AS (
               SELECT e.frame FROM events e, started s
               WHERE e.conversation_id = $1 AND e.seq < s.seq
                 AND (e.frame IS NULL OR e.frame->'event'->>'type' = 'message.user')
               ORDER BY e.seq DESC LIMIT 1)
           SELECT COALESCE((
               SELECT (t.frame IS NULL
                       OR (t.frame->'event'->'team_wake' IS NULL AND t.frame->'event'->'lead_brief' IS NULL))
                  AND a.ended_at <= s.created_at
                  AND NOT EXISTS (SELECT 1 FROM agent_team_rounds r WHERE r.team_id = $2 AND r.opened_by_run = $3)
               FROM started s, turn t, agent_teams a WHERE a.id = $2), false) AS "may!""#,
        lead,
        team,
        run
    )
    .fetch_one(connection)
    .await?)
}

/// Opens the ended `team`'s next round for the lead's `run`: the team runs again with a fresh budget and deadline from
/// the limits in force (A4), the round's record, and the gateway's `round_started` message to the team before the
/// helper's `assign`.
///
/// # Errors
/// Storage failure, or the team's log over its day's messages.
pub(crate) async fn open(
    connection: &mut PgConnection,
    team: Uuid,
    run: &str,
    teams: &AgentTeams,
) -> Result<(), GatewayError> {
    // The round that ended keeps its record, whatever wrote its end.
    close(connection, team, None).await?;
    let opened = sqlx::query!(
        "UPDATE agent_teams SET state = 'running', round = round + 1, spent_tokens = 0, stop_reason = NULL, \
         ended_at = NULL, token_budget = $2, deadline_at = CURRENT_TIMESTAMP + make_interval(mins => $3) \
         WHERE id = $1 AND state = 'ended' RETURNING round, channel_id",
        team,
        i64::try_from(teams.token_budget).unwrap_or(i64::MAX),
        i32::try_from(teams.deadline_minutes).unwrap_or(i32::MAX)
    )
    .fetch_one(&mut *connection)
    .await?;
    sqlx::query!(
        "INSERT INTO agent_team_rounds (team_id, round, token_budget, deadline_at, opened_by_run) \
         SELECT id, round, token_budget, deadline_at, $2 FROM agent_teams WHERE id = $1",
        team,
        run
    )
    .execute(&mut *connection)
    .await?;
    post_gateway(
        connection,
        opened.channel_id,
        &tqa_types::TeamMessageBody::RoundStarted {
            round: u32::try_from(opened.round).unwrap_or(u32::MAX),
            text: format!("Round {} of this team started.", opened.round),
        },
    )
    .await?;
    Ok(())
}

/// Writes the team's current round's record from the team row, as the round ends (`end_seq`, its end note's seq) or
/// as the next opens: when, why, and what it spent. A round with no record yet (a team made before rounds, whose
/// round 1 the migration recorded, or one written straight to the store) gets one.
///
/// # Errors
/// Storage failure.
pub(crate) async fn close(
    connection: &mut PgConnection,
    team: Uuid,
    end_seq: Option<i64>,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "INSERT INTO agent_team_rounds (team_id, round, started_at, token_budget, deadline_at, spent_tokens, \
         ended_at, stop_reason, end_seq) \
         SELECT id, round, created_at, token_budget, deadline_at, spent_tokens, COALESCE(ended_at, CURRENT_TIMESTAMP), \
         stop_reason, $2 FROM agent_teams WHERE id = $1 \
         ON CONFLICT (team_id, round) DO UPDATE SET spent_tokens = EXCLUDED.spent_tokens, \
         ended_at = EXCLUDED.ended_at, stop_reason = EXCLUDED.stop_reason, \
         end_seq = COALESCE(EXCLUDED.end_seq, agent_team_rounds.end_seq)",
        team,
        end_seq
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// The round being ended and the tokens of every round so far, this one's included, for its end note (A4).
///
/// # Errors
/// Storage failure.
pub(crate) async fn totals(
    connection: &mut PgConnection,
    team: Uuid,
) -> Result<(u32, i64), GatewayError> {
    let row = sqlx::query!(
        "SELECT t.round, t.spent_tokens + COALESCE((SELECT SUM(r.spent_tokens) FROM agent_team_rounds r \
         WHERE r.team_id = t.id AND r.round < t.round), 0)::bigint AS \"all!\" FROM agent_teams t WHERE t.id = $1",
        team
    )
    .fetch_one(connection)
    .await?;
    Ok((u32::try_from(row.round).unwrap_or(1), row.all))
}
