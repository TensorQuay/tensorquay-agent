//! The rules TK-I-007 and GW-I-181 judge a delayed-ACK stall by. Without `TCP_NODELAY`, a small write waits out the peer's
//! delayed ACK (40 ms on Linux), so a stalled round takes over [`STALL`]; a loopback round takes about 1 ms. The full
//! stall holds every round after the first, and a partial one (a path without `TCP_NODELAY` that only some rounds take)
//! holds some; a scheduling hiccup on a loaded host holds one or two. So up to [`LATE_ROUNDS_ALLOWED`] late rounds pass
//! and three or more fail.
use std::time::Duration;

/// Far above a loopback round, far below the 40 ms stall.
pub const STALL: Duration = Duration::from_millis(20);
/// Late rounds allowed for scheduling hiccups.
pub const LATE_ROUNDS_ALLOWED: usize = 2;

/// Passes when at most [`LATE_ROUNDS_ALLOWED`] of `times` are over [`STALL`]; otherwise says how many of the `what`
/// (a plural, such as "replies") were late and lists each late round (counted from 1) with its time.
///
/// # Errors
/// Three or more late rounds, in plain words.
pub fn check(what: &str, times: &[Duration]) -> Result<(), String> {
    let late: Vec<String> = times
        .iter()
        .enumerate()
        .filter(|(_, time)| **time > STALL)
        .map(|(index, time)| format!("round {} {time:?}", index + 1))
        .collect();
    if late.len() <= LATE_ROUNDS_ALLOWED {
        return Ok(());
    }
    Err(format!(
        "{} of {} {what} took over {STALL:?} (at most {LATE_ROUNDS_ALLOWED} may, for scheduling hiccups): {}",
        late.len(),
        times.len(),
        late.join(", ")
    ))
}

/// Passes when at most [`LATE_ROUNDS_ALLOWED`] of the `(measured, baseline)` rounds took over [`STALL`] longer than
/// their own baseline; otherwise says how many of the `what` were late and lists each late round with both times.
///
/// For a path whose round also does work a loaded host slows (a database commit, a request's handling), a fixed
/// bound fails on load alone (GW-I-181 at a one-minute load of 30: 16 and 19 of 20 rounds of 23 to 225 ms, no stall).
/// Each round's baseline is the same span on a path that can't stall, measured in that round under the same load, so
/// load moves both and only a stall's added wait counts.
///
/// # Errors
/// Three or more late rounds, in plain words.
pub fn check_paired(what: &str, rounds: &[(Duration, Duration)]) -> Result<(), String> {
    let late: Vec<String> = rounds
        .iter()
        .enumerate()
        .filter(|(_, (measured, baseline))| measured.saturating_sub(*baseline) > STALL)
        .map(|(index, (measured, baseline))| {
            format!("round {} {measured:?} (baseline {baseline:?})", index + 1)
        })
        .collect();
    if late.len() <= LATE_ROUNDS_ALLOWED {
        return Ok(());
    }
    Err(format!(
        "{} of {} {what} took over {STALL:?} longer than their baseline (at most {LATE_ROUNDS_ALLOWED} may, for \
         scheduling hiccups): {}",
        late.len(),
        rounds.len(),
        late.join(", ")
    ))
}

#[cfg(test)]
#[path = "stall_tests.rs"]
mod tests;
