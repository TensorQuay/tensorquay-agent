//! How many tasks are mid-step right now, for Settings › Status: a restart now would start each of those steps over
//! (a first-hour check). A count only, never which tasks or whose. A task counts while its runtime is linked and sent a
//! run's heartbeat lately (a supervisor sends them only while a run is active, every 30 s) and its journal's latest
//! lifecycle event says its run is going: a run that ended since its last heartbeat doesn't count, nor does one whose
//! runtime is gone.
use crate::{GatewayError, channels::store::RunStatus, link::LinkRegistry};
use std::time::Duration;

/// How recent a run's heartbeat must be: two and a half of the runtime's 30-s intervals, so one late beat isn't a
/// stopped run.
pub(crate) const HEARTBEAT_WITHIN: Duration = Duration::from_secs(75);

/// The tasks with a run going now.
///
/// # Errors
/// The registry's or the database's.
pub(crate) async fn count(
    pool: &sqlx::PgPool,
    links: &LinkRegistry,
    now: tokio::time::Instant,
) -> Result<usize, GatewayError> {
    let ids = links
        .heartbeating(now, HEARTBEAT_WITHIN)?
        .iter()
        .filter_map(|conversation| super::uuid(conversation).ok())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(0);
    }
    let statuses = super::endpoints::latest_statuses(pool, &ids).await?;
    Ok(statuses
        .values()
        .filter(|status| **status == RunStatus::Running)
        .count())
}

#[cfg(test)]
#[path = "active_tests.rs"]
mod tests;
