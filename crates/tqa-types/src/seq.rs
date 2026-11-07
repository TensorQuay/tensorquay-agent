//! Contiguous sequence checking without wrapping at exhaustion.

use crate::{ErrorCode, Seq};

/// A frame cannot follow the last accepted sequence number.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SeqError {
    /// A missing, duplicate or reordered frame requires sequence-gap recovery.
    #[error("seq_gap: expected {expected}, got {got}")]
    Gap {
        /// The next contiguous sequence number.
        expected: Seq,
        /// The received sequence number.
        got: Seq,
    },
    /// No number follows the largest sequence; this is an internal protocol limit.
    #[error("sequence number space is exhausted")]
    Exhausted,
}

impl SeqError {
    /// Returns the stable wire code for this sequence failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::Gap { .. } => ErrorCode::SeqGap,
            Self::Exhausted => ErrorCode::Internal,
        }
    }
}

/// Accepts contiguous sequence numbers and preserves its position on error.
#[derive(Clone, Debug)]
pub struct SeqChecker {
    expected: Option<Seq>,
}

impl SeqChecker {
    /// Starts after the last accepted sequence, or exhausted when that value is maximal.
    #[must_use]
    pub const fn new(last_seq: Seq) -> Self {
        Self {
            expected: last_seq.next(),
        }
    }

    /// Accepts exactly the next sequence and advances the checker.
    ///
    /// # Errors
    /// Returns a gap for a noncontiguous value or exhaustion after the maximum sequence.
    pub fn accept(&mut self, got: Seq) -> Result<(), SeqError> {
        let expected = self.expected.ok_or(SeqError::Exhausted)?;
        if got != expected {
            return Err(SeqError::Gap { expected, got });
        }
        self.expected = got.next();
        Ok(())
    }
}
