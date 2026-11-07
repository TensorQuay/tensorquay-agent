//! Named loop defaults; configured values are validated rather than clamped.
use std::time::Duration;
/// Conservative context capacity used when the route declares none.
pub const DEFAULT_CONTEXT_WINDOW: u32 = 32_768;
/// The output the context window plans for when the route declares no Max output. Planning only: such a request
/// names no output limit, so the model server's own applies (CORE-U-071).
pub const DEFAULT_OUTPUT_RESERVE: u32 = 4096;
/// Initial operation and two retries, per step per invocation.
pub const MODEL_ATTEMPTS: u32 = 3;
/// Retries of a compaction summary call after a retryable transport failure; parse and bounds failures fall back
/// at once (ADR-0009).
pub const SUMMARY_RETRY_ATTEMPTS: u32 = 2;
/// Initial equal-jitter ceiling: waits start at one second, then at two seconds.
pub const RETRY_BASE: Duration = Duration::from_secs(2);
/// Maximum retry delay, including a provider's retry-after value.
pub const RETRY_CAP: Duration = Duration::from_secs(30);
/// While the relay throttles a step, an unchanged wait is reported again after this long, so a long wait stays
/// visible without one event per request.
pub(crate) const THROTTLE_REPORT_EVERY: Duration = Duration::from_secs(30);
/// How long the run waits for the workspace's free-space answer after a failed command. The query can hang on a
/// stalled storage mount (a hard-mounted NFS `storage.root`); an unanswered query counts as not full.
pub(crate) const SPACE_QUERY_TIMEOUT: Duration = Duration::from_secs(1);
/// Timestamp for local projection frames only; these frames never leave the core.
pub const LOCAL_PROJECTION_TIMESTAMP: &str = "2000-01-01T00:00:00Z";
