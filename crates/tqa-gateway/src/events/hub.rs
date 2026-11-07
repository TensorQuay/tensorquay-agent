use crate::GatewayError;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Semaphore, broadcast};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tqa_types::{ConversationId, ServerFrame};

#[cfg(test)]
#[path = "hub_tests.rs"]
mod tests;

pub(super) struct Node {
    pub append: Semaphore,
    sender: broadcast::Sender<Arc<ServerFrame>>,
    /// Cancelled when the task is deleted: its open streams end, and their receivers drop the frames they hold.
    closed: CancellationToken,
}

impl Node {
    pub fn publish(&self, frame: ServerFrame) {
        // A durable append remains successful when all browsers have disconnected.
        let _no_receivers_is_allowed = self.sender.send(Arc::new(frame));
    }
}

pub(crate) struct Subscription {
    node: Arc<Node>,
    pub receiver: broadcast::Receiver<Arc<ServerFrame>>,
}

impl Subscription {
    /// Cancelled when the task is deleted ([`Hub::close`]).
    pub fn closed(&self) -> CancellationToken {
        self.node.closed.clone()
    }
}

pub(super) struct Hub {
    nodes: Mutex<HashMap<ConversationId, Arc<Node>>>,
    capacity: usize,
    queue: usize,
}

impl Hub {
    pub fn new(capacity: usize, queue: usize) -> Self {
        Self {
            nodes: Mutex::new(HashMap::new()),
            capacity,
            queue,
        }
    }

    pub fn node(&self, id: &ConversationId) -> Result<Arc<Node>, GatewayError> {
        let mut nodes = self
            .nodes
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if let Some(node) = nodes.get(id) {
            return Ok(node.clone());
        }
        if nodes.len() >= self.capacity {
            nodes.retain(|_, node| Arc::strong_count(node) > 1);
        }
        if nodes.len() >= self.capacity {
            return Err(GatewayError::Busy);
        }
        let (sender, _) = broadcast::channel(self.queue);
        let node = Arc::new(Node {
            append: Semaphore::new(1),
            sender,
            closed: CancellationToken::new(),
        });
        nodes.insert(id.clone(), node.clone());
        Ok(node)
    }

    pub fn subscribe(&self, id: &ConversationId) -> Result<Subscription, GatewayError> {
        let node = self.node(id)?;
        let receiver = node.sender.subscribe();
        Ok(Subscription { node, receiver })
    }

    /// Drops every node nobody else holds: no open stream and no append in flight (each holds an `Arc` of it), so a
    /// broadcast sent on it would reach no one anyway; the task's next append or stream makes a fresh one. Each node
    /// keeps its channel's whole ring (`broadcast_capacity` slots, about 40 KB) for as long as it is in the map, and
    /// below the cap nothing else dropped a task's node until the task was deleted (c15's soak, 9 Oct 2026; GW-U-178).
    pub fn sweep_idle(&self) -> usize {
        let mut nodes = self
            .nodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = nodes.len();
        nodes.retain(|_, node| Arc::strong_count(node) > 1);
        before - nodes.len()
    }

    /// Nodes held now.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.nodes.lock().map_or(0, |nodes| nodes.len())
    }

    /// A deleted task's node goes: each open stream on it ends, and a later subscriber gets a new, empty node.
    pub fn close(&self, id: &ConversationId) {
        let node = self
            .nodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
        if let Some(node) = node {
            node.closed.cancel();
        }
    }
}

/// How often [`start_sweep`] drops the hub's idle nodes.
pub(super) const SWEEP_EVERY: Duration = Duration::from_mins(1);

/// Runs [`Hub::sweep_idle`] every `every` until `cancel` (the gateway's stop), as the gateway's other sweeps do.
pub(super) fn start_sweep(
    hub: Arc<Hub>,
    every: Duration,
    cancel: CancellationToken,
    tasks: &TaskTracker,
) {
    tasks.spawn(async move {
        let mut timer = tokio::time::interval(every);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                _ = timer.tick() => {
                    let dropped = hub.sweep_idle();
                    if dropped > 0 {
                        tracing::debug!(dropped, "idle event hub nodes dropped");
                    }
                }
            }
        }
    });
}
