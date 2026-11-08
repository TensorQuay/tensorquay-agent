//! Stopping an agent team (ADR-0034 point 10): the one path for every reason. Whatever started it (the lead's `done`,
//! the budget, the deadline, the idle stop, a person's Stop, the lead's task deleted or its person disabled) only sets
//! `stopping` with its reason; [`finish`] carries a stopping team to `ended`. It cancels each member's active run
//! through the runtime link as a person's cancel does, marks the members still at work `stopped`, stores the gateway's
//! end note to the lead (a team message of type `ended`: the lead's decision of 2 Oct 2026) and ends the team, the note
//! and the end in one transaction. Every step is idempotent, so a restart part way finishes it on the next pass.
use crate::{GatewayError, app::State};
use sqlx::PgConnection;
use tqa_types::{
    ClientFrame, ConversationId, EndFile, EndReason, EndReport, EndTokens, MemberTokens,
    TeamMessageBody,
};
use uuid::Uuid;

/// The longest file list the note's sentences name before "and N more"; the parts beside the text keep every one.
const NAMED_FILES: usize = 20;

/// One storage step of a stop, bounded as every gateway wait is (ENGINEERING-GUARDRAILS): a stalled database fails the
/// step, and the next pass tries again, instead of holding the reconciler.
pub(super) async fn step<T>(
    state: &State,
    work: impl Future<Output = Result<T, GatewayError>>,
) -> Result<T, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, work).await
}

/// A disabled person's running teams start stopping (`person_disabled`), in the caller's transaction, whether agent
/// teams are on or not: the teams, for [`finish`] once it commits (the reconciler finishes any it can't).
///
/// # Errors
/// Storage failure.
pub(crate) async fn person_disabled(
    connection: &mut PgConnection,
    person: Uuid,
) -> Result<Vec<Uuid>, GatewayError> {
    Ok(sqlx::query_scalar!(
        "UPDATE agent_teams SET state = 'stopping', stop_reason = 'person_disabled' \
         WHERE owner_id = $1 AND state = 'running' RETURNING id",
        person
    )
    .fetch_all(&mut *connection)
    .await?)
}

/// Carries a stopping team to `ended`; a team in any other state is left as it is. Answers whether it ended now. Each
/// step takes its own connection and gives it back, so none is held across the members' cancels.
///
/// # Errors
/// Storage failure, or a step past its bound.
pub(crate) async fn finish(state: &State, team: Uuid) -> Result<bool, GatewayError> {
    let row = step(state, async {
        Ok(sqlx::query!(
            "SELECT channel_id, state, stop_reason FROM agent_teams WHERE id = $1",
            team
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    let Some(row) = row.filter(|row| row.state == "stopping") else {
        return Ok(false);
    };
    let reason = reason(row.stop_reason.as_deref());
    // The helpers the end stops, read before their runs go: each that hasn't reported is handed back (GW-I-311).
    let stopping = stopping(state, team).await?;
    // A person's Stop team stops the lead's own run too (real-world use before 17.0: it worked on alone); every other
    // reason leaves it to read the end note (after `done`, its closing words).
    cancel_members(state, team, reason == EndReason::Stopped).await?;
    // Before the note, which lists the files handed back.
    hand_back(state, team, &stopping, reason).await;
    let body = step(state, async {
        let mut connection = state.pool.acquire().await?;
        note(
            &mut connection,
            team,
            row.channel_id,
            reason,
            state.settings.current().limits.team_idle_stop_minutes,
        )
        .await
    })
    .await?;
    // `None` when another pass ended it first (its note is the one that stays); else the note's seq, if it was stored.
    let ended = step(state, async {
        let mut transaction = state.pool.begin().await?;
        end_members(&mut transaction, team).await?;
        let seq = match super::store::end_note(&mut transaction, row.channel_id, &body).await {
            Ok(seq) => Some(seq),
            // A stop always finishes: a log over its day's messages ends without the note rather than stay stopping.
            Err(GatewayError::ChannelRate(_)) => {
                tracing::warn!(team = %team, "the agent team's end note wasn't stored: its log is over its day's messages");
                None
            }
            Err(error) => return Err(error),
        };
        let changed = sqlx::query!(
            "UPDATE agent_teams SET state = 'ended', ended_at = CURRENT_TIMESTAMP WHERE id = $1 AND state = 'stopping'",
            team
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            transaction.rollback().await?;
            return Ok(None);
        }
        transaction.commit().await?;
        Ok(Some(seq))
    })
    .await?;
    let Some(seq) = ended else {
        return Ok(false);
    };
    // Again now that its helpers are `stopped`: a first turn sent between the cancels above and that commit (a copy
    // that finished just then) is dropped too, and none can be sent after it.
    let dropped = step(state, async {
        let mut connection = state.pool.acquire().await?;
        let conversations = sqlx::query_scalar!(
            "SELECT conversation_id FROM agent_team_members WHERE team_id = $1",
            team
        )
        .fetch_all(&mut *connection)
        .await?;
        drop_queued_turns(&mut connection, &conversations).await
    })
    .await?;
    withdraw(state, &dropped).await;
    if let Some(seq) = seq {
        state.channels.published(row.channel_id, seq);
    }
    Ok(true)
}

/// The helpers an end stops: neither done nor already stopped.
async fn stopping(state: &State, team: Uuid) -> Result<Vec<Uuid>, GatewayError> {
    step(state, async {
        Ok(sqlx::query_scalar!(
            "SELECT conversation_id FROM agent_team_members WHERE team_id = $1 AND state NOT IN ('done', 'stopped')",
            team
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await
}

/// Hands back what each of `stopping` that hasn't reported changed (GW-I-311); a failed hand-back never holds the end.
async fn hand_back(state: &State, team: Uuid, stopping: &[Uuid], reason: EndReason) {
    let why = super::hand_back::Why::End(reason);
    match super::hand_back::hand_back(state, team, stopping, why).await {
        Ok(Some((channel, seq))) => state.channels.published(channel, seq),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(team = %team, code = error.code(), "a stopped helper's work wasn't handed back");
        }
    }
}

/// Ends each helper still at work: a helper that reported since its last assignment is done (its own report, not the
/// gateway's hand-back, GW-I-311), as a run that ends after a
/// report makes it (`member_state.rs`), so its cancelled run's event, which comes after this and no longer changes it,
/// can't leave it stopped (GW-I-301, real-world use before 17.0: one finishing its turn when the lead called done
/// showed Stopped, like a failure). Any other is stopped.
async fn end_members(connection: &mut sqlx::PgConnection, team: Uuid) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE agent_team_members m SET state = CASE WHEN EXISTS (SELECT 1 FROM agent_teams t \
         JOIN channel_messages r ON r.channel_id = t.channel_id WHERE t.id = m.team_id AND r.type = 'report' \
         AND r.author_kind = 'conversation' AND r.author_id = m.conversation_id::text AND r.seq > COALESCE((SELECT MAX(a.seq) \
         FROM channel_messages a WHERE a.channel_id = t.channel_id AND a.type = 'assign' \
         AND a.to_conversation = m.conversation_id), 0)) THEN 'done' ELSE 'stopped' END \
         WHERE m.team_id = $1 AND m.state NOT IN ('done', 'stopped', 'failed')",
        team
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Cancels each member's active run through its runtime link, as a person's cancel does; a member with no link now is
/// left to the sandbox backstop, as the quota stop leaves it. The lead's queued wake goes too: the end note reaches it
/// on its next run's role instead. With `lead_too`, the lead's own active run is cancelled as well.
async fn cancel_members(state: &State, team: Uuid, lead_too: bool) -> Result<(), GatewayError> {
    let members = step(state, async {
        let mut connection = state.pool.acquire().await?;
        let members = sqlx::query_scalar!(
            "SELECT conversation_id FROM agent_team_members WHERE team_id = $1 AND state NOT IN ('done', 'stopped', 'failed')",
            team
        )
        .fetch_all(&mut *connection)
        .await?;
        let lead = sqlx::query_scalar!(
            "SELECT lead_conversation FROM agent_teams WHERE id = $1",
            team
        )
        .fetch_one(&mut *connection)
        .await?;
        let dropped = drop_queued_turns(&mut connection, &[lead]).await?;
        let mut members = members;
        if lead_too {
            members.extend([lead]);
        }
        Ok((members, dropped))
    })
    .await?;
    let (members, dropped) = members;
    withdraw(state, &dropped).await;
    // Together: each cancel is bounded, so a team's stop waits about one bound however many links are slow (review L2).
    let cancels = futures::future::join_all(
        members
            .into_iter()
            .map(|member| cancel_member(state, member)),
    );
    for cancelled in cancels.await {
        cancelled?;
    }
    Ok(())
}

/// Cancels `member`'s active run through the runtime link, as a person's cancel does, and drops its queued team turns
/// (a wake, or its first turn not yet started), so a stopped helper doesn't run again for its team. A cancel that can't
/// be delivered is logged: the run then ends on its own, and its tools answer that it was stopped.
///
/// # Errors
/// Storage failure, or a step past its bound.
pub(crate) async fn cancel_member(state: &State, member: Uuid) -> Result<(), GatewayError> {
    let (dropped, run) = step(state, async {
        let mut connection = state.pool.acquire().await?;
        let dropped = drop_queued_turns(&mut connection, &[member]).await?;
        Ok((
            dropped,
            super::store::current_run(&mut connection, member).await?,
        ))
    })
    .await?;
    withdraw(state, &dropped).await;
    let Some(run) = run else {
        return Ok(());
    };
    let (Ok(conversation), Ok(run_id)) = (
        member.to_string().parse::<ConversationId>(),
        run.parse::<tqa_types::RunId>(),
    ) else {
        return Ok(());
    };
    let cancel = ClientFrame::RunCancel { run_id };
    if let Err(error) = step(state, state.links.deliver(&conversation, cancel)).await {
        tracing::debug!(conversation_id = %member, error_code = error.code(), "a member's cancel was not delivered");
    }
    Ok(())
}

/// Drops the team turns still queued for `conversations`: their wakes (ADR-0034 point 10: stopping drops queued
/// wakes) and a helper's first turn not yet started (`team-start:`), so a helper stopped while it waited for a slot, the
/// lead's lease or its copy never starts with the lead's brief after the stop. A person's own queued messages stay.
/// Answers each dropped turn's task and whether the turn was its first, for [`withdraw`] once the drop has committed.
async fn drop_queued_turns(
    connection: &mut PgConnection,
    conversations: &[Uuid],
) -> Result<Vec<(Uuid, bool)>, GatewayError> {
    let rows = sqlx::query!(
        "UPDATE idempotency SET execution_status = 'cancelled', updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ANY($1) AND execution_status = 'queued' \
         AND (key LIKE 'team-wake:%' OR key LIKE 'team-start:%') \
         RETURNING conversation_id AS \"conversation_id!\", key LIKE 'team-start:%' AS \"first!\"",
        conversations
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.conversation_id, row.first))
        .collect())
}

/// After [`drop_queued_turns`] committed: each task whose turns it dropped leaves the queue on its open pages, as a
/// person's cancel withdraws it (GW-I-173), or a page went on reading it waiting until reloaded (a live check
/// before 17.0, GW-I-312). A helper whose first turn it dropped, and that never ran, has its sandbox retired, started
/// or still starting: it held a slot and the workspace's lease for its whole idle time. A failed notice or retirement
/// is logged; the idle timeout still ends the sandbox.
async fn withdraw(state: &State, dropped: &[(Uuid, bool)]) {
    let mut done: Vec<Uuid> = Vec::new();
    for &(conversation, _) in dropped {
        if done.contains(&conversation) {
            continue;
        }
        done.push(conversation);
        let Ok(id) = conversation.to_string().parse::<ConversationId>() else {
            continue;
        };
        if let Err(error) = state.manager.withdrawn(&id).await {
            tracing::warn!(conversation_id = %id, code = error.code(), "slot queue notice not delivered");
        }
        if let Err(error) =
            crate::actions::held::publish(&state.pool, &state.events, &id, None).await
        {
            tracing::warn!(conversation_id = %id, code = error.code(), "held messages notice not delivered");
        }
        let first = dropped
            .iter()
            .any(|&(of, first)| of == conversation && first);
        if first
            && never_ran(state, conversation).await
            && let Err(error) = state
                .manager
                .retire_for_switch(&id, crate::manager::Teardown::TeamStopped)
        {
            tracing::warn!(conversation_id = %id, code = error.code(), "a stopped helper's sandbox wasn't retired");
        }
    }
}

/// Whether `conversation` never started a run; unknown on a storage failure, so its sandbox is left to idle out.
async fn never_ran(state: &State, conversation: Uuid) -> bool {
    step(state, async {
        Ok(sqlx::query_scalar!(
            "SELECT NOT EXISTS(SELECT 1 FROM events WHERE conversation_id = $1 \
             AND frame->'event'->>'type' = 'run.started') AS \"never!\"",
            conversation
        )
        .fetch_one(&state.pool)
        .await?)
    })
    .await
    .unwrap_or(false)
}

/// The stop reason as the note names it; an unknown one reads as a person's Stop.
fn reason(stored: Option<&str>) -> EndReason {
    match stored {
        Some("done") => EndReason::Done,
        Some("budget") => EndReason::Budget,
        Some("deadline") => EndReason::Deadline,
        Some("idle") => EndReason::Idle,
        Some("lead_deleted") => EndReason::LeadDeleted,
        Some("person_disabled") => EndReason::PersonDisabled,
        _ => EndReason::Stopped,
    }
}

/// The end note's parts, read from the team's log and the relay's usage, and its sentences.
async fn note(
    connection: &mut PgConnection,
    team: Uuid,
    channel: Uuid,
    reason: EndReason,
    idle_minutes: u32,
) -> Result<TeamMessageBody, GatewayError> {
    let tokens = tokens(connection, team).await?;
    let (reports, files) = reports(connection, channel).await?;
    let summary = if reason == EndReason::Done {
        sqlx::query_scalar!(
            "SELECT body->>'text' FROM channel_messages WHERE channel_id = $1 AND type = 'done' ORDER BY seq DESC LIMIT 1",
            channel
        )
        .fetch_optional(&mut *connection)
        .await?
        .flatten()
    } else {
        None
    };
    let parts = Parts {
        reason,
        summary: summary.as_deref(),
        reports: &reports,
        files: &files,
    };
    let text = sentences(&parts, idle_minutes);
    Ok(TeamMessageBody::Ended {
        reason,
        summary,
        tokens,
        reports,
        files,
        text,
    })
}

/// The team's tokens: the budget's total, and each member's from the relay's usage since the team started.
async fn tokens(connection: &mut PgConnection, team: Uuid) -> Result<EndTokens, GatewayError> {
    let total = sqlx::query_scalar!("SELECT spent_tokens FROM agent_teams WHERE id = $1", team)
        .fetch_one(&mut *connection)
        .await?;
    let members = sqlx::query!(
        "SELECT m.name, COALESCE(SUM(COALESCE(u.prompt_tokens, 0) + COALESCE(u.completion_tokens, 0)), 0)::bigint \
         AS \"tokens!\" FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
         LEFT JOIN usage u ON u.conversation_id = m.conversation_id AND u.created_at >= t.created_at \
         WHERE m.team_id = $1 GROUP BY m.name, m.created_at ORDER BY m.created_at",
        team
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(EndTokens {
        total,
        members: members
            .into_iter()
            .map(|member| MemberTokens {
                name: member.name,
                tokens: member.tokens,
            })
            .collect(),
    })
}

/// Every report in the team's log, and the files each handed back (what slice 5 records: name and size).
async fn reports(
    connection: &mut PgConnection,
    channel: Uuid,
) -> Result<(Vec<EndReport>, Vec<EndFile>), GatewayError> {
    let rows = sqlx::query!(
        "SELECT seq, author_name, body->'files' AS files FROM channel_messages \
         WHERE channel_id = $1 AND type = 'report' ORDER BY seq",
        channel
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut reports = Vec::new();
    let mut files = Vec::new();
    for row in rows {
        let member = row.author_name.unwrap_or_default();
        let seq = u64::try_from(row.seq).unwrap_or_default();
        let handed = row
            .files
            .as_ref()
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten();
        for file in handed {
            if let (Some(name), Some(size)) = (file["name"].as_str(), file["size"].as_u64()) {
                files.push(EndFile {
                    member: member.clone(),
                    seq,
                    name: name.to_owned(),
                    size,
                });
            }
        }
        reports.push(EndReport { member, seq });
    }
    Ok((reports, files))
}

/// The parts the note's sentences name; the tokens stay in the body's parts, never in its words (PRD §12 item 11).
pub(crate) struct Parts<'a> {
    pub reason: EndReason,
    pub summary: Option<&'a str>,
    pub reports: &'a [EndReport],
    pub files: &'a [EndFile],
}

/// The note in the gateway's fixed sentences (UI-GUIDELINES copy: plain words, the facts, no jargon).
pub(crate) fn sentences(parts: &Parts<'_>, idle_minutes: u32) -> String {
    let Parts {
        reason,
        summary,
        reports,
        files,
    } = *parts;
    let why = match reason {
        EndReason::Done => "the lead said the work is done".to_owned(),
        EndReason::Stopped => "a person stopped it".to_owned(),
        EndReason::Budget => "it used its whole budget".to_owned(),
        EndReason::Deadline => "its time limit passed".to_owned(),
        EndReason::Idle => format!("nothing happened in it for {idle_minutes} minutes"),
        EndReason::LeadDeleted => "the lead task was deleted".to_owned(),
        EndReason::PersonDisabled => "its person's account was turned off".to_owned(),
    };
    let mut lines = vec![format!("The team stopped: {why}.")];
    if let Some(summary) = summary.filter(|summary| !summary.trim().is_empty()) {
        lines.push(format!("The lead's summary: {summary}"));
    }
    lines.push(if reports.is_empty() {
        "No helper sent a report.".to_owned()
    } else {
        let mut named: Vec<(String, Vec<u64>)> = Vec::new();
        for report in reports {
            match named
                .iter_mut()
                .find(|(member, _)| *member == report.member)
            {
                Some((_, seqs)) => seqs.push(report.seq),
                None => named.push((report.member.clone(), vec![report.seq])),
            }
        }
        let parts: Vec<String> = named
            .into_iter()
            .map(|(member, seqs)| {
                let list: Vec<String> = seqs.iter().map(u64::to_string).collect();
                format!(
                    "{member} (message{} {})",
                    if list.len() == 1 { "" } else { "s" },
                    list.join(", ")
                )
            })
            .collect();
        format!("Reports: {}.", parts.join("; "))
    });
    lines.push(if files.is_empty() {
        "No files were handed back.".to_owned()
    } else {
        let mut parts: Vec<String> = files
            .iter()
            .take(NAMED_FILES)
            .map(|file| format!("{} from {} ({})", file.name, file.member, bytes(file.size)))
            .collect();
        if files.len() > NAMED_FILES {
            parts.push(format!("and {} more", files.len() - NAMED_FILES));
        }
        format!("Files handed back: {}.", parts.join(", "))
    });
    lines.push("Every task of the team is an ordinary task again.".to_owned());
    lines.join("\n")
}

/// A size in plain units, to one decimal, rounded, without a trailing `.0` (as the web's sizes read: 2 KB, 1.2 KB).
pub(super) fn bytes(size: u64) -> String {
    let tenths = |unit: u64| size.saturating_mul(10).saturating_add(unit / 2) / unit;
    let (tenths, unit) = match size {
        0..1024 => return format!("{size} bytes"),
        1024..1_048_576 => (tenths(1024), "KB"),
        _ => (tenths(1_048_576), "MB"),
    };
    if tenths % 10 == 0 {
        format!("{} {unit}", tenths / 10)
    } else {
        format!("{}.{} {unit}", tenths / 10, tenths % 10)
    }
}

#[cfg(test)]
#[path = "stop_tests.rs"]
mod tests;
