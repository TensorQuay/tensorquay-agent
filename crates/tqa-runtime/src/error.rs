//! Content-free process failures; credentials and received payloads never become diagnostics.
use tqa_types::ErrorCode;

/// A runtime boundary failed; display and debug contain only fixed classifications.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RuntimeError {
    /// A required startup input is absent or invalid.
    #[error("invalid runtime configuration: {0}")]
    Configuration(&'static str),
    /// The initial registration deadline elapsed.
    #[error("the gateway never accepted the runtime token")]
    StartDeadline,
    /// A reconnect tried to change the process's frozen security contract.
    #[error("session configuration changed: {0}")]
    SnapshotChanged(&'static str),
    /// Authoritative history or its WAL tail has a sequence gap.
    #[error("runtime history is not contiguous")]
    History,
    /// An explicit allocation or queue bound was reached.
    #[error("runtime resource limit reached")]
    Limit,
    /// A durable append, sync or worker acknowledgement failed: which step, and the system's error kind when there is
    /// one, so the log names the WAL write, its sync or a closed channel (GUARDRAILS §1.5).
    #[error("runtime storage I/O failed: {0}")]
    Io(IoCause),
    /// The link transport failed without retaining its potentially sensitive diagnostics.
    #[error("runtime link unavailable")]
    Link,
    /// A bounded operation did not finish in time.
    #[error("runtime operation deadline reached")]
    Timeout,
    /// The workspace flush before a tool result failed or did not finish within its bound; the result is not
    /// journaled (ARCHITECTURE §4.5, `sync_before_result`).
    #[error("the workspace could not be flushed before the tool result")]
    WorkspaceSync,
    /// A typed wire boundary refused the received data.
    #[error("invalid runtime frame")]
    Frame,
    /// Shutdown interrupted an operation before it started.
    #[error("runtime stopping")]
    Stopped,
    /// An owned task stopped unexpectedly.
    #[error("runtime worker failed")]
    Worker,
}
impl RuntimeError {
    /// Returns the stable operating-system exit status for this failure.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Configuration(_) => 2,
            Self::StartDeadline => 3,
            Self::SnapshotChanged(_) => 4,
            Self::Stopped => 0,
            _ => 1,
        }
    }
    /// Returns the protocol classification without exposing transport or storage details.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::History => ErrorCode::SeqGap,
            Self::Frame | Self::Configuration(_) | Self::SnapshotChanged(_) => {
                ErrorCode::InvalidFrame
            }
            Self::Limit => ErrorCode::Budget,
            Self::Timeout
            | Self::StartDeadline
            | Self::Io(_)
            | Self::Link
            | Self::WorkspaceSync => ErrorCode::Io,
            Self::Stopped => ErrorCode::Restarting,
            Self::Worker => ErrorCode::Internal,
        }
    }
}

/// Which durable step failed, and how: fixed words and the system's error kind, never a path or a payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IoCause {
    /// The step, in fixed words: `wal append`, `wal sync`, `link frames closed`, …
    pub step: &'static str,
    /// Why, in fixed words, when the step's own error says (`full`, `sequence`, `invalid frame`, …).
    pub reason: Option<&'static str>,
    /// The operating system's error kind, when the step failed in a system call.
    pub kind: Option<std::io::ErrorKind>,
}
impl IoCause {
    /// A step that failed without a system error (a closed channel, a refused state).
    #[must_use]
    pub const fn step(step: &'static str) -> Self {
        Self {
            step,
            reason: None,
            kind: None,
        }
    }
    /// A step whose system call failed with `error`; only its kind is kept.
    #[must_use]
    pub fn of(step: &'static str, error: &std::io::Error) -> Self {
        Self {
            step,
            reason: None,
            kind: Some(error.kind()),
        }
    }
    /// A write-ahead log step: why the log refused, or the system's kind when its I/O failed.
    #[must_use]
    pub fn wal(step: &'static str, error: &tqa_context::WalError) -> Self {
        use tqa_context::WalError;
        let reason = match error {
            WalError::Locked => "locked",
            WalError::Full => "full",
            WalError::Sequence(_) => "sequence",
            WalError::InvalidFrame => "invalid frame",
            WalError::Corrupt => "corrupt",
            WalError::NeedsRecovery => "needs recovery",
            WalError::Io(io) => return Self::of(step, io),
        };
        Self {
            step,
            reason: Some(reason),
            kind: None,
        }
    }
}
impl std::fmt::Display for IoCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.step)?;
        match (self.reason, self.kind) {
            (Some(reason), _) => write!(f, " ({reason})"),
            (None, Some(kind)) => write!(f, " ({kind:?})"),
            (None, None) => Ok(()),
        }
    }
}
impl RuntimeError {
    /// [`RuntimeError::Io`] for a step that failed without a system error.
    #[must_use]
    pub const fn io(step: &'static str) -> Self {
        Self::Io(IoCause::step(step))
    }
}
