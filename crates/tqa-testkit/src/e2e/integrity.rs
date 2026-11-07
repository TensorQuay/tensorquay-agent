//! Event-log integrity (TK-U-003): the replayed log is contiguous from sequence 1, and every frame the
//! stream delivered appears in it unchanged and in order.
use serde::Serialize;
use std::collections::BTreeMap;
use tqa_types::{Seq, SeqChecker, SeqError, ServerFrame};

/// The first way the log was found incomplete.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "fault", rename_all = "snake_case")]
pub enum IntegrityFault {
    /// The replay skips or repeats a sequence number.
    Gap {
        /// The next contiguous number.
        expected: Seq,
        /// The number found instead.
        got: Seq,
    },
    /// The stream delivered a sequence number again or out of order.
    Duplicate {
        /// The repeated number.
        seq: Seq,
    },
    /// A streamed frame is absent from the replay or differs from it.
    Mismatch {
        /// The frame's number.
        seq: Seq,
    },
}
impl IntegrityFault {
    /// The sequence number that shows the fault.
    #[must_use]
    pub const fn seq(self) -> Seq {
        match self {
            Self::Gap { got, .. } => got,
            Self::Duplicate { seq } | Self::Mismatch { seq } => seq,
        }
    }
}

/// The journal sequence of a frame; heartbeats, session frames and errors carry none.
#[must_use]
pub fn seq(frame: &ServerFrame) -> Option<Seq> {
    match frame {
        ServerFrame::Event { seq, .. } | ServerFrame::PermissionRequest { seq, .. } => Some(*seq),
        _ => None,
    }
}

/// Compares what the stream delivered with the authoritative replay.
///
/// # Errors
/// The first [`IntegrityFault`] found, replay order first.
pub fn verify(streamed: &[ServerFrame], replayed: &[ServerFrame]) -> Result<(), IntegrityFault> {
    let mut checker = SeqChecker::new(Seq::ZERO);
    let mut journal = BTreeMap::new();
    for (number, frame) in replayed
        .iter()
        .filter_map(|frame| Some((seq(frame)?, frame)))
    {
        checker.accept(number).map_err(|error| match error {
            SeqError::Gap { expected, got } => IntegrityFault::Gap { expected, got },
            SeqError::Exhausted => IntegrityFault::Mismatch { seq: number },
        })?;
        journal.insert(number, frame);
    }
    let mut last = Seq::ZERO;
    for (number, frame) in streamed
        .iter()
        .filter_map(|frame| Some((seq(frame)?, frame)))
    {
        if number <= last {
            return Err(IntegrityFault::Duplicate { seq: number });
        }
        last = number;
        if journal.get(&number) != Some(&frame) {
            return Err(IntegrityFault::Mismatch { seq: number });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
