//! The route check's rule for a slow local model (GW-U-092): what its check concludes when it runs out of time.
use super::probe::{Check, Failure, Observation, local_route};
use tqa_llm::Route;

/// A local model that passed the call and the round trip, then ran out of time in the strict-lines step, works for
/// tasks, slowly (the 7 Nov 2026 first hour: 5,078 tokens of thinking at 12 a second reached the outer cap there): the
/// check passes, with the line breaks unverified, and says the model is slow (GW-U-092). Answers whether it did.
pub(super) fn out_of_time_after_round_trip(
    route: &Route,
    outcome: &mut Result<(), Failure>,
    observed: &mut Observation,
) -> bool {
    let out_of_time = local_route(route)
        && observed.round_trip == Check::Passed
        && matches!(outcome, Err(failure) if failure.code == "timeout");
    if out_of_time {
        observed.strict_line_breaks = Check::NotRun;
        if !observed.warnings.contains(&"line_breaks_unverified") {
            observed.warnings.push("line_breaks_unverified");
        }
        *outcome = Ok(());
    }
    out_of_time
}
