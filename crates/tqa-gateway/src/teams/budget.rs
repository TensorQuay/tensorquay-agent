//! An agent team's budget, enforced by the gateway at the relay (ADR-0034 point 8, slice 5). The team's tokens are the
//! prompt and completion tokens of its lead's and members' model calls from the first `start_member` on, as the relay
//! meters them: the runtime reports nothing this trusts. Before a call is forwarded it is refused once the budget or
//! the deadline is spent (`team_budget`, everyone's), or, a member's, while the team stops for another reason
//! (`team_stopped`): the lead's own calls pass then, so it can finish its answer after `done` (4 Oct 2026). Each
//! answer's tokens are added in the transaction that records them, the lead is told once at 80 %, and at 100 % the team
//! stops. An ended team's tasks are ordinary tasks again, charged nothing to it. Off with
//! `[agent_teams] enabled = false`: no query at all.
use crate::{GatewayError, app::State};
use sqlx::PgConnection;
use tqa_types::{ConversationId, NoticeKind};
use uuid::Uuid;

/// The notices at 80 %, in the gateway's fixed words: the lead is asked to collect the work and each working helper to
/// report it now (GW-I-304, c16k B9b: a budget end handed nothing back). The lead's doesn't wake it while a helper works
/// (`wake.rs`): it reads it with their reports, so it doesn't finish before they come (the 16n review).
const AT_80: &str = "The team has used 80 % of its budget. Each helper was asked to report what it has now. As their \
                     reports come, take their files, then finish with what you have.";
const AT_80_HELPER: &str = "The team has used 80 % of its budget. Report to your lead now with what you have and any \
                            files, then carry on.";

/// Refuses a model call from a lead or member of a team that may not spend now: `team_budget` from everyone once its
/// budget or deadline is spent (or it stops for either). While it stops for any other reason (`done`, a person's stop,
/// idle, a deleted lead or a disabled person) the lead's own calls pass, so it can finish its answer to the person,
/// and a member's are `team_stopped`. A task in no open team (an ended team's lead and members are ordinary tasks
/// again), and every task while agent teams are off, passes.
///
/// # Errors
/// `409 team_budget`, `409 team_stopped`, a store failure.
pub(crate) async fn check(
    state: &State,
    conversation: &ConversationId,
) -> Result<(), GatewayError> {
    if !state.config.agent_teams.enabled {
        return Ok(());
    }
    let id = crate::conversations::uuid(conversation)?;
    let team = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT t.state, t.stop_reason, t.spent_tokens >= t.token_budget AS \"spent!\", \
             t.spent_tokens * 100 >= t.token_budget * $2 AS \"helpers_out!\", \
             t.deadline_at <= CURRENT_TIMESTAMP AS \"late!\", t.lead_conversation = $1 AS \"lead!\" \
             FROM agent_teams t WHERE t.state <> 'ended' \
             AND (t.lead_conversation = $1 OR EXISTS (SELECT 1 FROM agent_team_members m \
             WHERE m.team_id = t.id AND m.conversation_id = $1))",
            id,
            i64::from(state.config.agent_teams.helpers_stop_percent)
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    let Some(team) = team else {
        return Ok(());
    };
    match team.state.as_str() {
        // The whole budget or the deadline first: everyone's refusal.
        "running" if team.spent || team.late => Err(GatewayError::Conflict("team_budget")),
        // Past the helpers' share the rest is the lead's, to write up (GW-I-304); a code of its own, so a helper's page
        // says what happened to it (c16k B9b-3).
        "running" if !team.lead && team.helpers_out => {
            Err(GatewayError::Conflict("team_budget_helpers"))
        }
        "running" => Ok(()),
        _ if matches!(team.stop_reason.as_deref(), Some("budget" | "deadline")) => {
            Err(GatewayError::Conflict("team_budget"))
        }
        // `done` exists for the lead's closing answer; its helpers are being stopped.
        _ if team.lead => Ok(()),
        _ => Err(GatewayError::Conflict("team_stopped")),
    }
}

/// Adds `tokens` to the open team of `conversation`, in the caller's transaction (the one that records them). Crossing
/// 80 % tells the lead, once: only the spend that crosses it does. Reaching the budget stops a running team
/// (`budget`; the shared stop, slice 4, takes over from here). Answers what to do after the commit.
///
/// # Errors
/// A store failure.
pub(crate) async fn spend(
    connection: &mut PgConnection,
    conversation: Uuid,
    tokens: i64,
    helpers_percent: u32,
) -> Result<Spent, GatewayError> {
    if tokens <= 0 {
        return Ok(Spent::default());
    }
    let Some(team) = sqlx::query!(
        "UPDATE agent_teams t SET spent_tokens = t.spent_tokens + $2 WHERE t.state <> 'ended' \
         AND (t.lead_conversation = $1 OR EXISTS (SELECT 1 FROM agent_team_members m \
         WHERE m.team_id = t.id AND m.conversation_id = $1)) \
         RETURNING t.id, t.channel_id, t.state, t.spent_tokens, t.token_budget",
        conversation,
        tokens
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(Spent::default());
    };
    let before = team.spent_tokens - tokens;
    let mut published = None;
    let mut helpers_stopped = None;
    // Whole numbers: crossed when before < 0.8 B <= after.
    if crossed(before, team.spent_tokens, team.token_budget, 80) {
        let mut seq =
            super::store::team_notice(connection, team.channel_id, NoticeKind::Budget80, AT_80)
                .await?;
        // Helpers already past their share are refused at their next call: none is asked for a report it can't send.
        let asked = if i128::from(team.spent_tokens) * 100
            < i128::from(team.token_budget) * i128::from(helpers_percent)
        {
            sqlx::query_scalar!(
                "SELECT conversation_id FROM agent_team_members WHERE team_id = $1 \
                 AND state NOT IN ('done', 'stopped', 'failed') ORDER BY name",
                team.id
            )
            .fetch_all(&mut *connection)
            .await?
        } else {
            Vec::new()
        };
        for member in asked {
            seq = super::store::member_notice(
                connection,
                team.channel_id,
                member,
                NoticeKind::Budget80,
                AT_80_HELPER,
            )
            .await?;
        }
        published = Some((team.channel_id, seq));
    }
    if crossed(
        before,
        team.spent_tokens,
        team.token_budget,
        helpers_percent,
    ) {
        published = Some((
            team.channel_id,
            helpers_out(connection, team.channel_id, helpers_percent).await?,
        ));
        helpers_stopped = Some(team.id);
    }
    let mut stopped = false;
    if team.spent_tokens >= team.token_budget && team.state == "running" {
        stopped = sqlx::query!(
            "UPDATE agent_teams SET state = 'stopping', stop_reason = 'budget' WHERE id = $1 AND state = 'running'",
            team.id
        )
        .execute(&mut *connection)
        .await?
        .rows_affected()
            > 0;
    }
    Ok(Spent {
        published,
        stopped,
        helpers_stopped,
    })
}

/// Tells the lead its helpers stop at their share of the budget; answers the notice's seq.
async fn helpers_out(
    connection: &mut PgConnection,
    channel: Uuid,
    helpers_percent: u32,
) -> Result<u64, GatewayError> {
    let text = format!(
        "Your helpers stop at their next step: the team is at {helpers_percent} % of its budget. Write up what you \
         have; the team stops at 100 %."
    );
    super::store::team_notice(connection, channel, NoticeKind::BudgetHelpersStopped, &text).await
}

/// What a spend leaves for after its commit.
#[derive(Default)]
pub(crate) struct Spent {
    /// The 80 % notice's channel and seq, to publish.
    pub(crate) published: Option<(Uuid, u64)>,
    /// The team started stopping (`budget`): the reconciler is nudged to carry the stop out (GW-I-261).
    pub(crate) stopped: bool,
    /// The team reached the helpers' share: they are stopped and handed back after the commit (GW-I-311).
    pub(crate) helpers_stopped: Option<Uuid>,
}

/// Whether a spend from `before` to `after` crosses `percent` % of `budget`.
pub(crate) fn crossed(before: i64, after: i64, budget: i64, percent: u32) -> bool {
    let line = i128::from(budget) * i128::from(percent);
    i128::from(before) * 100 < line && i128::from(after) * 100 >= line
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
