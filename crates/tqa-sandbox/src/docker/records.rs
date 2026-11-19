use crate::{BackendKind, SandboxError, SandboxHandle, SandboxStats};
use bollard::models::{ContainerStatsResponse, ContainerSummary};
use std::time::{Duration, SystemTime};

pub(super) fn handle(container: ContainerSummary) -> Result<SandboxHandle, SandboxError> {
    let invalid = || SandboxError::Backend("invalid sandbox labels in container listing".into());
    let labels = container.labels.ok_or_else(invalid)?;
    let conversation_id = labels
        .get("tqa.conversation")
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let timestamp = labels
        .get("tqa.created_at")
        .ok_or_else(invalid)?
        .parse::<u64>()
        .map_err(|_| invalid())?;
    // The engine's own creation time decides the age (SBX-U-021): the label, which anyone who can label a container
    // sets, is still required and checked, and stands in only where the engine gives no time.
    let seconds = container
        .created
        .and_then(|created| u64::try_from(created).ok())
        .unwrap_or(timestamp);
    let created_at = SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(seconds))
        .ok_or_else(invalid)?;
    let id = container
        .id
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or_else(invalid)?;
    Ok(SandboxHandle {
        kind: BackendKind::Docker,
        id,
        conversation_id,
        created_at,
    })
}

pub(super) fn stats(
    sample: ContainerStatsResponse,
    uptime: Duration,
) -> Result<SandboxStats, SandboxError> {
    let missing = || SandboxError::Backend("required Linux resource accounting is missing".into());
    let cpu = sample
        .cpu_stats
        .and_then(|stats| stats.cpu_usage)
        .and_then(|usage| usage.total_usage)
        .ok_or_else(missing)?;
    let memory_bytes = sample
        .memory_stats
        .and_then(|stats| stats.usage)
        .ok_or_else(missing)?;
    let pids = sample
        .pids_stats
        .and_then(|stats| stats.current)
        .ok_or_else(missing)?;
    Ok(SandboxStats {
        cpu_millis: cpu / 1_000_000,
        memory_bytes,
        pids,
        uptime,
    })
}
