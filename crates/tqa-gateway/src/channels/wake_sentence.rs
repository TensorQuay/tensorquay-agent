//! The wake's fixed words (ADR-0014's amendment, A4, A9 and the decision of 20 Nov 2026 on a person's post to the whole
//! channel): what waits, by kind and seq, the files placed for it, and the channel's manners; never anyone's words.
use super::Waiting;
use uuid::Uuid;

/// What every wake ends with (decided on 20 Nov 2026, replacing C-4's one sentence; to-dos one at a time, 21 Nov 2026):
/// the channel's manners, fixed text, never anyone's words.
pub(super) const MANNERS: &str = "The people and agents in this channel don't see your task: whatever they should read, post it \
                       with channel_post, as a reply (parent) to the message you answer. A post to the whole channel \
                       is for whoever can help: read what others already answered, then answer it if nobody has, or \
                       add only what is missing. Take on one to-do at a time: claim it with task_claim before \
                       working on it, report it with channel_post when done, then claim another if any is left; leave \
                       to-dos others have claimed to them. Stay silent only when a post is plainly not for you or already \
                       answered. Keep posts short, with no thanks or acknowledgements. End your turn when you have \
                       posted or have nothing to add.";

/// The wake's fixed sentence (A4).
pub(crate) fn sentence(channel: Uuid, after: i64, waiting: &[Waiting]) -> String {
    // Each kind once, in this order, with its seqs in the log's order.
    const KINDS: [(&str, &str, &str); 7] = [
        ("a proposal", "proposals", "for your vote"),
        ("a to-do", "to-dos", "open for claims"),
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
    format!(
        "New in channel {channel} since message {after}: {}. Read them with channel_read (channel {channel}, since \
         {after}). {MANNERS}",
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
