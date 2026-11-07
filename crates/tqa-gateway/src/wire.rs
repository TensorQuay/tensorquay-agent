use crate::GatewayError;
use tqa_types::{Event, SandboxState, Seq, ServerFrame};

pub(crate) fn sequence(frame: &ServerFrame) -> Option<Seq> {
    match frame {
        ServerFrame::Event { seq, .. } | ServerFrame::PermissionRequest { seq, .. } => Some(*seq),
        ServerFrame::Error { seq, .. } => *seq,
        _ => None,
    }
}

/// The client-facing sandbox state a stored run lifecycle event moves to (ARCHITECTURE §4.4).
pub(crate) const fn run_transition(frame: &ServerFrame) -> Option<SandboxState> {
    match frame {
        ServerFrame::Event {
            event: Event::RunStarted { .. } | Event::RunResumed { .. },
            ..
        } => Some(SandboxState::Busy),
        ServerFrame::Event {
            event: Event::RunCompleted { .. } | Event::RunCancelled { .. } | Event::RunFailed { .. },
            ..
        } => Some(SandboxState::Idle),
        _ => None,
    }
}

pub(crate) fn validated(frame: &ServerFrame) -> Result<ServerFrame, GatewayError> {
    let bytes = serde_json::to_vec(frame).map_err(|_| GatewayError::BadRequest("invalid frame"))?;
    ServerFrame::parse(&bytes).map_err(|error| GatewayError::Protocol(error.code))
}

pub(crate) fn db_sequence(seq: Seq) -> Result<i64, GatewayError> {
    seq.to_string()
        .parse::<i64>()
        .map_err(|_| GatewayError::BadRequest("sequence exceeds store limit"))
}

pub(crate) fn from_db_sequence(seq: i64) -> Result<Seq, GatewayError> {
    u64::try_from(seq)
        .map(Seq::from)
        .map_err(|_| GatewayError::StoreUnavailable)
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
