//! `[workspaces]`: the write lease (task 066b, ARCHITECTURE §4.6). A second task that wants to run on a workspace
//! whose lease another task's running sandbox holds either waits (`wait`, the default) or is refused
//! (`refuse`, `409 workspace_busy`). Agent profiles may override the policy later (Phase 6). The quota and
//! retention bound what workspaces hold and how long an unused one is kept (task 066c); on Docker a running workspace
//! is also measured during its run and its run stopped past the quota plus a margin.
use crate::GatewayError;
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Deserialize, serde::Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BusyPolicy {
    #[default]
    Wait,
    Refuse,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct WorkspacesInput {
    busy_policy: BusyPolicy,
    /// How long a lease lasts without a renewal; heartbeats of the holder's run renew it.
    lease_ttl_seconds: u32,
    /// The most a workspace may hold (task 066c): a fork beyond it fails; a workspace over it starts no new run.
    quota_bytes: u64,
    /// Workspaces unused this long, with no active task, are deleted; 0 keeps them for ever (the default).
    retention_days: u32,
    /// Whether a running Docker sandbox's workspace is measured during its run; Firecracker's image is the bound.
    live_check: bool,
    /// How often a running workspace is measured, far from its limit (sooner near it or when it grows fast).
    check_seconds: u32,
    /// How far past the quota a run may write before it is stopped, in percent of the quota (256 MiB through 4 GiB).
    margin_percent: u8,
}

impl Default for WorkspacesInput {
    fn default() -> Self {
        Self {
            busy_policy: BusyPolicy::Wait,
            lease_ttl_seconds: 90,
            quota_bytes: 10 * GIB,
            retention_days: 0,
            live_check: true,
            check_seconds: 30,
            margin_percent: 10,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Workspaces {
    pub busy_policy: BusyPolicy,
    pub lease_ttl: Duration,
    pub quota_bytes: u64,
    /// `None`: retention is off.
    pub retention: Option<Duration>,
    /// `None`: no live measurement; otherwise its interval far from the limit.
    pub live_check: Option<Duration>,
    pub margin_percent: u8,
}

const GIB: u64 = 1024 * 1024 * 1024;

/// # Errors
/// Rejects a lease time to live outside 10 s..=1 h (at least a few heartbeat intervals), a quota outside
/// 1 MiB..=1 TiB, a retention above ten years, a live check interval outside 5 s..=10 min and a margin outside
/// 1..=100 %.
pub(super) fn workspaces(input: WorkspacesInput) -> Result<Workspaces, GatewayError> {
    if !(10..=3600).contains(&input.lease_ttl_seconds) {
        return Err(GatewayError::Configuration(
            "workspaces.lease_ttl_seconds must be 10 through 3600",
        ));
    }
    if !(1024 * 1024..=1024 * GIB).contains(&input.quota_bytes) {
        return Err(GatewayError::Configuration(
            "workspaces.quota_bytes must be 1 MiB through 1 TiB",
        ));
    }
    if input.retention_days > 3650 {
        return Err(GatewayError::Configuration(
            "workspaces.retention_days must be 0 (off) through 3650",
        ));
    }
    if !(5..=600).contains(&input.check_seconds) {
        return Err(GatewayError::Configuration(
            "workspaces.check_seconds must be 5 through 600",
        ));
    }
    if !(1..=100).contains(&input.margin_percent) {
        return Err(GatewayError::Configuration(
            "workspaces.margin_percent must be 1 through 100",
        ));
    }
    Ok(Workspaces {
        busy_policy: input.busy_policy,
        lease_ttl: Duration::from_secs(u64::from(input.lease_ttl_seconds)),
        quota_bytes: input.quota_bytes,
        retention: (input.retention_days > 0)
            .then(|| Duration::from_secs(u64::from(input.retention_days) * 86_400)),
        live_check: input
            .live_check
            .then(|| Duration::from_secs(u64::from(input.check_seconds))),
        margin_percent: input.margin_percent,
    })
}

#[cfg(test)]
#[path = "workspaces_tests.rs"]
mod tests;
