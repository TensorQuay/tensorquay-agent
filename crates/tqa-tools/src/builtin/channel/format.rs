//! What the model sees of channel messages: our own header line with the cursor, then one untrusted-content block per
//! message whose attributes come from the gateway's record (seq, type, author), never from the body (SEC-035).
use super::{ChannelPage, PageMessage};
use crate::{ToolError, untrusted};
use std::fmt::Write as _;
use tqa_types::{AuthorKind, ChannelMessage, MessageBody, MessageType};

/// The most bytes one read or wait result takes; a page stops at the last whole message that fits.
pub(crate) const BUDGET: usize = 64 * 1024;

/// A page as the model reads it, at most `budget` bytes.
///
/// # Errors
/// Only if the marker refuses its fixed names, which is a bug.
pub(crate) fn page(channel: &str, page: &ChannelPage, budget: usize) -> Result<String, ToolError> {
    if page.messages.is_empty() {
        return Ok(format!(
            "channel {channel}: no new messages; next since={}\n",
            page.next_since
        ));
    }
    let mut blocks = Vec::new();
    let mut used = 0;
    for message in &page.messages {
        let block = match message {
            PageMessage::Known(message) => block(channel, message)?,
            PageMessage::Unrecognised(value) => unrecognised(channel, value)?,
        };
        // Room for the header, which is short and bounded.
        if used + block.len() + 1 + 200 > budget && !blocks.is_empty() {
            break;
        }
        used += block.len() + 1;
        blocks.push((message.seq(), block));
    }
    // A message without a seq (never sent by a well-behaved gateway) doesn't move the cursor.
    let seqs: Vec<u64> = blocks.iter().filter_map(|(seq, _)| *seq).collect();
    let cut = blocks.len() < page.messages.len();
    let next = match seqs.last() {
        Some(last) if cut => *last,
        _ => page.next_since,
    };
    let range = match (seqs.first(), seqs.last()) {
        (Some(first), Some(last)) => format!("{first}-{last}"),
        _ => "without seq".to_owned(),
    };
    let mut out = format!(
        "channel {channel}: messages {range} ({}); next since={next}",
        blocks.len()
    );
    if cut || page.more {
        let _ = write!(out, "; more after {next}");
    }
    out.push('\n');
    for (_, block) in blocks {
        out.push_str(&block);
        out.push('\n');
    }
    Ok(out)
}

/// One message as a marked block.
fn block(channel: &str, message: &ChannelMessage) -> Result<String, ToolError> {
    let seq = message.seq.to_string();
    let author = message
        .author
        .name
        .clone()
        .unwrap_or_else(|| message.author.id.clone());
    let parent = message.parent.map(|parent| parent.to_string());
    let task = message.body.task_seq().map(|task| task.to_string());
    let mut attributes = vec![
        ("channel", channel),
        ("seq", seq.as_str()),
        ("type", type_name(message.body.message_type())),
        ("author_kind", kind_name(message.author.kind)),
        ("author", author.as_str()),
    ];
    if let Some(parent) = &parent {
        attributes.push(("parent", parent));
    }
    if let Some(task) = &task {
        attributes.push(("task_seq", task));
    }
    untrusted::wrap("channel", &attributes, &body_text(&message.body))
}

/// A message this runtime doesn't recognise, as one marked block: its seq when it has one, and the gateway's JSON as
/// the body, which the marker keeps from closing the block.
fn unrecognised(channel: &str, value: &serde_json::Value) -> Result<String, ToolError> {
    let seq = value["seq"]
        .as_u64()
        .map_or_else(|| "unknown".to_owned(), |seq| seq.to_string());
    let attributes = [
        ("channel", channel),
        ("seq", seq.as_str()),
        ("type", "unknown"),
    ];
    let body = format!("a message this runtime does not recognise:\n{value}");
    untrusted::wrap("channel", &attributes, &body)
}

/// A typed body in plain words.
pub(crate) fn body_text(body: &MessageBody) -> String {
    match body {
        MessageBody::Text { text } => text.clone(),
        MessageBody::Proposal {
            title,
            text,
            window_seconds,
        } => {
            let window = window_seconds.unwrap_or(tqa_types::DEFAULT_PROPOSAL_WINDOW_SECONDS);
            format!("{title}\n\n{text}\n\nwindow: {window} s")
        }
        MessageBody::Vote {
            proposal_seq,
            choice,
            reason,
        } => {
            let choice = serde_json::to_value(choice)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            match reason {
                Some(reason) => format!("{choice} on #{proposal_seq}: {reason}"),
                None => format!("{choice} on #{proposal_seq}"),
            }
        }
        MessageBody::Decision {
            proposal_seq,
            outcome,
            rule,
        } => {
            let words = |value: serde_json::Result<serde_json::Value>| {
                value
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default()
            };
            format!(
                "#{proposal_seq} {} ({} rule)",
                words(serde_json::to_value(outcome)),
                words(serde_json::to_value(rule))
            )
        }
        MessageBody::Task {
            title,
            spec,
            claims,
        } => {
            let claims = match claims {
                tqa_types::ClaimLimit::Count(1) => "claims: 1".to_owned(),
                tqa_types::ClaimLimit::Count(count) => format!("claims: up to {count}"),
                tqa_types::ClaimLimit::All => "claims: every voting member".to_owned(),
            };
            format!("{title}\n\n{spec}\n\n{claims}")
        }
        MessageBody::Claim { task_seq } => format!("claims task #{task_seq}"),
        MessageBody::Report {
            task_seq,
            status,
            summary,
            refs,
        } => {
            let status = serde_json::to_value(status)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            let mut text = format!("task #{task_seq} {status}: {summary}");
            if !refs.is_empty() {
                let _ = write!(text, "\nrefs: {}", refs.join(", "));
            }
            text
        }
        MessageBody::Done { summary } => summary.clone().unwrap_or_else(|| "done".into()),
    }
}

pub(crate) const fn type_name(kind: MessageType) -> &'static str {
    match kind {
        MessageType::Text => "text",
        MessageType::Proposal => "proposal",
        MessageType::Vote => "vote",
        MessageType::Decision => "decision",
        MessageType::Task => "task",
        MessageType::Claim => "claim",
        MessageType::Report => "report",
        MessageType::Done => "done",
    }
}

const fn kind_name(kind: AuthorKind) -> &'static str {
    match kind {
        AuthorKind::Conversation => "conversation",
        AuthorKind::User => "user",
        AuthorKind::Gateway => "gateway",
    }
}
