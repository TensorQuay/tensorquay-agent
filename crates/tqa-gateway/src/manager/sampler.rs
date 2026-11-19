//! CPU and memory per running sandbox (the Sandboxes page design note, §3.4; FR-13): the backend's `stats` for every
//! running sandbox every [`SAMPLE_EVERY`], at most [`IN_FLIGHT`] calls at once, each bounded by the backend's own
//! timeout. The last two samples are kept in memory and the CPU rate comes from their difference, so a sandbox's
//! first value appears after its second sample. Never written to the store, and never a failure of the listing.
use super::SandboxManager;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use time::OffsetDateTime;
use tqa_sandbox::SandboxStats;
use tqa_types::ConversationId;

/// How often every running sandbox is sampled, with fewer than [`STRETCH_FROM`] running.
pub(crate) const SAMPLE_EVERY: Duration = Duration::from_secs(10);
/// Past this many running sandboxes the interval stretches with the count (the same bound the workspace quota walk has).
const STRETCH_FROM: u32 = 64;
/// Engine calls in flight at once.
const IN_FLIGHT: usize = 4;

#[derive(Clone, Debug)]
struct Sample {
    at: Instant,
    stats: SandboxStats,
}

/// A sandbox's measured use, derived from its last two samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct Usage {
    pub cpu_millicores: u64,
    pub memory_bytes: u64,
    pub pids: u64,
}

#[derive(Default)]
pub(crate) struct Samples {
    by: Mutex<HashMap<ConversationId, (Option<Sample>, Sample)>>,
    measured_at: Mutex<Option<OffsetDateTime>>,
}

/// The interval for `running` sandboxes: [`SAMPLE_EVERY`] up to [`STRETCH_FROM`], then longer in proportion.
pub(crate) fn interval(running: u32) -> Duration {
    if running <= STRETCH_FROM {
        SAMPLE_EVERY
    } else {
        SAMPLE_EVERY.mul_f64(f64::from(running) / f64::from(STRETCH_FROM))
    }
}

/// The CPU rate between two samples, in millicores, and the later sample's memory and processes.
fn usage(previous: &Sample, last: &Sample) -> Usage {
    let elapsed_ms = u64::try_from(last.at.saturating_duration_since(previous.at).as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let cpu_ms = last
        .stats
        .cpu_millis
        .saturating_sub(previous.stats.cpu_millis);
    Usage {
        cpu_millicores: u64::try_from(u128::from(cpu_ms) * 1000 / u128::from(elapsed_ms))
            .unwrap_or(u64::MAX),
        memory_bytes: last.stats.memory_bytes,
        pids: last.stats.pids,
    }
}

impl SandboxManager {
    /// The measured use of `id`'s sandbox, once it has two samples.
    pub(crate) fn usage_of(&self, id: &ConversationId) -> Option<Usage> {
        let by = self.samples.by.lock().ok()?;
        let (previous, last) = by.get(id)?;
        previous.as_ref().map(|previous| usage(previous, last))
    }

    /// When the running sandboxes were last sampled.
    pub(crate) fn usage_measured_at(&self) -> Option<OffsetDateTime> {
        self.samples.measured_at.lock().ok().and_then(|at| *at)
    }

    /// Starts the owned sampler; it stops with the gateway.
    pub(crate) fn start_usage_sampler(self: &Arc<Self>) {
        let manager = self.clone();
        self.services.tasks.spawn(async move {
            loop {
                let running = manager.sample_all().await;
                tokio::select! {biased;
                    () = manager.services.cancel.cancelled() => return,
                    () = tokio::time::sleep(interval(running)) => {}
                }
            }
        });
    }

    /// Samples every sandbox with a handle; how many there were.
    async fn sample_all(&self) -> u32 {
        let handles: Vec<_> = match self.slots.lock() {
            Ok(slots) => slots
                .iter()
                .filter_map(|(id, slot)| {
                    slot.handle
                        .lock()
                        .ok()
                        .and_then(|handle| handle.clone())
                        .map(|handle| (id.clone(), handle))
                })
                .collect(),
            Err(_) => return 0,
        };
        let running = u32::try_from(handles.len()).unwrap_or(u32::MAX);
        let limit = Arc::new(tokio::sync::Semaphore::new(IN_FLIGHT));
        let mut sampled = Vec::with_capacity(handles.len());
        let mut tasks = tokio::task::JoinSet::new();
        for (id, handle) in handles {
            let engine = self.backend.engine.clone();
            let limit = limit.clone();
            let timeout = self.config.io_timeout();
            tasks.spawn(async move {
                let _permit = limit.acquire().await.ok()?;
                let stats = tokio::time::timeout(timeout, engine.stats(&handle))
                    .await
                    .ok()?
                    .ok()?;
                Some((
                    id,
                    Sample {
                        at: Instant::now(),
                        stats,
                    },
                ))
            });
        }
        while let Some(result) = tasks.join_next().await {
            if let Ok(Some(sample)) = result {
                sampled.push(sample);
            }
        }
        if let Ok(mut by) = self.samples.by.lock() {
            let mut seen = std::collections::HashSet::new();
            for (id, sample) in sampled {
                seen.insert(id.clone());
                match by.remove(&id) {
                    Some((_, last)) => by.insert(id, (Some(last), sample)),
                    None => by.insert(id, (None, sample)),
                };
            }
            // A sandbox that is gone, or whose stats failed this time, starts over at its next sample.
            by.retain(|id, _| seen.contains(id));
        }
        if let Ok(mut at) = self.samples.measured_at.lock() {
            *at = Some(OffsetDateTime::now_utc());
        }
        running
    }
}
