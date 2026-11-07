//! Stable call failures and run outcomes without host-specific diagnostics.
use crate::SinkError;
use tqa_types::ErrorCode;

/// Failure to execute the loop contract itself; run failures are emitted outcomes.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// The append boundary failed and prohibits further writes.
    #[error(transparent)]
    Sink(#[from] SinkError),
    /// The supplied journal contradicts protocol state or sequence ordering.
    #[error(transparent)]
    History(#[from] tqa_context::ProjectionError),
    /// A prior user message or run must be resumed before accepting another message.
    #[error("busy: the existing run must be resumed first")]
    Busy,
    /// Programmatic inputs failed the same checks used at the wire boundary.
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(&'static str),
    /// A user turn failed the wire validation contract.
    #[error(transparent)]
    InvalidTurn(#[from] tqa_types::FrameError),
    /// An internal invariant failed without revealing event or argument contents.
    #[error("internal: {0}")]
    Internal(&'static str),
}
impl CoreError {
    /// Returns the stable code for the supervisor.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Sink(error) => error.code,
            Self::Busy => ErrorCode::Busy,
            Self::InvalidConfiguration(_) | Self::InvalidTurn(_) => ErrorCode::InvalidFrame,
            Self::History(_) | Self::Internal(_) => ErrorCode::Internal,
        }
    }
}
/// The result of advancing a journal, separate from append or input failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    /// No message or unfinished run was present.
    Idle,
    /// The run ended successfully.
    Completed,
    /// User cancellation was durably reflected.
    Cancelled,
    /// Shutdown stopped execution without closing the resumable journal.
    Interrupted,
    /// The run ended with a typed failure event.
    Failed {
        /// Stable run failure classification.
        code: ErrorCode,
        /// Bounded reason suitable for the model and user.
        reason: String,
    },
}
