//! Wakes readers waiting on a channel when a message is appended (ADR-0014 point 7, and point 11 for the stream). One
//! watch per channel with readers, carrying the last seq; a reader subscribes before it reads, so a post committed in
//! between is never missed. A channel's watch is dropped once nobody holds it.
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct Hub {
    channels: Mutex<HashMap<Uuid, watch::Sender<u64>>>,
}

impl Hub {
    /// A receiver that changes when the channel gets a message after this call.
    pub(crate) fn subscribe(&self, channel: Uuid) -> watch::Receiver<u64> {
        let mut channels = self.channels.lock().unwrap_or_else(PoisonError::into_inner);
        channels.retain(|_, sender| sender.receiver_count() > 0);
        channels
            .entry(channel)
            .or_insert_with(|| watch::channel(0).0)
            .subscribe()
    }

    /// Tells the channel's readers that `seq` was committed.
    pub(crate) fn published(&self, channel: Uuid, seq: u64) {
        let channels = self.channels.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(sender) = channels.get(&channel) {
            sender.send_replace(seq);
        }
    }
}
