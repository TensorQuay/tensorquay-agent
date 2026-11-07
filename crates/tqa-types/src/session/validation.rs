use super::{ReasoningSnapshot, RouteSnapshot};
use crate::{
    DEFAULT_MAX_REQUEST_BYTES, ErrorCode, FrameError, MAX_REQUEST_BYTES, MIN_REQUEST_BYTES,
};
use schemars::{Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, de::Error as _};

/// The accepted thinking budget of one reasoning value, in tokens.
const BUDGET_TOKENS: std::ops::RangeInclusive<u32> = 1024..=1_048_576;

fn invalid(path: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: path.into(),
        detail: detail.into(),
    }
}
impl ReasoningSnapshot {
    /// Validates a programmatically constructed set and its selected default.
    ///
    /// # Errors
    /// Rejects empty, oversized or duplicate values and a default outside the set.
    pub fn validate(&self) -> Result<(), FrameError> {
        if !(1..=16).contains(&self.allowed.len())
            || self
                .allowed
                .iter()
                .any(|value| value.is_empty() || value.len() > 64)
            || self
                .allowed
                .iter()
                .enumerate()
                .any(|(index, value)| self.allowed[..index].contains(value))
            || !self.allowed.contains(&self.default)
        {
            return Err(invalid(
                "reasoning",
                "expected 1 through 16 unique choices within 64 bytes and a default member",
            ));
        }
        if let Some(budgets) = &self.budget_tokens
            && (budgets.len() != self.allowed.len()
                || self
                    .allowed
                    .iter()
                    .any(|value| !budgets.contains_key(value))
                || budgets
                    .values()
                    .any(|budget| !BUDGET_TOKENS.contains(budget)))
        {
            return Err(invalid(
                "reasoning.budget_tokens",
                "expected a budget table with exactly one budget of 1024 through 1048576 tokens per allowed value",
            ));
        }
        Ok(())
    }
}
impl RouteSnapshot {
    /// Validates a programmatically constructed secret-free route snapshot.
    ///
    /// # Errors
    /// Rejects malformed relay paths, string bounds, token capacities or reasoning choices.
    pub fn validate(&self) -> Result<(), FrameError> {
        if self.name.is_empty() {
            return Err(invalid("route.name", "expected a nonempty registry name"));
        }
        if self.model.is_empty() || self.model.len() > 256 {
            return Err(invalid("route.model", "expected 1 through 256 UTF-8 bytes"));
        }
        if !relay_path(&self.relay_path) {
            return Err(invalid(
                "route.relay_path",
                "expected an absolute ASCII /relay/r/ path within 4096 bytes, without empty or parent segments, controls, query or fragment",
            ));
        }
        if self.context_window == Some(0)
            || self.max_output == Some(0)
            || self
                .max_output
                .zip(self.context_window)
                .is_some_and(|(output, window)| output > window)
        {
            return Err(invalid(
                "route",
                "token limits must be positive and output must fit the context window",
            ));
        }
        if let Some(reasoning) = &self.reasoning {
            reasoning.validate()?;
            // The protocol spends the thinking budget out of the output tokens, so each budget
            // must leave room for an answer.
            if let Some(budgets) = &reasoning.budget_tokens
                && budgets
                    .values()
                    .any(|budget| self.max_output.is_none_or(|output| *budget >= output))
            {
                return Err(invalid(
                    "route.reasoning.budget_tokens",
                    "every thinking budget must be below max_output, which must be set",
                ));
            }
        }
        if self
            .first_response_secs
            .is_some_and(|secs| !(5..=900).contains(&secs))
        {
            return Err(invalid(
                "route.first_response_secs",
                "expected 5 through 900 seconds",
            ));
        }
        if self
            .max_request_bytes
            .is_some_and(|bytes| !(MIN_REQUEST_BYTES..=MAX_REQUEST_BYTES).contains(&bytes))
        {
            return Err(invalid(
                "route.max_request_bytes",
                "expected 1024 through 16777216 bytes",
            ));
        }
        Ok(())
    }
    /// The request body budget: the gateway's stated relay limit, or the default relay limit.
    #[must_use]
    pub fn request_budget(&self) -> u32 {
        self.max_request_bytes.unwrap_or(DEFAULT_MAX_REQUEST_BYTES)
    }
}
fn relay_path(path: &str) -> bool {
    path.len() <= 4096
        && path.is_ascii()
        && path.starts_with("/relay/r/")
        && !path
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '?' | '#'))
        && path
            .split('/')
            .skip(1)
            .all(|segment| !segment.is_empty() && segment != "..")
}
pub(super) fn display<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control) {
        return Err(D::Error::custom(
            "expected display text within 4096 UTF-8 bytes, without control characters",
        ));
    }
    Ok(text)
}
pub(super) fn budget_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":["object","null"],"minProperties":1,"maxProperties":16,
        "additionalProperties":{"type":"integer","minimum":1024,"maximum":1_048_576}})
}
pub(super) fn reasoning_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"array","minItems":1,"maxItems":16,"uniqueItems":true,"items":{"type":"string","minLength":1,"maxLength":64}})
}
