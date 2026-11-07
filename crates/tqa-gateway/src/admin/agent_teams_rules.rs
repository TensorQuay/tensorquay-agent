//! The acceptance check's rule, apart from its I/O (ADR-0034, the slice S6 contract): one line of 1 to 4,096 bytes.
//! The 100 % class (ENGINEERING-GUARDRAILS §2.2, a validator): every branch is unit-tested here.
use crate::InvalidReason;

/// The acceptance check's bound: one line of at most this many bytes.
const MAX_CHECK_BYTES: usize = 4096;

/// Why an acceptance check is refused, if it is: empty or blank (`value_empty`), over 4,096 bytes (`value_too_long`),
/// or more than one line or holding NUL (`value_invalid`).
pub(super) fn refused_check(check: &str) -> Option<InvalidReason> {
    if check.trim().is_empty() {
        Some(InvalidReason::ValueEmpty)
    } else if check.len() > MAX_CHECK_BYTES {
        Some(InvalidReason::ValueTooLong)
    } else if check.contains(['\n', '\r', '\0']) {
        Some(InvalidReason::ValueInvalid)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "agent_teams_tests.rs"]
mod tests;
