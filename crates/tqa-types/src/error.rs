//! Stable wire error codes and structured frame parsing errors.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The closed protocol-v1 error vocabulary, with documented retry defaults.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Unknown frame kinds require a compatible sender; retrying unchanged input cannot help.
    UnknownFrame,
    /// Invalid input must be corrected before another attempt.
    InvalidFrame,
    /// A sequence gap can recover by reconnecting and replaying events.
    SeqGap,
    /// An unsupported version requires a compatible client or server.
    UnsupportedProtocol,
    /// Occupied capacity may become available on a later attempt.
    Busy,
    /// Retired by ADR-0035 (an approval binding that no longer held): read from histories written before it, never
    /// sent.
    PermissionBinding,
    /// Egress policy denied this destination; repeating the same request cannot help.
    EgressDenied,
    /// The service can accept work after its restart completes.
    Restarting,
    /// A temporary store outage may recover.
    StoreUnavailable,
    /// The provider may recover from a transient failure.
    ProviderError,
    /// A consumed run budget requires an explicit policy change.
    Budget,
    /// Repeated failures require intervention instead of another automatic retry.
    RepeatedFailure,
    /// A stalled run needs a changed approach before continuing.
    NoProgress,
    /// On a route without native tool calls, the model's reply could not be read as a tool call
    /// even after one retry prompt; another model or route is needed.
    FallbackParse,
    /// Disk or read failures require operator attention.
    Io,
    /// An internal invariant failure requires investigation.
    Internal,
    /// A sandbox could not start or register; retry with a new action key after recovery.
    SandboxFailed,
    /// The runtime exited before registration; retry a new action after startup is repaired.
    RuntimeStartFailed,
    /// An accepted action failed to execute; retry with a new action key after recovery.
    ActionFailed,
    /// The workspace is over its quota; the run stops past the quota plus a margin, and new runs start only once it
    /// holds less again.
    WorkspaceQuota,
    /// The conversation's runtime state is over its bound; its run was stopped and an administrator must intervene.
    StateQuota,
    /// A decision engine stopped the run as failed, with its reason (ADR-0013 point 2); it reaches the wire with the
    /// protocol revision of ADR-0013 point 7.
    DecisionStop,
    /// The task's decision engine is gone or disabled (ADR-0013 point 6): its sandbox was retired without a
    /// replacement, and a message continues the task once an administrator restores the engine. Gateway to clients
    /// only (revision 9).
    DecisionEngineUnavailable,
    /// The task's decision engine has used its budget for the period (ADR-0013, the adapter amendment): the agent decides
    /// on its own until the period ends or an administrator raises the budget. Gateway to clients only (revision 9).
    DecisionEngineBudgetSpent,
    /// A run parked for an approval when ADR-0035 removed approvals; written only by the gateway's upgrade, never sent
    /// by a runtime. A message starts a new run (revision 9).
    ApprovalsRemoved,
    /// The model ended its reply at its output limit (`finish_reason = length`) while the run's own output budget was
    /// left: the model server's context or max tokens, or the route's max output, cut it (revision 10). Retrying with
    /// the same limits cuts it again, so it is not retryable.
    ReplyCut,
    /// The task's sender has keys for web addresses the task could use, but its environment's runtime is older than
    /// revision 11 and can't take them: the run goes ahead without them until the environment is rebuilt (ADR-0037).
    /// An unsequenced notice, gateway to clients only (revision 11).
    HostKeysUnsupported,
}

impl ErrorCode {
    /// Returns the stable `snake_case` spelling used on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownFrame => "unknown_frame",
            Self::InvalidFrame => "invalid_frame",
            Self::SeqGap => "seq_gap",
            Self::UnsupportedProtocol => "unsupported_protocol",
            Self::Busy => "busy",
            Self::PermissionBinding => "permission_binding",
            Self::EgressDenied => "egress_denied",
            Self::Restarting => "restarting",
            Self::StoreUnavailable => "store_unavailable",
            Self::ProviderError => "provider_error",
            Self::Budget => "budget",
            Self::RepeatedFailure => "repeated_failure",
            Self::NoProgress => "no_progress",
            Self::FallbackParse => "fallback_parse",
            Self::Io => "io",
            Self::Internal => "internal",
            Self::SandboxFailed => "sandbox_failed",
            Self::RuntimeStartFailed => "runtime_start_failed",
            Self::ActionFailed => "action_failed",
            Self::WorkspaceQuota => "workspace_quota",
            Self::StateQuota => "state_quota",
            Self::DecisionStop => "decision_stop",
            Self::DecisionEngineUnavailable => "decision_engine_unavailable",
            Self::DecisionEngineBudgetSpent => "decision_engine_budget_spent",
            Self::ApprovalsRemoved => "approvals_removed",
            Self::ReplyCut => "reply_cut",
            Self::HostKeysUnsupported => "host_keys_unsupported",
        }
    }

    /// Returns the conservative protocol default; individual error frames carry their own flag.
    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::SeqGap
                | Self::Busy
                | Self::Restarting
                | Self::StoreUnavailable
                | Self::ProviderError
                | Self::SandboxFailed
                | Self::RuntimeStartFailed
                | Self::ActionFailed
                | Self::DecisionEngineUnavailable
        )
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A rejected frame with a stable code, a JSON field path and a human-readable detail.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{code} at {path}: {detail}")]
pub struct FrameError {
    /// Invalid-frame or unknown-frame classification.
    pub code: ErrorCode,
    /// A field path when serde identifies one, or the root `$` otherwise.
    pub path: String,
    /// The parsing or size-limit diagnostic, without transport policy.
    pub detail: String,
}
