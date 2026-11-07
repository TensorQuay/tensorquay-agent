//! Proposals, votes and the gateway's decisions, and a member's post with the log's rules (ADR-0014 point 9).
use super::{append, append_body, db_seq, kind_of, members::is_claimant, rule_of, seq_of};
use crate::GatewayError;
use sqlx::PgConnection;
use tqa_types::{Author, AuthorKind, ChannelMessage, ChannelPost, DecisionRule, MessageBody};
use uuid::Uuid;

/// A proposal's stored facts: the channel's rule, the proposal's window and when it opened, and its decision.
#[derive(Debug)]
pub(crate) struct Proposal {
    pub rule: DecisionRule,
    pub opened_at: time::OffsetDateTime,
    pub window: time::Duration,
    pub decision: Option<(u64, tqa_types::Outcome)>,
}

impl Proposal {
    /// When its window closes.
    pub(crate) fn closes_at(&self) -> time::OffsetDateTime {
        self.opened_at + self.window
    }
}

/// The proposal at `seq`.
///
/// # Errors
/// `409 not_a_proposal` when `seq` isn't one, storage failure.
pub(crate) async fn proposal(
    connection: &mut PgConnection,
    channel: Uuid,
    seq: u64,
) -> Result<Proposal, GatewayError> {
    let row = sqlx::query!(
        "SELECT p.type, p.body, p.created_at, c.rule, \
         (SELECT d.seq FROM channel_messages d WHERE d.channel_id = p.channel_id AND d.type = 'decision' \
          AND d.proposal_seq = p.seq ORDER BY d.seq LIMIT 1) AS decision_seq, \
         (SELECT d.body->>'outcome' FROM channel_messages d WHERE d.channel_id = p.channel_id AND d.type = 'decision' \
          AND d.proposal_seq = p.seq ORDER BY d.seq LIMIT 1) AS decision_outcome \
         FROM channel_messages p JOIN channels c ON c.id = p.channel_id WHERE p.channel_id = $1 AND p.seq = $2",
        channel,
        db_seq(seq)?
    )
    .fetch_optional(connection)
    .await?
    .ok_or(GatewayError::Conflict("not_a_proposal"))?;
    let MessageBody::Proposal { window_seconds, .. } =
        serde_json::from_value(row.body).map_err(|_| GatewayError::StoreUnavailable)?
    else {
        return Err(GatewayError::Conflict("not_a_proposal"));
    };
    let decision = match (row.decision_seq, row.decision_outcome) {
        (Some(seq), Some(outcome)) => Some((
            seq_of(seq)?,
            serde_json::from_value(serde_json::Value::String(outcome))
                .map_err(|_| GatewayError::StoreUnavailable)?,
        )),
        _ => None,
    };
    Ok(Proposal {
        rule: rule_of(&row.rule)?,
        opened_at: row.created_at,
        window: time::Duration::seconds(i64::from(
            window_seconds.unwrap_or(tqa_types::DEFAULT_PROPOSAL_WINDOW_SECONDS),
        )),
        decision,
    })
}

/// The channel's voting members (`lead` and `member`), as the evaluator sees them.
///
/// # Errors
/// Storage failure.
pub(crate) async fn voters(
    connection: &mut PgConnection,
    channel: Uuid,
) -> Result<Vec<crate::channels::decide::Voter>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT user_id, conversation_id FROM channel_members WHERE channel_id = $1 AND role IN ('lead', 'member')",
        channel
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match (row.user_id, row.conversation_id) {
            (Some(user), _) => Some(crate::channels::decide::Voter {
                key: ("user".into(), user.to_string()),
                human: true,
            }),
            (None, Some(conversation)) => Some(crate::channels::decide::Voter {
                key: ("conversation".into(), conversation.to_string()),
                human: false,
            }),
            _ => None,
        })
        .collect())
}

/// The votes on the proposal at `seq`, in seq order.
///
/// # Errors
/// Storage failure.
pub(crate) async fn ballots(
    connection: &mut PgConnection,
    channel: Uuid,
    seq: u64,
) -> Result<Vec<(crate::channels::decide::Ballot, Author)>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT seq, author_kind, author_id, author_name, body FROM channel_messages \
         WHERE channel_id = $1 AND type = 'vote' AND proposal_seq = $2 ORDER BY seq",
        channel,
        db_seq(seq)?
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let MessageBody::Vote { choice, .. } =
                serde_json::from_value(row.body).map_err(|_| GatewayError::StoreUnavailable)?
            else {
                return Err(GatewayError::StoreUnavailable);
            };
            Ok((
                crate::channels::decide::Ballot {
                    voter: (row.author_kind.clone(), row.author_id.clone()),
                    choice,
                    seq: seq_of(row.seq)?,
                },
                Author {
                    kind: kind_of(&row.author_kind)?,
                    id: row.author_id,
                    name: row.author_name,
                },
            ))
        })
        .collect()
}

/// The gateway, as the author of decisions.
pub(crate) fn gateway_author() -> Author {
    Author {
        kind: AuthorKind::Gateway,
        id: "gateway".into(),
        name: Some("gateway".into()),
    }
}

/// Decides the proposal at `seq` when its rule, its votes or its closed window (`force_close`, or the clock) say so,
/// and appends the gateway's `decision`. `None` while it stays open or when it was decided already. Runs in the
/// caller's transaction.
///
/// # Errors
/// Storage failure.
pub(crate) async fn decide(
    connection: &mut PgConnection,
    channel: Uuid,
    seq: u64,
    force_close: bool,
) -> Result<Option<ChannelMessage>, GatewayError> {
    let proposal = proposal(connection, channel, seq).await?;
    if proposal.decision.is_some() {
        return Ok(None);
    }
    let open = !force_close && time::OffsetDateTime::now_utc() < proposal.closes_at();
    let voters = voters(connection, channel).await?;
    let ballots: Vec<_> = ballots(connection, channel, seq)
        .await?
        .into_iter()
        .map(|(ballot, _)| ballot)
        .collect();
    let Some(outcome) = crate::channels::decide::outcome(proposal.rule, &voters, &ballots, open)
    else {
        return Ok(None);
    };
    let decision = MessageBody::Decision {
        proposal_seq: seq,
        outcome,
        rule: proposal.rule,
    };
    append_body(connection, channel, &gateway_author(), (None, &decision))
        .await
        .map(Some)
}

/// Checks a vote before it is stored: it names an open proposal, and only a person vetoes.
///
/// # Errors
/// `409 not_a_proposal` or `proposal_closed`, `403 veto_not_allowed`, storage failure.
pub(crate) async fn check_vote(
    connection: &mut PgConnection,
    channel: Uuid,
    (proposal_seq, choice): (u64, tqa_types::VoteChoice),
    author: &Author,
) -> Result<(), GatewayError> {
    let proposal = proposal(connection, channel, proposal_seq).await?;
    if proposal.decision.is_some() || time::OffsetDateTime::now_utc() >= proposal.closes_at() {
        return Err(GatewayError::Conflict("proposal_closed"));
    }
    if choice == tqa_types::VoteChoice::Veto && author.kind != AuthorKind::User {
        return Err(GatewayError::Denied("veto_not_allowed"));
    }
    Ok(())
}

/// The undecided proposals of the channel, or of every open channel, whose window has closed (at most 100).
///
/// # Errors
/// Storage failure.
pub(crate) async fn due_proposals(
    connection: &mut PgConnection,
    channel: Option<Uuid>,
    include_open_windows: bool,
) -> Result<Vec<(Uuid, u64)>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT p.channel_id, p.seq FROM channel_messages p JOIN channels c ON c.id = p.channel_id \
         WHERE p.type = 'proposal' AND ($1::uuid IS NULL OR p.channel_id = $1) \
         AND ($2 OR (c.state = 'open' AND p.created_at + make_interval(secs => \
              COALESCE((p.body->>'window_seconds')::double precision, $3)) <= CURRENT_TIMESTAMP)) \
         AND NOT EXISTS (SELECT 1 FROM channel_messages d WHERE d.channel_id = p.channel_id AND d.type = 'decision' \
                         AND d.proposal_seq = p.seq) \
         ORDER BY p.created_at LIMIT 100",
        channel,
        include_open_windows,
        f64::from(tqa_types::DEFAULT_PROPOSAL_WINDOW_SECONDS)
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| Ok((row.channel_id, seq_of(row.seq)?)))
        .collect()
}

/// Appends a member's validated post with the log's rules around it: a vote names an open proposal (and only a
/// person vetoes), a report comes from a claimant, and a vote that decides its proposal is followed by the gateway's
/// `decision`. Returns what was appended, the post first. Runs in the caller's transaction.
///
/// # Errors
/// As [`append`], [`check_vote`], and `403 not_claimant`.
pub(crate) async fn post(
    connection: &mut PgConnection,
    channel: Uuid,
    author: &Author,
    post: &ChannelPost,
) -> Result<Vec<ChannelMessage>, GatewayError> {
    if let MessageBody::Vote {
        proposal_seq,
        choice,
        ..
    } = &post.body
    {
        check_vote(connection, channel, (*proposal_seq, *choice), author).await?;
    }
    if let MessageBody::Report { task_seq, .. } = &post.body
        && !is_claimant(connection, channel, *task_seq, author).await?
    {
        return Err(GatewayError::Denied("not_claimant"));
    }
    let stored = append(connection, channel, author, post).await?;
    let mut appended = vec![stored];
    if let MessageBody::Vote { proposal_seq, .. } = &post.body
        && let Some(decision) = decide(connection, channel, *proposal_seq, false).await?
    {
        appended.push(decision);
    }
    Ok(appended)
}

/// Closes the channel's open proposals as its end does (ADR-0014 point 13); the last decision's seq.
///
/// # Errors
/// Storage failure.
pub(crate) async fn close_open(
    connection: &mut PgConnection,
    channel: Uuid,
) -> Result<Option<u64>, GatewayError> {
    let mut last = None;
    for (_, seq) in due_proposals(connection, Some(channel), true).await? {
        if let Some(decision) = decide(connection, channel, seq, true).await? {
            last = Some(decision.seq);
        }
    }
    Ok(last)
}
