//! A byte heuristic: ceil(UTF-8 bytes / 3.5), with four tokens of message overhead.
use crate::Message;

/// An estimate that explicitly distinguishes heuristic counts from exact tokenization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenEstimate {
    /// Estimated tokens, saturating at the representable maximum.
    pub tokens: u32,
    /// Always true for the byte heuristic.
    pub approximate: bool,
}
fn bytes(bytes: u128) -> TokenEstimate {
    TokenEstimate {
        tokens: u32::try_from(bytes.saturating_mul(2).div_ceil(7)).unwrap_or(u32::MAX),
        approximate: true,
    }
}

/// Estimates UTF-8 text using the documented byte formula, without a route-specific tokenizer.
#[must_use]
pub fn estimate(text: &str) -> TokenEstimate {
    estimate_bytes(text.len())
}

/// Estimates an accumulated UTF-8 byte count without buffering streaming content.
/// Rounds once for the whole response, independently of transport chunk boundaries.
#[must_use]
pub fn estimate_bytes(count: usize) -> TokenEstimate {
    bytes(count as u128)
}

/// Counts what a request sends of a message: its text, its opaque reasoning states, call identifiers, names and
/// arguments, plus four overhead tokens. Reasoning text isn't sent, so it isn't counted.
#[must_use]
pub fn message_tokens(message: &Message) -> TokenEstimate {
    // Past reasoning text is never sent (only replayable opaque states are), so it takes no room in the window
    // (CORE-U-073): the model makers' guidance for thinking models is to drop earlier reasoning.
    let mut count = message.text.len() as u128;
    // Opaque reasoning states are sent back as they are (ADR-0027), so they take room (CORE-U-074). The caller keeps
    // only the states it will replay to this route.
    for state in &message.reasoning_states {
        count += state.item.as_ref().map_or(0, String::len) as u128;
    }
    for call in &message.tool_calls {
        count += call.call_id.as_str().len() as u128
            + call.name.len() as u128
            + call.arguments.len() as u128;
    }
    if let Some(call) = &message.tool_call_id {
        count += call.as_str().len() as u128;
    }
    let mut estimate = bytes(count);
    estimate.tokens = estimate.tokens.saturating_add(4);
    estimate
}
