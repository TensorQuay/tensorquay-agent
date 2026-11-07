//! Dedupe (ARCHITECTURE §7, CTX-U-012): a tool result whose content equals a later result's is replaced by a
//! one-line stub naming that later call, so a file read several times costs its tokens once. Every result stays
//! paired with its call, and the pass is a pure function of the projected messages, so resume stays exact.
use crate::{Message, Role};
use std::collections::HashMap;
use tqa_types::ToolCallId;

/// Results shorter than this stay whole: a stub would save little and would hide small outputs such as "ok".
pub const DEDUPE_MIN_BYTES: usize = 256;

/// The text that replaces an earlier copy of the result of `later`.
#[must_use]
pub fn dedupe_stub(later: &ToolCallId) -> String {
    format!("[same output as the later result of call {later}]")
}

/// Replaces every earlier copy of a repeated tool result, newest first, so the latest copy stays whole.
pub(crate) fn dedupe(messages: &mut [Message]) {
    let mut latest: HashMap<String, ToolCallId> = HashMap::new();
    for message in messages.iter_mut().rev() {
        if message.role != Role::Tool || message.text.len() < DEDUPE_MIN_BYTES {
            continue;
        }
        let Some(call_id) = message.tool_call_id.clone() else {
            continue;
        };
        match latest.get(&message.text) {
            Some(later) => message.text = dedupe_stub(later),
            None => {
                latest.insert(message.text.clone(), call_id);
            }
        }
    }
}
