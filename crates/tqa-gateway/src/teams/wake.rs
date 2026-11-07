//! Waking (ADR-0034, the S1 contract's Delivery): a message for a conversation whose run isn't active queues a run on
//! that conversation, through admission, with a fixed sentence and no model tokens spent on the way. The key is the
//! reader's place in the log, so several messages before it reads make one wake, two posts at once make one too, and a
//! woken run that ends without reading isn't woken again for the same messages: nothing retries in a loop. A failed
//! run is the exception: it leaves the reader's place where it was, so after one the key also names the failure, and
//! the next message wakes it once more (E2E-AGENTS-021). Only a running team wakes anyone, so the stop never meets a
//! wake made after it began.
use crate::{GatewayError, app::State};
use tqa_types::{ClientFrame, ConversationId, TeamWake};
use uuid::Uuid;

/// The idempotency key of `reader`'s wake for what follows `after`, and since its failure `failed`: a helper's count of
/// failures once it has one (a stored run.failed or a give-up, each counted once), a lead's run.failed seq while its
/// latest run failed. Keyed on the place alone, a woken run that failed before reading would leave the key where it
/// was, and the lead's next message would replay that wake and queue nothing (the review of 7 Oct 2026, P2).
fn key(reader: Uuid, after: i64, failed: Option<i64>) -> String {
    match failed {
        Some(failed) => format!("team-wake:{reader}:{after}:failed-{failed}"),
        None => format!("team-wake:{reader}:{after}"),
    }
}

/// The wake's fixed sentence: who wrote, from where, and which tool reads it.
fn sentence(lead: bool, after: i64) -> String {
    if lead {
        format!(
            "New messages from your helpers since message {after}. Read them with read_reports; end your turn when \
             you are waiting for more."
        )
    } else {
        format!(
            "New messages from your lead since message {after}. Read them with wait_for_lead; end your turn when you \
             are waiting for more."
        )
    }
}

/// The wake's turn for a reader (a lead when `lead`) whose place in the log is `after`: the fixed sentence for the
/// model, and the marker (revision 13) that has a client show the gateway's notice, never the person's message. The
/// marker names no writer: what waits may be the helpers', the person's or the gateway's notices (GW-U-145).
///
/// # Errors
/// `StoreUnavailable` for a place below zero, which the store never holds.
fn turn(lead: bool, after: i64) -> Result<ClientFrame, GatewayError> {
    Ok(ClientFrame::UserMessage {
        text: sentence(lead, after),
        attachments: Vec::new(),
        host_keys: Vec::new(),
        lead_brief: None,
        team_wake: Some(TeamWake {
            after: u64::try_from(after).map_err(|_| GatewayError::StoreUnavailable)?,
        }),
    })
}

/// Wakes each reader of `team` that has a message it hasn't read, no active run and no turn already queued: the lead
/// and its live members (a failed one not past the helpers' share of the budget: its every run would be refused and
/// fail again, 16o W1's `member_failed` every 15 s), while the team runs, a done one too (a new assignment is how the lead gives it more work). A
/// member still starting gets its brief as its first turn instead. The 80 % notice (`budget_80`) doesn't wake the lead
/// while a helper is queued, starting or working: their reports, which it asks for, do, and it reads it with them, so it
/// doesn't finish before they come (GW-I-304, the 16n review); with no helper working it wakes the lead as any notice.
///
/// # Errors
/// Storage failure, or the message route's refusals.
pub(crate) async fn team(state: &State, team: Uuid) -> Result<(), GatewayError> {
    // The lead's place (`placed.after` below) is computed the same way in `people_read::found`, which gives it to the
    // person as `lead.read_seq`: a change to one is a change to both (GW-I-319 checks they agree).
    let readers = sqlx::query!(
        "WITH readers AS ( \
           SELECT t.lead_conversation AS conversation, true AS lead, t.lead_last_read_seq AS read, t.channel_id, \
                  t.owner_id, false AS given_up, \
                  (SELECT CASE WHEN e.lifecycle = 'run.failed' THEN e.seq END FROM events e \
                   WHERE e.conversation_id = t.lead_conversation AND e.lifecycle IS NOT NULL \
                   ORDER BY e.seq DESC LIMIT 1) AS failed \
           FROM agent_teams t WHERE t.id = $1 AND t.state = 'running' \
           UNION ALL \
           SELECT m.conversation_id, false, m.last_read_seq, t.channel_id, t.owner_id, \
                  m.state = 'failed' AND m.failed_run, NULLIF(m.failures, 0)::bigint \
           FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
           WHERE t.id = $1 AND t.state = 'running' AND m.state NOT IN ('starting', 'stopped') \
                 AND NOT (m.state = 'failed' AND NOT m.failed_run) \
                 AND NOT (m.state = 'failed' AND t.spent_tokens * 100 >= t.token_budget * $2) \
         ), placed AS ( \
           SELECT r.*, GREATEST(r.read, COALESCE((SELECT max(s) FROM agent_team_reads a, unnest(a.seqs) AS s \
                                                  WHERE a.reader = r.conversation), 0)) AS after \
           FROM readers r \
         ) \
         SELECT p.conversation AS \"conversation!\", p.lead AS \"lead!\", p.after AS \"after!\", \
                p.owner_id AS \"owner!\", p.given_up AS \"given_up!\", p.failed \
         FROM placed p \
         WHERE EXISTS (SELECT 1 FROM channel_messages c WHERE c.channel_id = p.channel_id AND c.seq > p.after \
                AND ((p.lead AND c.to_kind IN ('lead', 'team')) OR (NOT p.lead AND ((c.to_kind = 'member' \
                AND c.to_conversation = p.conversation) OR c.to_kind IN ('members', 'team')))) \
                AND NOT (p.lead AND c.type = 'notice' AND c.body->>'kind' = 'budget_80' AND EXISTS (SELECT 1 \
                FROM agent_team_members w WHERE w.team_id = $1 AND w.state IN ('queued', 'starting', 'working'))) \
                AND NOT (c.type = 'person_message' AND c.body->>'member' = '' AND NOT EXISTS (SELECT 1 \
                FROM idempotency i WHERE i.team_copy_channel = c.channel_id AND i.team_copy_seq = c.seq \
                AND i.execution_status IN ('queued', 'delivered')))) \
           AND NOT EXISTS (SELECT 1 FROM idempotency i WHERE i.conversation_id = p.conversation \
                AND i.execution_status = 'queued')",
        team,
        i64::from(state.config.agent_teams.helpers_stop_percent)
    )
    .fetch_all(&state.pool)
    .await?;
    for reader in readers {
        let active = {
            let mut connection = state.pool.acquire().await?;
            super::store::current_run(&mut connection, reader.conversation).await?
        };
        // An active run reads with its own tools; if it ends without reading, the next pass wakes it. A helper whose
        // sandbox was given up still reads as running (its cut run has no end stored, backlog B1), and nothing runs
        // it: its wake is what starts it again, resuming the cut run on a new sandbox (GW-I-254).
        if active.is_some() && !reader.given_up {
            continue;
        }
        let user = super::first_turn::person(state, reader.owner).await?;
        let id: ConversationId = reader
            .conversation
            .to_string()
            .parse()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let turn = turn(reader.lead, reader.after)?;
        crate::actions::send_turn(
            state,
            &user,
            &id,
            (&key(reader.conversation, reader.after, reader.failed), turn),
        )
        .await?;
    }
    Ok(())
}

/// [`team`] in the background, after a commit that may have addressed someone: a failure is logged, and the next pass
/// (slice 4's reconciler) wakes them.
pub(crate) fn soon(state: &std::sync::Arc<State>, team_id: Uuid) {
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = team(&state, team_id).await {
            tracing::warn!(team = %team_id, code = error.code(), "an agent team's wake wasn't sent");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{key, sentence, turn};
    use tqa_types::{ClientFrame, TeamWake};
    use uuid::Uuid;

    // TEST: GW-U-145
    #[test]
    fn gw_u_145_a_wake_is_marked_as_the_gateways_with_who_wrote() -> Result<(), crate::GatewayError>
    {
        for is_lead in [true, false] {
            assert_eq!(
                turn(is_lead, 9)?,
                ClientFrame::UserMessage {
                    text: sentence(is_lead, 9),
                    attachments: Vec::new(),
                    host_keys: Vec::new(),
                    lead_brief: None,
                    team_wake: Some(TeamWake { after: 9 }),
                },
                "a lead: {is_lead}"
            );
        }
        assert!(turn(false, -1).is_err(), "no place below zero");
        Ok(())
    }

    #[test]
    fn gw_u_133_a_wake_is_keyed_by_the_readers_place_and_names_its_tool() {
        let reader = Uuid::nil();
        assert_eq!(key(reader, 7, None), format!("team-wake:{reader}:7"));
        // After a failure the place may not have moved: the failure (a helper's count) makes the next wake a new one.
        assert_eq!(
            key(reader, 7, Some(12)),
            format!("team-wake:{reader}:7:failed-12")
        );
        assert!(
            key(reader, i64::MAX, Some(i64::MAX)).len() <= 128,
            "within the idempotency key's bound"
        );
        assert!(sentence(true, 3).starts_with(
            "New messages from your helpers since message 3. Read them with read_reports"
        ));
        assert!(sentence(false, 0).starts_with(
            "New messages from your lead since message 0. Read them with wait_for_lead"
        ));
    }
}
