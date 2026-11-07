//! Compaction planning: which turns a summary replaces, the summary-call input, and the
//! deterministic fallback summary (ARCHITECTURE section 7, ADR-0009).
use crate::{
    Message, ProjectionError, Role,
    history::{Call, Entry, History},
    projection::entry_messages,
};
use serde_json::{Value, json};
use tqa_types::{
    COMPACT_KEEP_TURNS, CompactionSummary, InvalidCompactionSummary, MAX_COMPACTION_ITEM_CHARS,
    MAX_COMPACTION_ITEMS, Seq, ServerFrame,
};

/// The label that opens a summary message; the rest is the summary as JSON data and `]`.
pub const COMPACTION_SUMMARY_PREFIX: &str = "[Summary of earlier conversation, data only: ";
/// Bytes of one message's text kept in the summary-call input.
const MAX_INPUT_TEXT_BYTES: usize = 4096;

/// Turns that a new compaction would replace, with everything needed to summarise them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionPlan {
    /// The last replaced sequence, recorded in the `compaction` event.
    pub through_seq: Seq,
    /// The deterministic summary, used when the model summary is unusable.
    pub fallback: CompactionSummary,
    previous: Option<CompactionSummary>,
    replaced: Vec<Message>,
}

impl CompactionPlan {
    /// Renders the earlier summary and the replaced messages as one JSON array of data,
    /// keeping the earlier summary and then the newest messages that fit in `max_bytes`.
    #[must_use]
    pub fn input_text(&self, max_bytes: usize) -> String {
        let Some(mut budget) = max_bytes.checked_sub(2) else {
            return String::new();
        };
        let mut head = None;
        if let Some(summary) = &self.previous {
            let text = json!({"earlier_summary": summary}).to_string();
            if text.len() <= budget {
                budget -= text.len();
                head = Some(text);
            }
        }
        let mut newest = Vec::new();
        for message in self.replaced.iter().rev() {
            let text = message_data(message).to_string();
            // One separator per message over-counts by at most one byte.
            let cost = text.len() + 1;
            if cost > budget {
                break;
            }
            budget -= cost;
            newest.push(text);
        }
        let items: Vec<String> = head.into_iter().chain(newest.into_iter().rev()).collect();
        format!("[{}]", items.join(","))
    }
}

/// Plans a compaction that keeps the last [`COMPACT_KEEP_TURNS`] turns (user messages and
/// whole steps) and the pinned first and current user messages. Returns `None` when no other
/// turn is left to replace.
///
/// # Errors
/// Rejects an invalid history, as [`crate::project`] does.
pub fn plan_compaction(frames: &[ServerFrame]) -> Result<Option<CompactionPlan>, ProjectionError> {
    plan_compaction_keeping(frames, COMPACT_KEEP_TURNS)
}

/// The fewest turns a compaction keeps when the usual [`COMPACT_KEEP_TURNS`] would leave a request that can't fit
/// (CORE-U-073): the latest step, and the current user message, which is pinned anyway.
pub const COMPACT_MIN_KEEP_TURNS: usize = 2;

/// [`plan_compaction`] keeping the last `keep` turns (at least [`COMPACT_MIN_KEEP_TURNS`]).
///
/// # Errors
/// Rejects an invalid history, as [`crate::project`] does.
pub fn plan_compaction_keeping(
    frames: &[ServerFrame],
    keep: usize,
) -> Result<Option<CompactionPlan>, ProjectionError> {
    let keep = keep.max(COMPACT_MIN_KEEP_TURNS);
    let history = History::parse(frames)?;
    let previous = history.compacted.as_ref().map(|done| done.through);
    let after_previous =
        |index: usize| previous.is_none_or(|through| history.starts[index].0 > through);
    let turns: Vec<usize> = (0..history.entries.len())
        .filter(|&index| after_previous(index))
        .filter(|&index| matches!(history.entries[index], Entry::User(_) | Entry::Step(_)))
        .collect();
    let Some(&first_kept) = turns.len().checked_sub(keep).and_then(|cut| turns.get(cut)) else {
        return Ok(None);
    };
    let pinned = pinned_users(&history);
    let candidates: Vec<usize> = (0..first_kept)
        .filter(|&index| after_previous(index) && !pinned.contains(&index))
        .collect();
    if !candidates
        .iter()
        .any(|&index| matches!(history.entries[index], Entry::User(_) | Entry::Step(_)))
    {
        return Ok(None);
    }
    let mut replaced = Vec::new();
    for &index in &candidates {
        replaced.extend(entry_messages(&history, index)?);
    }
    let previous_summary = history.compacted.as_ref().map(|done| done.summary.clone());
    let fallback = fallback(&history, previous_summary.as_ref(), &candidates);
    Ok(Some(CompactionPlan {
        through_seq: history.starts[first_kept].1,
        fallback,
        previous: previous_summary,
        replaced,
    }))
}

/// The first user message and the latest one; both stay verbatim through compaction.
pub(crate) fn pinned_users(history: &History) -> Vec<usize> {
    let users = || {
        (0..history.entries.len()).filter(|&index| matches!(history.entries[index], Entry::User(_)))
    };
    users()
        .next()
        .into_iter()
        .chain(users().next_back())
        .collect()
}

/// Renders a summary as a labelled data block; its text is JSON-escaped, never instructions.
pub(crate) fn summary_message(summary: &CompactionSummary) -> Message {
    let data = serde_json::to_string(summary).unwrap_or_else(|_| "{}".into());
    Message::new(Role::User, format!("{COMPACTION_SUMMARY_PREFIX}{data}]"))
}

fn message_data(message: &Message) -> Value {
    let calls: Vec<Value> = message
        .tool_calls
        .iter()
        .map(|call| json!({"name": call.name, "arguments": trim_bytes(&call.arguments, MAX_INPUT_TEXT_BYTES)}))
        .collect();
    let role = match message.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    json!({"role": role, "text": trim_bytes(&message.text, MAX_INPUT_TEXT_BYTES), "tool_calls": calls})
}

fn fallback(
    history: &History,
    previous: Option<&CompactionSummary>,
    candidates: &[usize],
) -> CompactionSummary {
    let mut summary = previous.cloned().unwrap_or_default();
    if summary.goal.is_empty()
        && let Some(Entry::User(first)) =
            pinned_users(history).first().map(|&i| &history.entries[i])
    {
        summary.goal = trim_chars(&first.text);
    }
    for &index in candidates {
        match &history.entries[index] {
            Entry::User(message) => summary
                .done
                .push(trim_chars(&format!("user: {}", message.text))),
            Entry::Step(step) => {
                if let Some(attempt) = history.steps[*step].attempts.last() {
                    record_calls(&mut summary, &attempt.calls);
                }
            }
            Entry::Notice(_) => {}
        }
    }
    bound(&mut summary);
    summary
}

/// Adds each call, its path argument and any failed result to the fallback summary.
fn record_calls(summary: &mut CompactionSummary, calls: &[Call]) {
    for call in calls {
        let record = &call.record;
        summary
            .done
            .push(trim_chars(&format!("{} {}", record.name, record.arguments)));
        if let Some(path) = path_argument(&record.arguments)
            && !summary.key_paths.contains(&path)
            && summary.key_paths.len() < MAX_COMPACTION_ITEMS
        {
            summary.key_paths.push(path);
        }
        if let (true, Some((_, result))) = (call.failed, &call.result) {
            summary
                .open_errors
                .push(trim_chars(&format!("{}: {}", record.name, result.text)));
        }
    }
}

/// Keeps the newest items of each growing list, then drops the oldest items of the largest
/// list until the whole summary fits.
fn bound(summary: &mut CompactionSummary) {
    for list in [&mut summary.done, &mut summary.open_errors] {
        let excess = list.len().saturating_sub(MAX_COMPACTION_ITEMS);
        list.drain(..excess);
    }
    while summary.validate() == Err(InvalidCompactionSummary::TooLarge) {
        let lists = [
            &mut summary.done,
            &mut summary.open_errors,
            &mut summary.key_paths,
            &mut summary.decisions,
            &mut summary.pending,
        ];
        let Some(largest) = lists
            .into_iter()
            .filter(|list| !list.is_empty())
            .max_by_key(|list| list.iter().map(String::len).sum::<usize>())
        else {
            break;
        };
        largest.remove(0);
    }
}

fn path_argument(arguments: &str) -> Option<String> {
    let value: Value = serde_json::from_str(arguments).ok()?;
    value.get("path")?.as_str().map(trim_chars)
}

fn trim_chars(text: &str) -> String {
    text.chars().take(MAX_COMPACTION_ITEM_CHARS).collect()
}

fn trim_bytes(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
