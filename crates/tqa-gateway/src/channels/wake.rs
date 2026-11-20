//! Waking a channel's agent members (ADR-0014 amendment of 20 Nov 2026, A1 to A6): who a message wakes, one wake for
//! a burst keyed by the member's place, the gateway's fixed sentence (nothing written by anyone else), and the hop that
//! bounds agents waking each other.
use crate::{GatewayError, app::State};
use sqlx::PgConnection;
use tqa_types::{Author, AuthorKind, ChannelNoticeKind, ChannelWake, ClientFrame, MessageBody};
use uuid::Uuid;

/// The bound on agent-to-agent wakes (A6): no message with this hop or more wakes anyone.
pub(crate) const MAX_HOP: i16 = 4;

/// What waits for a woken member, by kind and seq (A4): never a title, a body or the channel's name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Waiting {
    /// A proposal for its vote.
    Proposal(i64),
    /// A to-do it may take on.
    Task(i64),
    /// A post naming it.
    Named(i64),
    /// A person's post to the whole channel, naming no one (decided on 20 Nov 2026, C-4).
    Channel(i64),
    /// A reply to its post.
    Reply(i64),
    /// The decision on its proposal.
    Decision(i64),
    /// A report on its to-do.
    Report(i64),
}

/// The idempotency key of `member`'s wake in `channel` for what follows `after` (A3), naming its failure count once a
/// run failed, as a team's wake does. The ids without hyphens, so the longest key fits an idempotency key's 128 bytes.
pub(crate) fn key(channel: Uuid, member: Uuid, after: i64, failed: Option<i64>) -> String {
    match failed {
        Some(failed) => format!(
            "channel-wake:{}:{}:{after}:failed-{failed}",
            channel.simple(),
            member.simple()
        ),
        None => format!(
            "channel-wake:{}:{}:{after}",
            channel.simple(),
            member.simple()
        ),
    }
}

#[path = "wake_sentence.rs"]
mod sentence;
pub(crate) use sentence::{files_sentence, sentence};

/// The members a text names (A1): `@` and a member's name, matched without case, the longest name first; each once.
pub(crate) fn named(text: &str, members: &[(Uuid, &str)]) -> Vec<Uuid> {
    let lower = text.to_lowercase();
    let mut names: Vec<(Uuid, String)> = members
        .iter()
        .filter(|(_, name)| !name.trim().is_empty())
        .map(|(id, name)| (*id, name.trim().to_lowercase()))
        .collect();
    // The longest first, so "@Pricing analyst" is never read as "@Pricing".
    names.sort_by_key(|(_, name)| std::cmp::Reverse(name.chars().count()));
    let mut found = Vec::new();
    for (at, _) in lower.match_indices('@') {
        // An "@" inside a word (an e-mail address) isn't a mention.
        if lower[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        let rest = &lower[at + 1..];
        let hit = names.iter().find(|(_, name)| {
            rest.starts_with(name.as_str())
                && !rest[name.len()..]
                    .chars()
                    .next()
                    .is_some_and(char::is_alphanumeric)
        });
        if let Some((id, _)) = hit
            && !found.contains(id)
        {
            found.push(*id);
        }
    }
    found
}

/// A new message's hop (A6): a person's is 0; an agent's is one more than the hop of the wake that started its run,
/// or 1 in a run a person started.
pub(crate) const fn message_hop(person: bool, woken_at: Option<i16>) -> i16 {
    if person {
        return 0;
    }
    match woken_at {
        Some(hop) => hop.saturating_add(1),
        None => 1,
    }
}

/// A wake's hop (A6): the smallest among the unread messages that wake the member, so a person's post starts a fresh
/// chain; `None` when nothing may wake it (each at [`MAX_HOP`] or more).
pub(crate) fn wake_hop(unread: &[i16]) -> Option<i16> {
    unread.iter().copied().min().filter(|hop| *hop < MAX_HOP)
}

/// Wakes for agents' posts a channel sends in any hour, at most (A6).
pub(crate) const HOURLY: i64 = 30;

/// The notice when a bound holds a wake back (A6).
const BOUNDED: &str = "Agents here stopped waking each other; a person's post starts them again.";

/// A message's author as a member task, when an agent wrote it.
fn agent_of(author: &Author) -> Option<Uuid> {
    (author.kind == AuthorKind::Conversation)
        .then(|| Uuid::parse_str(&author.id).ok())
        .flatten()
}

/// The member task that wrote message `seq` of `channel`, if an agent did.
async fn written_by(
    connection: &mut PgConnection,
    channel: Uuid,
    seq: i64,
) -> Result<Option<Uuid>, GatewayError> {
    let row = sqlx::query!(
        "SELECT author_kind, author_id FROM channel_messages WHERE channel_id = $1 AND seq = $2",
        channel,
        seq
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(row
        .filter(|row| row.author_kind == "conversation")
        .and_then(|row| Uuid::parse_str(&row.author_id).ok()))
}

/// The agent members a message wakes (A1): every voting agent but its writer for a proposal or a to-do, the members
/// its words name, the writer of what it replies to, and a decision's proposer or a report's to-do writer.
async fn addressees(
    connection: &mut PgConnection,
    (channel, writer, person): (Uuid, Option<Uuid>, bool),
    (parent, body): (Option<i64>, &MessageBody),
) -> Result<Vec<Uuid>, GatewayError> {
    // Every agent voting member is an addressee; only "@" needs a name, and one without a name has none to match.
    let others: Vec<(Uuid, Option<String>)> = sqlx::query!(
        "SELECT m.conversation_id AS \"conversation!\", m.name FROM channel_members m \
         JOIN conversations c ON c.id = m.conversation_id \
         WHERE m.channel_id = $1 AND m.conversation_id IS NOT NULL AND m.role <> 'observer' AND c.state = 'active' \
         ORDER BY m.added_at",
        channel
    )
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .filter(|member| Some(member.conversation) != writer)
    .map(|member| (member.conversation, member.name))
    .collect();
    let mut wakes: Vec<Uuid> = Vec::new();
    let mut add = |id: Option<Uuid>| {
        if let Some(id) = id
            && !wakes.contains(&id)
            && others.iter().any(|(member, _)| *member == id)
        {
            wakes.push(id);
        }
    };
    let (everyone, words) = match body {
        MessageBody::Proposal { text, .. } => (true, Some(text.as_str())),
        MessageBody::Task { spec, .. } => (true, Some(spec.as_str())),
        MessageBody::Text { text } => (false, Some(text.as_str())),
        MessageBody::Report { summary, .. } => (false, Some(summary.as_str())),
        _ => (false, None),
    };
    if everyone {
        for (member, _) in &others {
            add(Some(*member));
        }
    }
    match body {
        MessageBody::Decision { proposal_seq, .. } => {
            add(written_by(connection, channel, seq_of(*proposal_seq)?).await?);
        }
        MessageBody::Report { task_seq, .. } => {
            add(written_by(connection, channel, seq_of(*task_seq)?).await?);
        }
        _ => {}
    }
    if let Some(words) = words {
        // "@" matches members' names in the channel, never a title (A12); a member without a name can't be named.
        let names: Vec<(Uuid, &str)> = others
            .iter()
            .filter_map(|(member, name)| name.as_deref().map(|name| (*member, name)))
            .collect();
        for member in named(words, &names) {
            add(Some(member));
        }
    }
    if let Some(parent) = parent {
        add(written_by(connection, channel, parent).await?);
    }
    // A person's top-level text that names no one is for the whole channel (C-4): it wakes every agent voting member,
    // as a to-do does. A reply stays in its thread, and an agent's text never fans out.
    if person && parent.is_none() && wakes.is_empty() && matches!(body, MessageBody::Text { .. }) {
        wakes = others.iter().map(|(member, _)| *member).collect();
    }
    Ok(wakes)
}

/// A new message's hop (A6): a person's 0; an agent's one more than its run's wake (1 in a run a person started); a
/// decision the hop of what decided it (the proposal on a timeout, else its latest vote or the proposal); any other
/// gateway message 0.
async fn hop_of(
    connection: &mut PgConnection,
    (channel, writer): (Uuid, Option<Uuid>),
    author: &Author,
    body: &MessageBody,
) -> Result<i16, GatewayError> {
    Ok(match (author.kind, body) {
        (AuthorKind::User, _) => message_hop(true, None),
        (AuthorKind::Conversation, _) => {
            let woken_at = sqlx::query_scalar!(
                "SELECT hop FROM channel_charges WHERE conversation_id = $1",
                writer
            )
            .fetch_optional(&mut *connection)
            .await?;
            message_hop(false, woken_at)
        }
        (
            AuthorKind::Gateway,
            MessageBody::Decision {
                proposal_seq,
                outcome,
                ..
            },
        ) => {
            let proposal = seq_of(*proposal_seq)?;
            let timed_out = *outcome == tqa_types::Outcome::TimedOut;
            sqlx::query_scalar!(
                "SELECT hop FROM channel_messages WHERE channel_id = $1 \
                 AND (seq = $2 OR (NOT $3 AND type = 'vote' AND proposal_seq = $2)) ORDER BY seq DESC LIMIT 1",
                channel,
                proposal,
                timed_out
            )
            .fetch_optional(&mut *connection)
            .await?
            .unwrap_or(0)
        }
        (AuthorKind::Gateway, _) => 0,
    })
}

/// Marks message `seq` of `channel`, just appended in the caller's transaction: the agent members it wakes (A1) and its
/// hop (A6). A person's post also clears the channel's said bound, so the next chain's is said again.
///
/// # Errors
/// Storage failure.
pub(crate) async fn mark(
    connection: &mut PgConnection,
    (channel, seq): (Uuid, i64),
    author: &Author,
    (parent, body): (Option<i64>, &MessageBody),
) -> Result<(), GatewayError> {
    let writer = agent_of(author);
    let person = author.kind == AuthorKind::User;
    let wakes = addressees(connection, (channel, writer, person), (parent, body)).await?;
    let hop = hop_of(connection, (channel, writer), author, body).await?;
    sqlx::query!(
        "UPDATE channel_messages SET wakes = $3, hop = $4 WHERE channel_id = $1 AND seq = $2",
        channel,
        seq,
        &wakes,
        hop
    )
    .execute(&mut *connection)
    .await?;
    if author.kind == AuthorKind::User {
        sqlx::query!(
            "UPDATE channels SET wakes_bounded = false WHERE id = $1 AND wakes_bounded",
            channel
        )
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

fn seq_of(seq: u64) -> Result<i64, GatewayError> {
    i64::try_from(seq).map_err(|_| GatewayError::StoreUnavailable)
}

/// Posts the gateway's notice in the caller's transaction; answers its seq, to publish after the commit.
///
/// # Errors
/// Storage failure, or `channel_ended`.
pub(crate) async fn notice(
    connection: &mut PgConnection,
    channel: Uuid,
    (kind, member): (ChannelNoticeKind, Option<String>),
    text: String,
) -> Result<u64, GatewayError> {
    let author = super::store::gateway_author();
    let message = super::store::append_body(
        connection,
        channel,
        &author,
        (None, &MessageBody::Notice { kind, member, text }),
    )
    .await?;
    Ok(message.seq)
}

/// A notice in its own transaction, published after the commit.
async fn notice_now(
    state: &State,
    channel: Uuid,
    (kind, member): (ChannelNoticeKind, Option<String>),
    text: String,
) -> Result<(), GatewayError> {
    let mut transaction = state.pool.begin().await?;
    let seq = notice(&mut transaction, channel, (kind, member), text).await?;
    transaction.commit().await?;
    state.channels.published(channel, seq);
    Ok(())
}

/// Says once that `member` (named `title`) can't be woken, for `reason` (A10): `runtime` or `deleted`.
///
/// # Errors
/// Storage failure.
pub(crate) async fn unreachable(
    connection: &mut PgConnection,
    (channel, member): (Uuid, Uuid),
    title: &str,
    reason: &str,
) -> Result<Option<u64>, GatewayError> {
    let first = sqlx::query_scalar!(
        "INSERT INTO channel_unreachable (channel_id, conversation_id, reason) VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING RETURNING channel_id",
        channel,
        member,
        reason
    )
    .fetch_optional(&mut *connection)
    .await?;
    if first.is_none() {
        return Ok(None);
    }
    let (kind, why) = if reason == "deleted" {
        (ChannelNoticeKind::MemberDeleted, "its task was deleted")
    } else {
        (
            ChannelNoticeKind::UnreachableRuntime,
            "its environment needs an update",
        )
    };
    Ok(Some(
        notice(
            connection,
            channel,
            (kind, Some(title.to_owned())),
            format!("{title} can't be woken: {why}."),
        )
        .await?,
    ))
}

/// Says a bound held a wake back, once for each chain (A6).
async fn bounded(state: &State, channel: Uuid) -> Result<(), GatewayError> {
    let mut transaction = state.pool.begin().await?;
    let first = sqlx::query_scalar!(
        "UPDATE channels SET wakes_bounded = true WHERE id = $1 AND NOT wakes_bounded RETURNING id",
        channel
    )
    .fetch_optional(&mut *transaction)
    .await?;
    if first.is_none() {
        return Ok(());
    }
    let seq = notice(
        &mut transaction,
        channel,
        (ChannelNoticeKind::WakesBounded, None),
        BOUNDED.into(),
    )
    .await?;
    transaction.commit().await?;
    state.channels.published(channel, seq);
    Ok(())
}

/// The channel's budget for wakes: its own, or the installation's (A5); 0 means waking is off.
fn budget_of(state: &State, own: Option<i64>) -> i64 {
    own.unwrap_or_else(|| i64::try_from(state.config.channels.wake_budget).unwrap_or(i64::MAX))
}

/// A member a wake may be due for: an unread message wakes it.
struct Due {
    channel: Uuid,
    member: Uuid,
    read_seq: i64,
    owner: Uuid,
    revision: Option<i32>,
    title: String,
    budget: Option<i64>,
    spent: i64,
    /// The smallest hop among the unread messages that wake it.
    hop: i16,
    /// Its latest run's failure, while that run failed (the key names it, A3).
    failed: Option<i64>,
}

/// The members with an unread message that wakes them, in `only`'s channel or in all.
async fn due(state: &State, only: Option<Uuid>) -> Result<Vec<Due>, GatewayError> {
    Ok(sqlx::query!(
        "SELECT m.channel_id AS \"channel!\", m.conversation_id AS \"conversation!\", m.read_seq, cv.owner_id, \
                cv.runtime_revision, \
                COALESCE(m.name, cv.profile->>'display_name', cv.profile->>'name', 'An agent') AS \"title!\", \
                ch.budget, ch.spent_tokens, \
                (SELECT min(x.hop) FROM channel_messages x WHERE x.channel_id = m.channel_id AND x.seq > m.read_seq \
                 AND m.conversation_id = ANY(x.wakes)) AS \"hop!\", \
                (SELECT CASE WHEN e.lifecycle = 'run.failed' THEN e.seq END FROM events e \
                 WHERE e.conversation_id = m.conversation_id AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1) \
                 AS failed \
         FROM channel_members m JOIN channels ch ON ch.id = m.channel_id \
         JOIN conversations cv ON cv.id = m.conversation_id \
         WHERE ch.kind = 'channel' AND ch.state = 'open' AND m.role <> 'observer' AND cv.state = 'active' \
           AND ($1::uuid IS NULL OR m.channel_id = $1) \
           AND EXISTS (SELECT 1 FROM channel_messages x WHERE x.channel_id = m.channel_id AND x.seq > m.read_seq \
                       AND m.conversation_id = ANY(x.wakes)) \
         ORDER BY m.channel_id, m.conversation_id",
        only
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| Due {
        channel: row.channel,
        member: row.conversation,
        read_seq: row.read_seq,
        owner: row.owner_id,
        revision: row.runtime_revision,
        title: row.title,
        budget: row.budget,
        spent: row.spent_tokens,
        hop: row.hop,
        failed: row.failed,
    })
    .collect())
}

/// Whether `due`'s wake is held back now; `None` when it may go, with its hop. Busy (a run or a turn on its way, A3),
/// below revision 16 (said once, A10), the budget spent (A5), or a bound (said once per chain, A6).
async fn held_back(state: &State, due: &Due) -> Result<Option<i16>, GatewayError> {
    let mut connection = state.pool.acquire().await?;
    // A turn on its way: queued, or a wake whose run hasn't started yet (sent within the last 15 minutes, so a turn that
    // never ran doesn't hold the member for ever).
    let queued = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM idempotency WHERE conversation_id = $1 AND execution_status = 'queued') \
         OR EXISTS (SELECT 1 FROM channel_wakes w JOIN idempotency i ON i.conversation_id = w.conversation_id \
                    AND i.key = w.key WHERE w.conversation_id = $1 AND NOT w.started \
                    AND i.execution_status <> 'failed' AND w.sent_at > CURRENT_TIMESTAMP - interval '15 minutes') \
         AS \"queued!\"",
        due.member
    )
    .fetch_one(&mut *connection)
    .await?;
    if queued
        || crate::teams::store::current_run(&mut connection, due.member)
            .await?
            .is_some()
    {
        return Ok(None);
    }
    if due
        .revision
        .is_some_and(|revision| revision < i32::from(tqa_types::CHANNEL_WAKE_REVISION))
    {
        let mut transaction = state.pool.begin().await?;
        let said = unreachable(
            &mut transaction,
            (due.channel, due.member),
            &due.title,
            "runtime",
        )
        .await?;
        transaction.commit().await?;
        if let Some(seq) = said {
            state.channels.published(due.channel, seq);
        }
        return Ok(None);
    }
    let budget = budget_of(state, due.budget);
    if budget <= 0 || due.spent >= budget {
        return Ok(None);
    }
    let Some(hop) = wake_hop(&[due.hop]) else {
        bounded(state, due.channel).await?;
        return Ok(None);
    };
    if hop >= 1 {
        let recent = sqlx::query_scalar!(
            "SELECT count(*) AS \"count!\" FROM channel_wakes WHERE channel_id = $1 AND hop >= 1 \
             AND sent_at > CURRENT_TIMESTAMP - interval '1 hour'",
            due.channel
        )
        .fetch_one(&mut *connection)
        .await?;
        if recent >= HOURLY {
            bounded(state, due.channel).await?;
            return Ok(None);
        }
    }
    Ok(Some(hop))
}

/// What waits for `due`'s member, by kind and seq (A4).
async fn waiting(state: &State, due: &Due) -> Result<Vec<Waiting>, GatewayError> {
    let member = due.member.to_string();
    // Members' names, to tell a post naming this member from one to the whole channel.
    let named_members: Vec<(Uuid, String)> = sqlx::query!(
        "SELECT conversation_id AS \"conversation!\", name AS \"name!\" FROM channel_members \
         WHERE channel_id = $1 AND conversation_id IS NOT NULL AND name IS NOT NULL",
        due.channel
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| (row.conversation, row.name))
    .collect();
    let names: Vec<(Uuid, &str)> = named_members
        .iter()
        .map(|(id, name)| (*id, name.as_str()))
        .collect();
    Ok(sqlx::query!(
        "SELECT x.seq, x.type AS kind, p.author_id AS \"parent_author?\", x.author_kind, x.parent, \
                x.body->>'text' AS words FROM channel_messages x \
         LEFT JOIN channel_messages p ON p.channel_id = x.channel_id AND p.seq = x.parent \
         WHERE x.channel_id = $1 AND x.seq > $2 AND $3 = ANY(x.wakes) ORDER BY x.seq",
        due.channel,
        due.read_seq,
        due.member
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|message| match message.kind.as_str() {
        "proposal" => Waiting::Proposal(message.seq),
        "task" => Waiting::Task(message.seq),
        "decision" => Waiting::Decision(message.seq),
        "report" => Waiting::Report(message.seq),
        _ if message.parent_author.as_deref() == Some(member.as_str()) => {
            Waiting::Reply(message.seq)
        }
        // A person's top-level text that doesn't name this member woke it as a post to the whole channel (C-4).
        "text"
            if message.author_kind == "user"
                && message.parent.is_none()
                && !named(message.words.as_deref().unwrap_or(""), &names).contains(&due.member) =>
        {
            Waiting::Channel(message.seq)
        }
        _ => Waiting::Named(message.seq),
    })
    .collect())
}

/// Sends `due`'s wake at `hop`, unless its key went already (a run that ended without reading, A3). `true` once sent.
async fn deliver(state: &State, due: &Due, hop: i16) -> Result<bool, GatewayError> {
    let key = key(due.channel, due.member, due.read_seq, due.failed);
    let sent = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM idempotency WHERE conversation_id = $1 AND key = $2) AS \"sent!\"",
        due.member,
        key
    )
    .fetch_one(&state.pool)
    .await?;
    if sent {
        return Ok(false);
    }
    let waiting = waiting(state, due).await?;
    // The unread messages' files, in its workspace before the turn that names their paths (A9).
    let placed: Vec<i64> =
        super::files::place(state, (due.channel, due.member), (due.read_seq, None))
            .await?
            .into_iter()
            .filter(|placed| placed.path.is_some())
            .map(|placed| placed.seq)
            .collect();
    let after = u64::try_from(due.read_seq).map_err(|_| GatewayError::StoreUnavailable)?;
    let channel: tqa_types::ChannelId = due
        .channel
        .to_string()
        .parse()
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let turn = ClientFrame::UserMessage {
        text: sentence(due.channel, due.read_seq, &waiting) + &files_sentence(&placed),
        attachments: Vec::new(),
        host_keys: Vec::new(),
        lead_brief: None,
        team_wake: None,
        channel_wake: Some(Box::new(ChannelWake { channel, after })),
    };
    let user = crate::teams::first_turn::person(state, due.owner).await?;
    let id: tqa_types::ConversationId = due
        .member
        .to_string()
        .parse()
        .map_err(|_| GatewayError::StoreUnavailable)?;
    crate::actions::send_turn(state, &user, &id, (&key, turn)).await?;
    sqlx::query!(
        "INSERT INTO channel_wakes (channel_id, conversation_id, hop, key) VALUES ($1, $2, $3, $4)",
        due.channel,
        due.member,
        hop,
        key
    )
    .execute(&state.pool)
    .await?;
    Ok(true)
}

/// Sends every wake that is due (A3): in `only`'s channel, or in all, one pass at a time. Each member with an unread
/// message that wakes it gets one turn unless [`held_back`]; a member of two channels gets one turn a pass.
///
/// # Errors
/// Storage failure, or the message route's refusals.
pub(crate) async fn send(state: &State, only: Option<Uuid>) -> Result<(), GatewayError> {
    if state.channels.held() {
        return Ok(());
    }
    let _pass = state.channels.pass().await;
    let mut woken: Vec<Uuid> = Vec::new();
    for due in due(state, only).await? {
        if woken.contains(&due.member) {
            continue;
        }
        if let Some(hop) = held_back(state, &due).await?
            && deliver(state, &due, hop).await?
        {
            woken.push(due.member);
        }
    }
    Ok(())
}

/// How long a post's wake gathers what follows it, so a burst is one wake that names it all (A3).
const GATHER: std::time::Duration = std::time::Duration::from_secs(1);

/// [`send`] in the background, after a commit that may have made a wake due, a moment later so a burst is told in one
/// sentence; a failure is logged and the sweep sends it later.
pub(crate) fn soon(state: &std::sync::Arc<State>, channel: Option<Uuid>) {
    // The gateway's own task: its shutdown joins it, and its cancel stops it before anything is sent.
    let tasks = state.tasks.clone();
    let state = state.clone();
    tasks.spawn(async move {
        let sent = tokio::select! {biased;
            () = state.cancel.cancelled() => Ok(()),
            sent = async {
                tokio::time::sleep(GATHER).await;
                send(&state, channel).await
            } => sent,
        };
        if let Err(error) = sent {
            tracing::warn!(code = error.code(), "a channel's wake wasn't sent");
        }
    });
}

/// A run starts on `conversation`, in the transaction that stores its start: if a channel's wake started it (the wake
/// whose turn was handed over and hasn't started a run yet), it is charged to that channel with that wake's hop (A5,
/// A6); any other run is charged to none.
///
/// # Errors
/// Storage failure.
pub(crate) async fn run_started(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM channel_charges WHERE conversation_id = $1",
        conversation
    )
    .execute(&mut *connection)
    .await?;
    // The wake handed over for this run, marked at the hand-over: never "the turn delivered last", which update times
    // can't tell apart (CI on #144: an earlier wake's row touched later reset the chain's hop).
    sqlx::query!(
        "INSERT INTO channel_charges (conversation_id, channel_id, hop) \
         SELECT conversation_id, channel_id, hop FROM channel_wakes \
         WHERE conversation_id = $1 AND handed AND NOT started ORDER BY sent_at DESC LIMIT 1",
        conversation
    )
    .execute(&mut *connection)
    .await?;
    // Whatever turn started it, the wakes sent before it are on their way no more.
    sqlx::query!(
        "UPDATE channel_wakes SET started = true WHERE conversation_id = $1 AND NOT started",
        conversation
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// A run ends on `conversation`: nothing more is charged to a channel (A5).
///
/// # Errors
/// Storage failure.
pub(crate) async fn run_ended(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM channel_charges WHERE conversation_id = $1",
        conversation
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Whether `before` < `percent` % of `budget` <= `after`, in whole numbers.
fn crossed(before: i64, after: i64, budget: i64, percent: i64) -> bool {
    let line = i128::from(budget) * i128::from(percent);
    i128::from(before) * 100 < line && line <= i128::from(after) * 100
}

/// Adds `tokens` to the channel a charged run of `conversation` spends for, in the meter's transaction (A5): 80 % and
/// 100 % are each said once, by the spend that crosses them. Answers the notices to publish after the commit.
///
/// # Errors
/// Storage failure.
pub(crate) async fn spend(
    connection: &mut PgConnection,
    conversation: Uuid,
    tokens: i64,
    default_budget: u64,
) -> Result<Vec<(Uuid, u64)>, GatewayError> {
    if tokens <= 0 {
        return Ok(Vec::new());
    }
    let Some(channel) = sqlx::query!(
        "UPDATE channels c SET spent_tokens = c.spent_tokens + $2 FROM channel_charges g \
         WHERE g.conversation_id = $1 AND c.id = g.channel_id RETURNING c.id, c.spent_tokens, c.budget",
        conversation,
        tokens
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(Vec::new());
    };
    let budget = channel
        .budget
        .unwrap_or_else(|| i64::try_from(default_budget).unwrap_or(i64::MAX));
    let before = channel.spent_tokens - tokens;
    let mut published = Vec::new();
    if budget > 0 && crossed(before, channel.spent_tokens, budget, 80) {
        let text = "Woken agents have used 80 % of this channel's budget.".to_owned();
        published.push((
            channel.id,
            notice(
                connection,
                channel.id,
                (ChannelNoticeKind::WakeBudget80, None),
                text,
            )
            .await?,
        ));
    }
    if budget > 0 && crossed(before, channel.spent_tokens, budget, 100) {
        let text = "Woken agents have used this channel's budget: no agent is woken here until it is raised.".to_owned();
        published.push((
            channel.id,
            notice(
                connection,
                channel.id,
                (ChannelNoticeKind::WakeBudgetSpent, None),
                text,
            )
            .await?,
        ));
    }
    Ok(published)
}

/// Refuses a model call from a run charged to a channel whose budget is spent: `409 channel_budget` (A5).
///
/// # Errors
/// `409 channel_budget`, a store failure.
pub(crate) async fn check(state: &State, conversation: Uuid) -> Result<(), GatewayError> {
    let default = i64::try_from(state.config.channels.wake_budget).unwrap_or(i64::MAX);
    let spent = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT COALESCE(c.budget, $2) > 0 AND c.spent_tokens >= COALESCE(c.budget, $2) AS \"spent!\" \
             FROM channel_charges g JOIN channels c ON c.id = g.channel_id WHERE g.conversation_id = $1",
            conversation,
            default
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    if spent == Some(true) {
        return Err(GatewayError::Conflict("channel_budget"));
    }
    Ok(())
}

/// The member's reads were answered up to `seq`: its place moves there, never back (A2).
///
/// # Errors
/// Storage failure.
pub(crate) async fn read_to(
    connection: &mut PgConnection,
    (channel, member): (Uuid, Uuid),
    seq: i64,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE channel_members SET read_seq = GREATEST(read_seq, $3) WHERE channel_id = $1 AND conversation_id = $2",
        channel,
        member,
        seq
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// An agent member's live state (A7): `waiting`, `working`, `waking`, `unreachable`, `held` or `idle`.
///
/// # Errors
/// Storage failure.
pub(crate) async fn member_state(
    state: &State,
    connection: &mut PgConnection,
    (channel, member): (Uuid, Uuid),
) -> Result<&'static str, GatewayError> {
    if state.channels.is_waiting(channel, member) {
        return Ok("waiting");
    }
    if crate::teams::store::current_run(connection, member)
        .await?
        .is_some()
    {
        return Ok("working");
    }
    let prefix = format!("channel-wake:{}:{}:%", channel.simple(), member.simple());
    let row = sqlx::query!(
        "SELECT EXISTS (SELECT 1 FROM idempotency WHERE conversation_id = $2 AND execution_status = 'queued' \
                        AND key LIKE $3) AS \"waking!\", \
                cv.runtime_revision, ch.budget, ch.spent_tokens, ch.wakes_bounded, \
                EXISTS (SELECT 1 FROM channel_messages x JOIN channel_members m ON m.channel_id = x.channel_id \
                        AND m.conversation_id = $2 WHERE x.channel_id = $1 AND x.seq > m.read_seq \
                        AND $2 = ANY(x.wakes)) AS \"unread!\" \
         FROM channels ch, conversations cv WHERE ch.id = $1 AND cv.id = $2",
        channel,
        member,
        prefix
    )
    .fetch_one(&mut *connection)
    .await?;
    if row.waking {
        return Ok("waking");
    }
    if row
        .runtime_revision
        .is_some_and(|revision| revision < i32::from(tqa_types::CHANNEL_WAKE_REVISION))
    {
        return Ok("unreachable");
    }
    let budget = budget_of(state, row.budget);
    if row.unread && (budget <= 0 || row.spent_tokens >= budget || row.wakes_bounded) {
        return Ok("held");
    }
    Ok("idle")
}

/// The open channels `conversation` belongs to, and its name: noted before its task is deleted, for [`deleted`].
///
/// # Errors
/// Storage failure.
pub(crate) async fn memberships(
    state: &State,
    conversation: Uuid,
) -> Result<Vec<(Uuid, String)>, GatewayError> {
    Ok(sqlx::query!(
        "SELECT m.channel_id, \
         COALESCE(m.name, c.profile->>'display_name', c.profile->>'name', 'An agent') AS \"title!\" \
         FROM channel_members m JOIN channels ch ON ch.id = m.channel_id \
         JOIN conversations c ON c.id = m.conversation_id \
         WHERE m.conversation_id = $1 AND ch.kind = 'channel' AND ch.state = 'open' AND m.role <> 'observer'",
        conversation
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| (row.channel_id, row.title))
    .collect())
}

/// A member's task was deleted: each of its channels says once that it can't be woken (A10). A deletion happens
/// once, so no record is kept for it (the task's row may be gone).
pub(crate) async fn deleted(state: &State, memberships: Vec<(Uuid, String)>) {
    for (channel, title) in memberships {
        let text = format!("{title} can't be woken: its task was deleted.");
        if let Err(error) = notice_now(
            state,
            channel,
            (ChannelNoticeKind::MemberDeleted, Some(title.clone())),
            text,
        )
        .await
        {
            tracing::warn!(
                code = error.code(),
                "a channel wasn't told a member's task was deleted"
            );
        }
    }
}

#[cfg(test)]
#[path = "wake_tests.rs"]
mod tests;
