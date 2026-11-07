//! Contiguous gateway-authoritative history, followed only by the unacknowledged local tail.
use crate::RuntimeError;
use tqa_types::{Seq, ServerFrame};

pub(crate) fn sequence(frame: &ServerFrame) -> Option<Seq> {
    match frame {
        ServerFrame::Event { seq, .. } | ServerFrame::PermissionRequest { seq, .. } => Some(*seq),
        ServerFrame::Error { seq, .. } => *seq,
        _ => None,
    }
}
/// Joins the complete gateway prefix through its watermark and the WAL tail strictly after it.
///
/// # Errors
/// Refuses gaps, duplicates, out-of-order or unsequenced frames and mismatched WAL record numbers.
pub fn merge_history(
    mut stored: Vec<ServerFrame>,
    tail: Vec<(Seq, ServerFrame)>,
    watermark: Seq,
) -> Result<Vec<ServerFrame>, RuntimeError> {
    let mut last = Seq::ZERO;
    for frame in &stored {
        advance(&mut last, frame)?;
    }
    if last != watermark {
        return Err(RuntimeError::History);
    }
    for (seq, frame) in tail {
        if sequence(&frame) != Some(seq) {
            return Err(RuntimeError::History);
        }
        if seq <= watermark {
            continue;
        }
        advance(&mut last, &frame)?;
        stored.push(frame);
    }
    Ok(stored)
}
pub(crate) fn advance(last: &mut Seq, frame: &ServerFrame) -> Result<(), RuntimeError> {
    let next = sequence(frame).ok_or(RuntimeError::History)?;
    if last.next() != Some(next) {
        return Err(RuntimeError::History);
    }
    *last = next;
    Ok(())
}
