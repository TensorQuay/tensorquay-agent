//! PERF-002's accounting: pure, so its rules are tested without a gateway.
use std::time::Duration;

/// A burst may take this much longer than its schedule to send; past it, the link held the sender back and the rate
/// wasn't really offered.
pub const BURST_SLACK: f64 = 1.1;

/// A fixed sending schedule: `per_ms` events due each millisecond, never "as fast as possible".
#[derive(Clone, Copy, Debug)]
pub struct Pacing {
    per_ms: u64,
}

impl Pacing {
    /// `rate` events a second, a whole number of events each millisecond.
    pub const fn per_second(rate: u64) -> Self {
        Self {
            per_ms: rate / 1000,
        }
    }

    /// When event `index` (from 0) is due, from the burst's start.
    pub fn due(self, index: u64) -> Duration {
        Duration::from_millis(index / self.per_ms.max(1))
    }

    /// How long a burst of `events` lasts on the schedule.
    pub fn span(self, events: u64) -> Duration {
        Duration::from_millis(events.div_ceil(self.per_ms.max(1)))
    }
}

/// The client's receipts: every sequence exactly once, in order.
#[derive(Debug, Default)]
pub struct Receipts {
    last: u64,
}

impl Receipts {
    /// Takes the next received sequence.
    pub fn record(&mut self, sequence: u64) -> Result<(), String> {
        if sequence <= self.last {
            return Err(format!(
                "sequence {sequence} arrived again after {}",
                self.last
            ));
        }
        if sequence != self.last + 1 {
            return Err(format!(
                "sequence {sequence} arrived after {}: {} is missing",
                self.last,
                self.last + 1
            ));
        }
        self.last = sequence;
        Ok(())
    }

    /// The last sequence received.
    pub const fn last(&self) -> u64 {
        self.last
    }

    /// Whether all `sent` events arrived.
    pub fn complete(&self, sent: u64) -> Result<(), String> {
        if self.last == sent {
            Ok(())
        } else {
            Err(format!("only {} of {sent} events arrived", self.last))
        }
    }
}

/// Fails a burst of `events` that took longer than `scheduled` × [`BURST_SLACK`] to send, with the rate it reached.
pub fn burst_in_time(elapsed: Duration, events: u64, scheduled: Duration) -> Result<(), String> {
    if elapsed.as_secs_f64() <= scheduled.as_secs_f64() * BURST_SLACK {
        return Ok(());
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a rate for a message"
    )]
    let rate = (events as f64 / elapsed.as_secs_f64()).round() as u64;
    Err(format!(
        "the burst of {events} events took {:.3} s to send ({rate} events/s): the link held the sender back",
        elapsed.as_secs_f64()
    ))
}

/// The TEST-PLAN row's rate, 10,000 events a second.
pub const ROW_RATE: u64 = 10_000;

/// The offered rate: the row's, or a recorded variant's (`TQA_PERF_002_RATE`, such as 1,000 for the lag at low
/// rates), a whole number of thousands from 1,000 to 100,000 so each millisecond gets the same number of events.
pub fn rate(variant: Option<&str>) -> Result<u64, String> {
    let Some(text) = variant else {
        return Ok(ROW_RATE);
    };
    match text.parse::<u64>() {
        Ok(rate) if (1_000..=100_000).contains(&rate) && rate % 1_000 == 0 => Ok(rate),
        _ => Err(format!(
            "TQA_PERF_002_RATE={text}: a whole number of thousands of events a second, 1000 to 100000"
        )),
    }
}

/// The p50 and p99 of `lags_ns`, in milliseconds (nearest rank).
pub fn lag_summary(lags_ns: &[u64]) -> Result<(f64, f64), String> {
    let ms = |ns: u64| Duration::from_nanos(ns).as_secs_f64() * 1000.0;
    Ok((
        ms(tqa_testkit::perf::percentile(lags_ns, 50)?),
        ms(tqa_testkit::perf::percentile(lags_ns, 99)?),
    ))
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
