//! Wakes readers waiting on a channel when a message is appended (ADR-0014 point 7, and point 11 for the stream). One
//! watch per channel with readers, carrying the last seq; a reader subscribes before it reads, so a post committed in
//! between is never missed. A channel's watch is dropped once nobody holds it.
use std::{
    collections::HashMap,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct Hub {
    channels: Mutex<HashMap<Uuid, watch::Sender<u64>>>,
    /// Who is in `channel_wait` on which channel, counted (ADR-0014's amendment of 20 Nov 2026, A7: `waiting`).
    waiting: Mutex<HashMap<(Uuid, Uuid), usize>>,
    /// Wakes held back (tests only: as if the gateway had stopped before sending them).
    held: AtomicBool,
    /// One wake pass at a time, so two never send one member two turns (A3).
    pass: tokio::sync::Mutex<()>,
}

/// A member's wait on a channel, for as long as it is held.
pub(crate) struct Waiting<'a> {
    hub: &'a Hub,
    key: (Uuid, Uuid),
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        let mut waiting = self
            .hub
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = waiting.get_mut(&self.key) {
            *count -= 1;
            if *count == 0 {
                waiting.remove(&self.key);
            }
        }
    }
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

    /// Marks `conversation` as waiting on `channel` until the answer is dropped.
    pub(crate) fn wait(&self, channel: Uuid, conversation: Uuid) -> Waiting<'_> {
        let key = (channel, conversation);
        *self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key)
            .or_insert(0) += 1;
        Waiting { hub: self, key }
    }

    /// Whether `conversation` is in `channel_wait` on `channel` now.
    pub(crate) fn is_waiting(&self, channel: Uuid, conversation: Uuid) -> bool {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&(channel, conversation))
    }

    /// Holds wakes back (`true`) or lets them go: tests only.
    pub(crate) fn hold(&self, held: bool) {
        self.held.store(held, Ordering::SeqCst);
    }

    /// The wake pass's turn: held for the whole pass.
    pub(crate) async fn pass(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.pass.lock().await
    }

    /// Whether wakes are held back.
    pub(crate) fn held(&self) -> bool {
        self.held.load(Ordering::SeqCst)
    }
}
