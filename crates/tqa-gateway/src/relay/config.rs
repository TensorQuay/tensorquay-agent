use crate::GatewayError;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RelayConfig {
    /// One user's burst; unset, [`BURST_PER_RUN`] for each run the user may have live.
    pub capacity: Option<u32>,
    /// One user's sustained requests per second; unset, [`REFILL_PER_RUN`] for each run the user may have live.
    pub refill_per_second: Option<f64>,
    pub users: usize,
    pub body_bytes: usize,
    pub sse_event_bytes: usize,
    pub response_bytes: usize,
    pub header_timeout: u64,
    pub idle_timeout: u64,
    pub response_timeout: u64,
    /// Seconds a route check of a model on this computer (or one without a key) may take, and wait for each answer's
    /// first byte: a CPU model can take tens of seconds to answer (GW-U-092).
    pub local_check_timeout: u64,
    /// Seconds a request may wait for its route's limiter (LLM-U-012).
    pub queue_wait: u64,
    /// Requests that may wait for one route at once.
    pub queue_max: usize,
    /// The runs one user may have live (`[admission] per_user`), which the unset rate settings scale with.
    #[serde(skip)]
    pub live_runs: u32,
}

/// The longest `idle_timeout` accepted: the runtime's own wait between a relayed answer's chunks is just above it
/// (`tqa_types::RELAY_IDLE_MAX_SECONDS`), so the gateway is always the bound that ends an answer and logs why.
pub(crate) const MAX_IDLE_SECONDS: u64 = tqa_types::RELAY_IDLE_MAX_SECONDS;
/// The longest silence a title request or a route check waits through: they keep the bound they had before the
/// relay's idle limit grew, so a check in Settings never sits minutes on a silent server; a lower `idle_timeout` still
/// applies.
pub(crate) const CHECK_IDLE_SECONDS: u64 = 60;

impl RelayConfig {
    /// The silence a title request or a route check waits through: `idle_timeout`, at most [`CHECK_IDLE_SECONDS`].
    #[must_use]
    pub fn check_idle(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.idle_timeout.min(CHECK_IDLE_SECONDS))
    }
}

/// The burst each live run of a user adds when `capacity` is unset.
pub(crate) const BURST_PER_RUN: u32 = 60;
/// The sustained requests per second each live run of a user adds when `refill_per_second` is unset.
pub(crate) const REFILL_PER_RUN: f64 = 1.0;
const MAX_CAPACITY: u32 = 100_000;
const MAX_REFILL: f64 = 10_000.0;
impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            capacity: None,
            refill_per_second: None,
            users: 10_000,
            body_bytes: 16 * 1024 * 1024,
            sse_event_bytes: 1024 * 1024,
            response_bytes: 64 * 1024 * 1024,
            header_timeout: 60,
            // A server may hold a whole tool call back until its arguments are complete (decided on 21 Nov 2026).
            idle_timeout: 300,
            response_timeout: 1800,
            local_check_timeout: 120,
            queue_wait: 30,
            queue_max: 64,
            live_runs: 1,
        }
    }
}
impl RelayConfig {
    /// Scales the unset per-user rate settings with the runs one user may have live, so that one person driving
    /// several agents at once is not refused as if they were one.
    #[must_use]
    pub fn for_live_runs(self, live_runs: u32) -> Self {
        Self {
            live_runs: live_runs.max(1),
            ..self
        }
    }
    /// One user's burst.
    pub fn capacity(&self) -> u32 {
        self.capacity.unwrap_or_else(|| {
            BURST_PER_RUN
                .saturating_mul(self.live_runs)
                .min(MAX_CAPACITY)
        })
    }
    /// One user's sustained requests per second.
    pub fn refill(&self) -> f64 {
        self.refill_per_second
            .unwrap_or_else(|| (REFILL_PER_RUN * f64::from(self.live_runs)).min(MAX_REFILL))
    }
    pub fn validate(&self) -> Result<(), GatewayError> {
        if !(1..=MAX_CAPACITY).contains(&self.capacity())
            || !self.refill().is_finite()
            || !(0.001..=MAX_REFILL).contains(&self.refill())
            || !(1..=100_000).contains(&self.users)
            || !(1024..=16 * 1024 * 1024).contains(&self.body_bytes)
            || !(128..=4 * 1024 * 1024).contains(&self.sse_event_bytes)
            || !(1024..=256 * 1024 * 1024).contains(&self.response_bytes)
            || !(1..=300).contains(&self.header_timeout)
            || !(1..=MAX_IDLE_SECONDS).contains(&self.idle_timeout)
            || !(1..=3600).contains(&self.response_timeout)
            || !(1..=600).contains(&self.local_check_timeout)
            || !(1..=120).contains(&self.queue_wait)
            || !(1..=10_000).contains(&self.queue_max)
        {
            return Err(GatewayError::Configuration("relay bounds are invalid"));
        }
        Ok(())
    }
}
