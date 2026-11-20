//! The wake's fixed words (ADR-0014's amendment, A4, A9 and the decision of 20 Nov 2026 on a person's post to the whole
//! channel): what waits, by kind and seq, the files placed for it, and the one sentence of manners; never anyone's words.
use super::Waiting;
use uuid::Uuid;

/// What a wake for a post to the whole channel adds (C-4): the one sentence of manners it needs, fixed text.
const MANNERS: &str = " Answer a post to the whole channel only when it falls to you or you can add what the others \
                       did not; otherwise end your turn without posting.";

/// The wake's fixed sentence (A4).
pub(crate) fn sentence(channel: Uuid, after: i64, waiting: &[Waiting]) -> String {
    // Each kind once, in this order, with its seqs in the log's order.
    const KINDS: [(&str, &str, &str); 7] = [
        ("a proposal", "proposals", "for your vote"),
        ("a to-do", "to-dos", "you may take on"),
        ("a post", "posts", "naming you"),
        ("a post", "posts", "to the channel"),
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
                    Waiting::Channel(seq) => (3, seq),
                    Waiting::Reply(seq) => (4, seq),
                    Waiting::Decision(seq) => (5, seq),
                    Waiting::Report(seq) => (6, seq),
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
    let manners = if waiting
        .iter()
        .any(|item| matches!(item, Waiting::Channel(_)))
    {
        MANNERS
    } else {
        ""
    };
    format!(
        "New in channel {channel} since message {after}: {}. Read them with channel_read (channel {channel}, since \
         {after}); end your turn when you are waiting for more.{manners}",
        parts.join("; ")
    )
}

/// The sentence the wake adds for the files placed for it (A9): how many wait and from which messages, never a path,
/// since a channel's or a file's name is someone's words (SEC-068); `channel_read` lists the paths. `seqs` has each
/// placed file's message.
pub(crate) fn files_sentence(seqs: &[i64]) -> String {
    let mut messages: Vec<i64> = seqs.to_vec();
    messages.sort_unstable();
    messages.dedup();
    let named = match messages.as_slice() {
        [] => return String::new(),
        [one] => format!("message {one}"),
        [rest @ .., last] => format!(
            "messages {} and {last}",
            rest.iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    if seqs.len() == 1 {
        format!(" 1 file waits in your workspace, from {named}; channel_read lists its path.")
    } else {
        format!(
            " {} files wait in your workspace, from {named}; channel_read lists their paths.",
            seqs.len()
        )
    }
}
