//! The people's routes' rules, apart from their I/O (ADR-0034, the slice S6 contract): a messages page's parameters
//! an answer's fields, and a person's message cut for the lead. The 100 % class (ENGINEERING-GUARDRAILS §2.2, parsers and validators): every branch is
//! unit-tested here.
use crate::{GatewayError, InvalidReason};

/// A messages page's size, by default and at most.
const DEFAULT_MESSAGES: u32 = 100;
const MAX_MESSAGES: u32 = 200;
/// An answer's text bound, as a person's answer to the lead's question.
const MAX_ANSWER_BYTES: usize = 16_000;

/// `after` and `limit`, each at most once.
pub(super) fn page(query: Option<&str>) -> Result<(u64, u32), GatewayError> {
    let (mut after, mut limit) = (None, None);
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        let invalid = |field| GatewayError::Invalid {
            field,
            reason: InvalidReason::ParameterInvalid,
        };
        match key.as_ref() {
            "after" if after.is_none() => {
                after = Some(value.parse::<u64>().map_err(|_| invalid("after"))?);
            }
            "limit" if limit.is_none() => {
                limit = Some(
                    value
                        .parse::<u32>()
                        .ok()
                        .filter(|limit| (1..=MAX_MESSAGES).contains(limit))
                        .ok_or_else(|| invalid("limit"))?,
                );
            }
            _ => return Err(invalid("query")),
        }
    }
    Ok((after.unwrap_or(0), limit.unwrap_or(DEFAULT_MESSAGES)))
}

/// An answer to the lead's question: its `question_seq` 1 or more, its text 1 to 16,000 bytes without NUL.
///
/// # Errors
/// `400` naming `text` or `question_seq`.
pub(super) fn answer(question_seq: u64, text: &str) -> Result<(), GatewayError> {
    let invalid = |field| GatewayError::Invalid {
        field,
        reason: InvalidReason::ParameterInvalid,
    };
    if text.is_empty() || text.len() > MAX_ANSWER_BYTES || text.contains('\0') {
        return Err(invalid("text"));
    }
    if question_seq == 0 {
        return Err(invalid("question_seq"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "people_rules_tests.rs"]
mod tests;

/// A person's message to a helper, cut for the lead's copy: at most `limit` bytes on a character boundary, ending with
/// "…" when cut, so a long message still reaches the lead.
pub(super) fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
