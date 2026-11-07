//! Provider-independent messages, history projection and recovery, runtime write-ahead storage,
//! token estimates, deterministic context windows and tool-result spill handling.
mod cap;
mod compaction;
mod dedupe;
mod error;
mod history;
mod message;
mod projection;
mod resume;
mod tokens;
mod wal;
mod window;

pub use cap::{
    CappedResult, MAX_RESULT_WINDOW_SHARE, MIN_RESULT_BYTES, cap_tool_result, window_result_cap,
};
pub use compaction::{
    COMPACT_MIN_KEEP_TURNS, COMPACTION_SUMMARY_PREFIX, CompactionPlan, plan_compaction,
    plan_compaction_keeping,
};
pub use dedupe::{DEDUPE_MIN_BYTES, dedupe_stub};
pub use error::ContextError;
pub use history::FALLBACK_RETRY_PREFIX;
pub use message::{CallRecord, Message, ReasoningStateRecord, Role};
pub use projection::{
    CORRECTION_PREFIX, Projection, ProjectionError, RUN_NOTES_PREFIX, is_correction,
    leading_correction, may_be_correction, project, project_turn,
};
pub use resume::{ResumePoint, recent_turns, resume_point};
pub use tokens::{TokenEstimate, estimate, estimate_bytes, message_tokens};
pub use wal::{MAX_WAL_BYTES, Recovery, Wal, WalError, wal_should_sync};
pub use window::{Budget, Window};

/// Returns this crate's package name.
#[must_use]
pub fn crate_name() -> &'static str {
    "tqa-context"
}

#[cfg(test)]
mod tests {
    #[test]
    fn returns_crate_name() {
        assert_eq!(super::crate_name(), "tqa-context");
    }
}
