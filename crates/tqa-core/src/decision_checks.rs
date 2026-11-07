//! The loop's checks of an engine's answer (ADR-0013), public so that `ext-check decision`, the conformance suite an
//! engine's author runs, applies these very rules rather than a copy that could drift (CORE-U-063). Each refusal is
//! the fallback code the loop records.
use crate::{Decision, PlannedAction, RunState};
use std::collections::BTreeSet;
use tqa_llm::ToolCall;
use tqa_types::DecisionFallbackReason;

/// A `call_model` checked against the step's limits: its tools and route, or the fallback code. Any other decision is
/// `invalid` here.
///
/// # Errors
/// `route_not_allowed` for a route outside the step's; `tool_not_allowed` for a tool outside its tools, or one named
/// twice; `invalid` for another decision.
pub fn check_call_model(
    state: &RunState,
    decision: Decision,
) -> Result<(BTreeSet<String>, String), DecisionFallbackReason> {
    let Decision::CallModel { route, tool_subset } = decision else {
        return Err(DecisionFallbackReason::Invalid);
    };
    if !state.routes.contains(&route) {
        return Err(DecisionFallbackReason::RouteNotAllowed);
    }
    let names: BTreeSet<_> = tool_subset.iter().cloned().collect();
    if names.len() != tool_subset.len() || !names.iter().all(|name| state.tools.contains(name)) {
        return Err(DecisionFallbackReason::ToolNotAllowed);
    }
    Ok((names, route))
}

/// An `act`'s call checked against the step's tools and the tool's schema, which `accepts` applies: its arguments as a
/// JSON object. The loop's own cap on `act`s in a row is the run's, not the answer's, so it isn't checked here.
///
/// # Errors
/// `tool_not_allowed` for a tool outside the step's; `schema` for arguments that aren't a JSON object the schema
/// accepts.
pub fn check_act(
    state: &RunState,
    call: &ToolCall,
    accepts: impl Fn(&str, &serde_json::Value) -> bool,
) -> Result<serde_json::Value, DecisionFallbackReason> {
    if !state.tools.contains(&call.name) {
        return Err(DecisionFallbackReason::ToolNotAllowed);
    }
    serde_json::from_str::<serde_json::Value>(&call.arguments)
        .ok()
        .filter(|arguments| arguments.is_object() && accepts(&call.name, arguments))
        .ok_or(DecisionFallbackReason::Schema)
}

/// An `after_response` selection checked against the response's calls: the ids selected.
///
/// # Errors
/// `invalid` for an id that isn't one of the response's calls, or one named twice.
pub fn check_selection(
    original: &BTreeSet<String>,
    planned: &[PlannedAction],
) -> Result<BTreeSet<String>, DecisionFallbackReason> {
    let selected: BTreeSet<_> = planned
        .iter()
        .map(|action| action.call_id.clone())
        .collect();
    if selected.len() == planned.len() && selected.is_subset(original) {
        Ok(selected)
    } else {
        Err(DecisionFallbackReason::Invalid)
    }
}

/// A stop reason the journal can carry: 1 through 200 characters, no control characters.
#[must_use]
pub fn valid_stop_reason(reason: &str) -> bool {
    let chars = reason.chars().count();
    (1..=tqa_types::MAX_DECISION_STOP_REASON_CHARS).contains(&chars)
        && !reason.chars().any(char::is_control)
}
