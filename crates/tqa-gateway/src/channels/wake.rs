//! Waking a channel's agent members (ADR-0014 amendment of 20 Nov 2026, A1 to A6): who a message wakes, one wake for
//! a burst keyed by the member's place, the gateway's fixed sentence (nothing written by anyone else), and the hop that
//! bounds agents waking each other.
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

/// The wake's fixed sentence (A4).
pub(crate) fn sentence(channel: Uuid, after: i64, waiting: &[Waiting]) -> String {
    // Each kind once, in this order, with its seqs in the log's order.
    const KINDS: [(&str, &str, &str); 6] = [
        ("a proposal", "proposals", "for your vote"),
        ("a to-do", "to-dos", "you may take on"),
        ("a post", "posts", "naming you"),
        ("a reply", "replies", "to your post"),
        ("the decision", "decisions", "on your proposal"),
        ("a report", "reports", "on your to-do"),
    ];
    let mut parts = Vec::new();
    for (index, (one, many, what)) in KINDS.iter().enumerate() {
        let mut seqs: Vec<i64> = waiting
            .iter()
            .filter_map(|item| {
                let (kind, seq) = match *item {
                    Waiting::Proposal(seq) => (0, seq),
                    Waiting::Task(seq) => (1, seq),
                    Waiting::Named(seq) => (2, seq),
                    Waiting::Reply(seq) => (3, seq),
                    Waiting::Decision(seq) => (4, seq),
                    Waiting::Report(seq) => (5, seq),
                };
                (kind == index).then_some(seq)
            })
            .collect();
        seqs.sort_unstable();
        seqs.dedup();
        match seqs.as_slice() {
            [] => {}
            [seq] => parts.push(format!("{one} {what} (message {seq})")),
            _ => {
                let list = seqs
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                parts.push(format!("{} {many} {what} (messages {list})", seqs.len()));
            }
        }
    }
    format!(
        "New in channel {channel} since message {after}: {}. Read them with channel_read (channel {channel}, since \
         {after}); end your turn when you are waiting for more.",
        parts.join("; ")
    )
}

/// The members a text names (A1): `@` and a member's name, matched without case, the longest name first; each once.
pub(crate) fn named(text: &str, members: &[(Uuid, &str)]) -> Vec<Uuid> {
    let lower = text.to_lowercase();
    let mut names: Vec<(Uuid, String)> = members
        .iter()
        .filter(|(_, name)| !name.trim().is_empty())
        .map(|(id, name)| (*id, name.trim().to_lowercase()))
        .collect();
    // The longest first, so "@Pricing analyst" is never read as "@Pricing".
    names.sort_by(|a, b| b.1.chars().count().cmp(&a.1.chars().count()));
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

#[cfg(test)]
mod tests {
    use super::{MAX_HOP, Waiting, key, message_hop, named, sentence, wake_hop};
    use uuid::Uuid;

    // TEST: GW-U-227
    #[test]
    fn gw_u_227_a_wake_is_keyed_by_the_members_place_and_its_failures() {
        let (channel, member) = (Uuid::from_u128(1), Uuid::from_u128(2));
        assert_eq!(
            key(channel, member, 7, None),
            format!("channel-wake:{}:{}:7", channel.simple(), member.simple())
        );
        assert_eq!(
            key(channel, member, 7, Some(3)),
            format!(
                "channel-wake:{}:{}:7:failed-3",
                channel.simple(),
                member.simple()
            )
        );
        // Another channel, another member or another place is another wake; the same is the same one.
        assert_ne!(
            key(channel, member, 7, None),
            key(Uuid::from_u128(3), member, 7, None)
        );
        assert_ne!(key(channel, member, 7, None), key(channel, member, 8, None));
        assert_eq!(key(channel, member, 7, None), key(channel, member, 7, None));
        // An idempotency key is at most 128 bytes.
        assert!(key(Uuid::max(), Uuid::max(), i64::MAX, Some(i64::MAX)).len() <= 128);
    }

    // TEST: GW-U-228
    #[test]
    fn gw_u_228_the_wake_names_kinds_and_seqs_and_the_call_that_reads_them() {
        let channel = Uuid::from_u128(9);
        assert_eq!(
            sentence(
                channel,
                11,
                &[
                    Waiting::Proposal(12),
                    Waiting::Task(13),
                    Waiting::Named(14),
                    Waiting::Named(15),
                    Waiting::Reply(16),
                    Waiting::Decision(17),
                    Waiting::Report(18),
                ]
            ),
            format!(
                "New in channel {channel} since message 11: a proposal for your vote (message 12); a to-do you may \
                 take on (message 13); 2 posts naming you (messages 14, 15); a reply to your post (message 16); the \
                 decision on your proposal (message 17); a report on your to-do (message 18). Read them with \
                 channel_read (channel {channel}, since 11); end your turn when you are waiting for more."
            )
        );
        assert_eq!(
            sentence(channel, 0, &[Waiting::Proposal(3), Waiting::Proposal(5)]),
            format!(
                "New in channel {channel} since message 0: 2 proposals for your vote (messages 3, 5). Read them with \
                 channel_read (channel {channel}, since 0); end your turn when you are waiting for more."
            )
        );
    }

    // TEST: GW-U-229
    #[test]
    fn gw_u_229_a_post_names_members_by_task_title_longest_first_without_case() {
        let (pricing, analyst, checker) =
            (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let members = [
            (analyst, "Analyst"),
            (pricing, "Pricing analyst"),
            (checker, "Data checker"),
        ];
        // The longest name wins where two start at the same "@".
        assert_eq!(
            named("@pricing analyst, please look", &members),
            vec![pricing]
        );
        assert_eq!(
            named("@Analyst and @DATA CHECKER: both", &members),
            vec![analyst, checker]
        );
        // Each once, in the order first named.
        assert_eq!(
            named("@Data checker then @data checker", &members),
            vec![checker]
        );
        // No "@", or a name that isn't a member's, names no one; an e-mail address isn't a mention.
        assert!(named("Analyst, please look", &members).is_empty());
        assert!(named("@Nobody here", &members).is_empty());
        assert!(named("write to analyst@example.org", &members).is_empty());
        // A name must end where a word ends: "@Analysts" isn't "@Analyst".
        assert!(named("@Analysts", &members).is_empty());
    }

    // TEST: GW-U-230
    #[test]
    fn gw_u_230_hops_start_at_a_persons_post_and_stop_at_four() {
        assert_eq!(message_hop(true, None), 0);
        assert_eq!(message_hop(true, Some(2)), 0);
        assert_eq!(message_hop(false, None), 1);
        assert_eq!(message_hop(false, Some(0)), 1);
        assert_eq!(message_hop(false, Some(3)), 4);
        // The smallest unread hop wakes: a person's post in the burst starts a fresh chain.
        assert_eq!(wake_hop(&[3, 0, 2]), Some(0));
        assert_eq!(wake_hop(&[2, 3]), Some(2));
        // At the bound nothing wakes; below it, it does.
        assert_eq!(wake_hop(&[MAX_HOP]), None);
        assert_eq!(wake_hop(&[MAX_HOP, MAX_HOP + 1]), None);
        assert_eq!(wake_hop(&[MAX_HOP, MAX_HOP - 1]), Some(MAX_HOP - 1));
        assert_eq!(wake_hop(&[]), None);
    }
}
