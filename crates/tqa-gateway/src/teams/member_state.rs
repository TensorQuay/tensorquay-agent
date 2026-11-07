//! A helper's stored state from its runs (ADR-0034 point 9, the member states of 4 Oct 2026). Its first turn sets
//! `queued` or `working` (`first_turn`); from then on its runs do, in the transaction that stores each run's start or
//! end: a run started or resumed makes it `working`; a run that ends makes it `done` when it reported since its last
//! assignment, else `waiting` for its lead. A failed run makes it `failed` with `failed_run` set (E2E-AGENTS-021): it
//! keeps its seat and the lead's next message wakes it, and its next run makes it `working` again. `stuck` stays the
//! reconciler's, `stopped` the stop's, and a `failed` without `failed_run` (a helper that couldn't start) the first
//! turn's.
use crate::GatewayError;
use sqlx::PgConnection;
use tqa_types::NoticeKind;
use uuid::Uuid;

/// What the run `lifecycle` event (`run.started`, `run.completed`, …) of `conversation` makes of its member, when the
/// conversation is one; nothing for any other conversation or event.
///
/// # Errors
/// Storage failure.
pub(crate) async fn on_lifecycle(
    connection: &mut PgConnection,
    conversation: Uuid,
    lifecycle: &str,
) -> Result<(), GatewayError> {
    match lifecycle {
        // At work again: a first run, a woken one, or one resumed after a restart.
        "run.started" | "run.resumed" => {
            sqlx::query!(
                "UPDATE agent_team_members SET state = 'working', failed_run = false WHERE conversation_id = $1 \
                 AND (state IN ('starting', 'queued', 'waiting', 'done') OR (state = 'failed' AND failed_run))",
                conversation
            )
            .execute(&mut *connection)
            .await?;
        }
        // A lead's turn that its model ended while its team runs, without done: reminded (GW-I-306). A cancel is a
        // person's, who meant it; it isn't reminded.
        "run.completed" => {
            ended(connection, conversation).await?;
            remind_lead(connection, conversation).await?;
            helper_quiet(connection, conversation).await?;
        }
        "run.cancelled" => {
            ended(connection, conversation).await?;
            helper_quiet(connection, conversation).await?;
        }

        // Failed: shown as such until its next run, which the lead's next message starts. From the states a given-up
        // sandbox fails it from too (`sandbox_failed`): a run that fails before its run.started is stored finds the
        // helper still queued or waiting.
        "run.failed" => {
            sqlx::query!(
                "UPDATE agent_team_members SET state = 'failed', failed_run = true, failures = failures + 1 \
                 WHERE conversation_id = $1 AND state IN ('queued', 'working', 'stuck', 'waiting')",
                conversation
            )
            .execute(&mut *connection)
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// A run's end for a member: done with what it was given if it reported since its last assignment, else waiting for its
/// lead.
async fn ended(connection: &mut PgConnection, conversation: Uuid) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE agent_team_members m SET state = CASE WHEN EXISTS (SELECT 1 FROM agent_teams t \
         JOIN channel_messages r ON r.channel_id = t.channel_id WHERE t.id = m.team_id AND r.type = 'report' \
         AND r.author_kind = 'conversation' AND r.author_id = m.conversation_id::text AND r.seq > COALESCE((SELECT MAX(a.seq) \
         FROM channel_messages a WHERE a.channel_id = t.channel_id AND a.type = 'assign' \
         AND a.to_conversation = m.conversation_id), 0)) THEN 'done' ELSE 'waiting' END \
         WHERE m.conversation_id = $1 AND m.state IN ('working', 'stuck')",
        conversation
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// The reminder a lead gets when its turn ends without done while its team runs (GW-I-306, c16k B2: the page said
/// Done, the team ran on and the finish check never ran).
const REMINDER: &str = "Your team is still running and your turn ended without done. If the work is finished, call \
                        done: your finish check runs then. Otherwise keep leading: read your helpers or assign.";
/// At most this many unanswered reminders: a lead that ignores them can't loop (its own post resets the count).
const REMINDERS: i64 = 2;

/// Reminds `conversation`'s lead, when it leads a running team and nothing else can wake it: no helper queued, starting
/// or working (each will run and report, and its report wakes the lead; the read tool tells a lead to end its turn and
/// wait for it, 16n integration), no question to its person still open (the answer wakes it), no turn on its way to it
/// (queued, sending, or delivered since its last run event and not yet started: it starts the lead anyway, as the
/// wake's own guard has it), and nothing unread but the gateway's own reminders (a wake brings it back for anything
/// else); and fewer than [`REMINDERS`] reminders since its own last post.
async fn remind_lead(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<(), GatewayError> {
    let Some(team) = sqlx::query!(
        "SELECT t.channel_id, \
         EXISTS (SELECT 1 FROM channel_messages m WHERE m.channel_id = t.channel_id AND m.seq > t.lead_last_read_seq \
         AND m.to_kind IN ('lead', 'team') AND NOT (m.type = 'notice' AND m.body->>'kind' = 'lead_unfinished') \
         AND NOT (m.type = 'person_message' AND m.body->>'member' = '' AND NOT EXISTS (SELECT 1 FROM idempotency i \
         WHERE i.team_copy_channel = m.channel_id AND i.team_copy_seq = m.seq \
         AND i.execution_status IN ('queued', 'delivered')))) \
         AS \"unread!\", \
         (EXISTS (SELECT 1 FROM agent_team_members w WHERE w.team_id = t.id \
         AND w.state IN ('queued', 'starting', 'working')) \
         OR EXISTS (SELECT 1 FROM channel_messages q WHERE q.channel_id = t.channel_id AND q.type = 'person_question' \
         AND NOT EXISTS (SELECT 1 FROM channel_messages a WHERE a.channel_id = q.channel_id \
         AND a.type = 'person_answer' AND a.body->>'question_seq' = q.seq::text)) \
         OR EXISTS (SELECT 1 FROM idempotency i WHERE i.conversation_id = t.lead_conversation \
         AND i.team_copy_seq IS NULL AND (i.execution_status IN ('queued', 'sending') \
         OR (i.execution_status = 'delivered' AND i.updated_at > COALESCE((SELECT MAX(e.created_at) FROM events e \
         WHERE e.conversation_id = t.lead_conversation AND e.lifecycle IS NOT NULL), '-infinity'::timestamptz))))) \
         AS \"busy!\", \
         (SELECT count(*) FROM channel_messages m WHERE m.channel_id = t.channel_id AND m.type = 'notice' \
         AND m.body->>'kind' = 'lead_unfinished' AND m.seq > COALESCE((SELECT MAX(o.seq) FROM channel_messages o \
         WHERE o.channel_id = t.channel_id AND o.author_id = t.lead_conversation::text), 0)) AS \"reminded!\" \
         FROM agent_teams t WHERE t.lead_conversation = $1 AND t.state = 'running'",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(());
    };
    if team.busy || team.unread || team.reminded >= REMINDERS {
        return Ok(());
    }
    super::store::team_notice(
        connection,
        team.channel_id,
        NoticeKind::LeadUnfinished,
        REMINDER,
    )
    .await?;
    Ok(())
}

/// A helper's run ended: it may have been the last one that could wake its lead (a lead that ended its turn waiting
/// for a report, as the read tool tells it to, and read it while the helper finished its own turn). So a lead whose
/// last run its model ended, not one the person cancelled, is checked for the reminder now ([`remind_lead`]).
async fn helper_quiet(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<(), GatewayError> {
    let Some(lead_task) = sqlx::query_scalar!(
        "SELECT t.lead_conversation FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
         WHERE m.conversation_id = $1 AND t.state = 'running'",
        conversation
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(());
    };
    let latest = sqlx::query_scalar!(
        "SELECT lifecycle FROM events WHERE conversation_id = $1 AND lifecycle IS NOT NULL ORDER BY seq DESC LIMIT 1",
        lead_task
    )
    .fetch_optional(&mut *connection)
    .await?
    .flatten();
    if latest.as_deref() == Some("run.completed") {
        remind_lead(connection, lead_task).await?;
    }
    Ok(())
}

/// A helper whose sandbox the manager gave up on: it stopped `losses` times in a few minutes and wasn't restarted. Its
/// run stays unfinished, so to the reconciler it still looks at work, and the lead heard nothing until it seemed stuck
/// 15 minutes later. The lead is told now that it failed, in words; a message to it starts it again (GW-I-254).
/// Nothing for a task in no running team.
///
/// # Errors
/// Storage failure.
pub(crate) async fn sandbox_failed(
    pool: &sqlx::PgPool,
    conversation: Uuid,
    losses: u32,
) -> Result<(), GatewayError> {
    let mut transaction = pool.begin().await?;
    let helper = sqlx::query!(
        "SELECT m.name, t.channel_id FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
         WHERE m.conversation_id = $1 AND t.state = 'running' AND m.state <> 'stopped'",
        conversation
    )
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(helper) = helper else {
        return Ok(());
    };
    // Failed as a failed run is: the lead's next message starts it again (E2E-AGENTS-021). Each give-up counts once, so
    // that message's wake is a new one: once by control flow, not by a uniqueness guard, since this function's only
    // caller is `recovery::stop_replacing`, which runs once per give-up and stops that sandbox's worker, and an error
    // here is logged, never retried. A helper given up again before any run event is stored (already `failed`) counts too.
    sqlx::query!(
        "UPDATE agent_team_members SET state = 'failed', failed_run = true, failures = failures + 1 \
         WHERE conversation_id = $1 AND (state IN ('queued', 'working', 'stuck', 'waiting') \
         OR (state = 'failed' AND failed_run))",
        conversation
    )
    .execute(&mut *transaction)
    .await?;
    let text = format!(
        "{} failed: its sandbox stopped {losses} times in a few minutes, so it was not restarted. A message to it \
         starts it again.",
        helper.name
    );
    super::store::notice(
        &mut transaction,
        helper.channel_id,
        (tqa_types::NoticeKind::MemberFailed, &helper.name),
        &text,
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}
