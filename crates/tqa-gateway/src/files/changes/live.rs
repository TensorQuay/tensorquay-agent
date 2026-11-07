//! One live scan per workspace at a time (review of 068c): the latest run's changes compare with the live workspace,
//! and readers may ask in parallel. A request waits for the scan in progress and reuses its result only when that scan
//! finished after the request began waiting, so nobody gets a result older than their own question.
use super::manifest::Manifest;
use crate::GatewayError;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, LazyLock, Mutex},
    time::Instant,
};
use uuid::Uuid;

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;

type Slot = Arc<tokio::sync::Mutex<Option<(Instant, Arc<Manifest>)>>>;

pub(crate) struct LiveScans {
    slots: Mutex<HashMap<Uuid, Slot>>,
}

/// The gateway's live scans; keyed by workspace, so one process serves every gateway instance in it.
pub(crate) static LIVE: LazyLock<LiveScans> = LazyLock::new(LiveScans::new);

impl LiveScans {
    pub(crate) fn new() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// The workspace's live manifest: the result of a scan that finished after this call began, or a new scan.
    ///
    /// # Errors
    /// The scan's failure; a later request scans again.
    pub(crate) async fn get<F, Fut>(
        &self,
        workspace: Uuid,
        scan: F,
    ) -> Result<Arc<Manifest>, GatewayError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Manifest, GatewayError>>,
    {
        let asked = Instant::now();
        let slot = self.slot(workspace)?;
        // Held across the scan: a second request for this workspace waits here.
        let mut cached = slot.lock().await;
        if let Some((finished, manifest)) = cached.as_ref()
            && *finished >= asked
        {
            return Ok(Arc::clone(manifest));
        }
        let manifest = Arc::new(scan().await?);
        *cached = Some((Instant::now(), Arc::clone(&manifest)));
        Ok(manifest)
    }

    /// The workspace's slot; slots nobody is using are dropped so the map stays small.
    fn slot(&self, workspace: Uuid) -> Result<Slot, GatewayError> {
        let mut slots = self.slots.lock().map_err(|_| GatewayError::Io)?;
        slots.retain(|_, slot| Arc::strong_count(slot) > 1);
        Ok(Arc::clone(slots.entry(workspace).or_default()))
    }
}
