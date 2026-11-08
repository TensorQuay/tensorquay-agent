//! The route check's time (GW-U-092): how long a check may take on a hosted or a local route, and what a local
//! model's check concludes when it runs out of time.
use super::RelayConfig;
use super::probe::{Check, Failure, Observation, Probe, fail, local_route, steps};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tqa_llm::Route;

/// The whole probe's bound.
const DEADLINE: Duration = Duration::from_mins(1);
/// A local route's whole check: bounded by progress, not by its total (a first-hour check before 17.0). Each answer's first
/// byte waits up to `[relay] local_check_timeout`, then a gap of `[relay] idle_timeout` without a token ends it; a
/// model that keeps answering may finish within this outer cap, however slowly it thinks.
const LOCAL_OUTER_CAP: Duration = Duration::from_mins(10);

/// The longest any route's check may take: a local route's outer cap, or its first-byte wait if that is longer.
pub(super) fn longest_deadline(config: &RelayConfig) -> Duration {
    DEADLINE.max(local_deadline(config))
}

/// A local route's whole check: [`LOCAL_OUTER_CAP`], or `[relay] local_check_timeout` when that is longer.
fn local_deadline(config: &RelayConfig) -> Duration {
    LOCAL_OUTER_CAP.max(Duration::from_secs(config.local_check_timeout))
}

/// How long the whole check may take on this route: a local model may keep answering up to [`local_deadline`], as long
/// as it makes progress (GW-U-092); a hosted one gets the minute.
pub(super) fn deadline(route: &Route, config: &RelayConfig) -> Duration {
    if local_route(route) {
        local_deadline(config)
    } else {
        DEADLINE
    }
}

/// How long one request of the check waits for its answer's first byte: the relay's own wait, or on a local route as
/// long as the whole check (a local model may think for a while before its first byte).
pub(super) fn first_byte(route: &Route, config: &RelayConfig) -> Duration {
    Duration::from_secs(if local_route(route) {
        config.local_check_timeout
    } else {
        config.header_timeout
    })
}

/// The checks, within `until` unless cancelled, and whether `until` ended them: a model still answering then is slow
/// (GW-U-092), while a gap past `[relay] idle_timeout` is a `timeout` from the steps themselves, which tasks meet the
/// same way.
pub(super) async fn steps_until(
    route: &Route,
    probe: &Probe<'_>,
    observed: &mut Observation,
    cancel: &CancellationToken,
    until: Duration,
) -> (Result<(), Failure>, bool) {
    tokio::select! {biased;
        () = cancel.cancelled() => (Err(fail("cancelled")), false),
        result = tokio::time::timeout(until, steps(route, probe, observed)) => match result {
            Ok(outcome) => (outcome, false),
            Err(_) => (Err(fail("timeout")), true),
        },
    }
}

/// A local model that passed the call and the round trip, and was still answering when the check's own deadline ended
/// it in the strict-lines step, works for tasks, slowly (the 7 Nov 2026 first hour: 5,078 tokens of thinking at 12 a
/// second reached the outer cap there): the check passes, with the line breaks unverified, and says the model is slow
/// (GW-U-092). A gap past `[relay] idle_timeout` stays a `timeout` in every step, since tasks meet the same gap.
/// Answers whether it passed.
pub(super) fn out_of_time_after_round_trip(
    route: &Route,
    reached_deadline: bool,
    outcome: &mut Result<(), Failure>,
    observed: &mut Observation,
) -> bool {
    let out_of_time = reached_deadline
        && local_route(route)
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
