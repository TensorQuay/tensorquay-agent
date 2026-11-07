//! The live workspace quota on Docker (N4): when the next measurement of a running sandbox's workspace is due, and the
//! hard limit past which its run is stopped. Pure; the worker measures and acts.
use std::time::{Duration, Instant};

/// The smallest margin past the quota before a run is stopped.
pub(super) const MIN_MARGIN: u64 = 256 << 20;
/// The largest margin past the quota before a run is stopped.
pub(super) const MAX_MARGIN: u64 = 4 << 30;
/// How often a workspace close to its hard limit is measured.
const NEAR_INTERVAL: Duration = Duration::from_secs(5);
/// Never measured sooner than this after the last measurement, whatever the growth rate.
const MIN_INTERVAL: Duration = Duration::from_secs(2);
/// A walk may use at most one tenth of the time between walks.
const COST_FACTOR: u32 = 10;

/// The usage past which a run is stopped: the quota plus `margin_percent` of it, the margin kept within
/// [`MIN_MARGIN`]..=[`MAX_MARGIN`].
pub(super) fn hard_limit(quota: u64, margin_percent: u8) -> u64 {
    let margin =
        u64::try_from(u128::from(quota) * u128::from(margin_percent) / 100).unwrap_or(u64::MAX);
    quota.saturating_add(margin.clamp(MIN_MARGIN, MAX_MARGIN))
}

/// One measurement: when it finished and what the workspace was charged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Sample {
    pub at: Instant,
    pub bytes: u64,
}

/// How long after `last` the next measurement is due: `interval`, sooner near the hard limit, sooner still when the
/// growth since `previous` would reach the limit before then (at half that time), never sooner than [`MIN_INTERVAL`]
/// and never sooner than ten times the last walk's `cost`.
pub(super) fn next_due(
    interval: Duration,
    hard: u64,
    last: Sample,
    previous: Option<Sample>,
    cost: Duration,
) -> Duration {
    let mut wait = interval;
    // Near: at or past four fifths of the hard limit.
    if last.bytes >= hard / 5 * 4 {
        wait = wait.min(NEAR_INTERVAL);
    }
    if let Some(previous) = previous
        && last.bytes > previous.bytes
    {
        // The time to the limit at the growth since `previous`, in whole milliseconds (exact: 128-bit arithmetic).
        let elapsed = last.at.saturating_duration_since(previous.at).as_millis();
        let growth = u128::from(last.bytes - previous.bytes);
        let reach = u128::from(hard.saturating_sub(last.bytes)) * elapsed / growth;
        let half = Duration::from_millis(u64::try_from(reach / 2).unwrap_or(u64::MAX));
        wait = wait.min(half);
    }
    wait.max(MIN_INTERVAL).max(cost.saturating_mul(COST_FACTOR))
}

#[cfg(test)]
#[path = "live_usage_tests.rs"]
mod tests;
