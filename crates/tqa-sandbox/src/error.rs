use std::{borrow::Cow, fmt};

/// The exit code of an environment that failed before the runtime ran: the microVM guest init's `INIT_FAILED`
/// (tqa-vm-init `exit_record`, for example a workspace disk that won't mount) and Docker's "command cannot be
/// invoked". The runtime itself exits only 0 to 4 (`RuntimeError::exit_code`).
pub const ENVIRONMENT_FAILED_EXIT: i32 = 126;

/// Stable sandbox failure categories; foreign backend details never reach formatting.
#[derive(Clone, Eq, PartialEq, thiserror::Error)]
pub enum SandboxError {
    /// A required host capability is unavailable.
    #[error("sandbox preflight failed: {0}")]
    Preflight(&'static str),
    /// The registry or local store does not contain the pinned image.
    #[error("sandbox image digest was not found")]
    ImageNotFound,
    /// The inspected image does not attest the requested repository digest.
    #[error("sandbox image digest does not match")]
    ImageDigestMismatch,
    /// A specification is invalid; the reason never echoes input values.
    #[error("invalid sandbox field {field}: {reason}")]
    InvalidSpec {
        /// Name of the rejected contract field.
        field: &'static str,
        /// A bounded explanation. Usually static; it may name keys (never values), as for image environment.
        reason: Cow<'static, str>,
    },
    /// A backend operation failed. The payload is private diagnostic context, always redacted.
    #[error("sandbox backend operation failed (details redacted)")]
    Backend(String),
    /// The requested sandbox no longer exists.
    #[error("sandbox was not found")]
    NotFound,
    /// The runtime process exited before registering; only its bounded exit code is exposed. [`ENVIRONMENT_FAILED_EXIT`]
    /// means the environment failed before the runtime ever ran.
    #[error("runtime exited before registration: code {code}")]
    RuntimeExited {
        /// Signed process exit code reported by the backend.
        code: i32,
    },
    /// A microVM's VM host ended before its runtime registered and without the guest's exit record: the launcher, its
    /// network setup or Firecracker died, or the guest reset before its runtime ran. No task state is lost, so the
    /// gateway starts it once more (SBX-F-038).
    #[error("the VM host ended before the runtime registered")]
    HostDied,
    /// The runtime image speaks an older protocol revision than this build accepts (ADR-0026).
    #[error("sandbox runtime image protocol revision {has} is older than the required {needs}")]
    RuntimeTooOld {
        /// The lowest accepted revision.
        needs: u16,
        /// The image's revision; an unlabelled image is revision 1.
        has: u16,
    },
    /// The operation exceeded its configured bound.
    #[error("sandbox operation timed out")]
    Timeout,
    /// The caller cancelled the operation.
    #[error("sandbox operation was cancelled")]
    Cancelled,
    /// Every subnet of the per-sandbox network pool is in use; retry when a sandbox is gone.
    #[error("sandbox network pool is exhausted")]
    Capacity,
}

impl fmt::Debug for SandboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl SandboxError {
    /// Returns the stable machine-readable category, without backend content.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Preflight(_) => "sandbox.preflight",
            Self::ImageNotFound => "sandbox.image_not_found",
            Self::ImageDigestMismatch => "sandbox.image_digest_mismatch",
            Self::InvalidSpec { .. } => "sandbox.invalid_spec",
            Self::Backend(_) => "sandbox.backend",
            Self::NotFound => "sandbox.not_found",
            Self::RuntimeExited { .. } => "sandbox.runtime_exited",
            Self::HostDied => "sandbox.host_died",
            Self::RuntimeTooOld { .. } => "sandbox.runtime_too_old",
            Self::Timeout => "sandbox.timeout",
            Self::Cancelled => "sandbox.cancelled",
            Self::Capacity => "sandbox.network_pool_exhausted",
        }
    }

    /// Whether an unchanged request may succeed after a transient backend failure.
    #[must_use]
    pub const fn retryable(&self) -> bool {
        matches!(self, Self::Backend(_) | Self::Timeout | Self::Capacity)
    }

    pub(crate) const fn invalid(field: &'static str, reason: &'static str) -> Self {
        Self::InvalidSpec {
            field,
            reason: Cow::Borrowed(reason),
        }
    }
}
