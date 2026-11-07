//! The agent loop and its native-tool and text-fallback executors, budgets, recovery
//! and event emission, independent of host services.

mod decision;
mod decision_checks;
mod error;
mod gateway_link;
mod limits;
mod runner;
mod screen_store;
mod screenshots;
mod sink;

pub use decision::{
    BudgetUse, CallSummary, Decision, DecisionEngine, EngineFailure, ModelDecides, ModelResponse,
    PlannedAction, RunState, TranscriptLine,
};
pub use decision_checks::{check_act, check_call_model, check_selection, valid_stop_reason};
pub use error::{CoreError, RunOutcome};
pub use gateway_link::{GatewayLink, LinkSettle};
pub use limits::{
    DEFAULT_CONTEXT_WINDOW, DEFAULT_OUTPUT_RESERVE, LOCAL_PROJECTION_TIMESTAMP, MODEL_ATTEMPTS,
    RETRY_BASE, RETRY_CAP, SUMMARY_RETRY_ATTEMPTS,
};
pub use runner::{
    PreRunFailure, PromptInputs, RouteProvider, Runner, RunnerDeps, UserTurn, fail_before_run,
};
pub use screen_store::{AFTER_WAIT, BEFORE_WAIT, ScreenStore, Unstored, screenshot_id};
pub use screenshots::{SENT_SCREENSHOTS, Screenshots};
pub use sink::{EventContext, EventSink, SinkError};

/// Returns this crate's package name.
#[must_use]
pub fn crate_name() -> &'static str {
    "tqa-core"
}

#[cfg(test)]
mod tests {
    #[test]
    fn returns_crate_name() {
        assert_eq!(super::crate_name(), "tqa-core");
    }
}
