use crate::{GatewayError, SandboxId};
use async_trait::async_trait;
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::ReadinessSource;
use tqa_types::{ClientFrame, ConversationId};
use uuid::Uuid;

pub(crate) struct Delivery {
    pub frame: ClientFrame,
    pub sent: tokio::sync::oneshot::Sender<()>,
}

pub(crate) struct Registration {
    pub generation: Uuid,
    pub actions: mpsc::Receiver<Delivery>,
    pub cancel: CancellationToken,
    /// This link's workspace lease renewals.
    pub renewals: super::renewals::Renewals,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

struct Entry {
    sandbox: SandboxId,
    /// The protocol revision the runtime announced: host keys go only to one at [`tqa_types::HOST_KEYS_REVISION`].
    revision: u16,
    heartbeat: Option<RuntimeHeartbeat>,
    generation: Uuid,
    sender: mpsc::Sender<Delivery>,
    cancel: CancellationToken,
}

/// The most recent supervisor heartbeat; socket pongs never update this observation.
#[derive(Clone, Debug)]
pub struct RuntimeHeartbeat {
    /// Run whose supervisor emitted the frame.
    pub run_id: tqa_types::RunId,
    /// Monotonic receive time for later sandbox liveness decisions.
    pub received_at: tokio::time::Instant,
}

/// Pending workspace requests per link unless configured.
const DEFAULT_WORKSPACE_REQUESTS: usize = 8;

pub(crate) struct LinkRegistry {
    entries: Mutex<HashMap<ConversationId, Entry>>,
    /// Workspace requests in flight (ADR-0030), whose replies are answers rather than client notices.
    pub workspace: super::workspace::WorkspaceRequests,
    capacity: usize,
    queue: usize,
    cancel: CancellationToken,
}

impl LinkRegistry {
    pub fn new(capacity: usize, queue: usize, cancel: CancellationToken) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            workspace: super::workspace::WorkspaceRequests::new(DEFAULT_WORKSPACE_REQUESTS),
            capacity,
            queue,
            cancel,
        }
    }

    /// Allows `per_link` pending workspace requests on each link (`[files] link_requests`).
    #[must_use]
    pub fn with_workspace_requests(mut self, per_link: usize) -> Self {
        self.workspace = super::workspace::WorkspaceRequests::new(per_link);
        self
    }

    /// The live link's generation and cancellation.
    ///
    /// # Errors
    /// `409 sandbox_not_ready` without a live link.
    pub(super) fn link(
        &self,
        conversation: &ConversationId,
    ) -> Result<(Uuid, CancellationToken), GatewayError> {
        self.entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .get(conversation)
            .filter(|entry| !entry.cancel.is_cancelled())
            .map(|entry| (entry.generation, entry.cancel.clone()))
            .ok_or(GatewayError::Conflict("sandbox_not_ready"))
    }

    /// Registers a runtime announcing the current revision.
    #[cfg(test)]
    pub fn register(
        &self,
        conversation: ConversationId,
        sandbox: SandboxId,
    ) -> Result<Registration, GatewayError> {
        self.register_at(conversation, sandbox, tqa_types::PROTOCOL_REVISION)
    }

    /// Registers the runtime of `sandbox` for `conversation`, announcing protocol `revision`.
    pub fn register_at(
        &self,
        conversation: ConversationId,
        sandbox: SandboxId,
        revision: u16,
    ) -> Result<Registration, GatewayError> {
        if self.cancel.is_cancelled() {
            return Err(GatewayError::Cancelled);
        }
        let (sender, actions) = mpsc::channel(self.queue);
        let cancel = self.cancel.child_token();
        let generation = Uuid::now_v7();
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        entries.retain(|_, entry| !entry.cancel.is_cancelled() && !entry.sender.is_closed());
        if !entries.contains_key(&conversation) && entries.len() >= self.capacity {
            return Err(GatewayError::Busy);
        }
        let old = entries.insert(
            conversation,
            Entry {
                sandbox,
                revision,
                heartbeat: None,
                generation,
                sender,
                cancel: cancel.clone(),
            },
        );
        drop(entries);
        if let Some(old) = old {
            old.cancel.cancel();
        }
        Ok(Registration {
            generation,
            actions,
            cancel,
            renewals: super::renewals::Renewals::default(),
        })
    }

    pub fn unregister(
        &self,
        conversation: &ConversationId,
        generation: Uuid,
    ) -> Result<(), GatewayError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if entries
            .get(conversation)
            .is_some_and(|entry| entry.generation == generation)
            && let Some(entry) = entries.remove(conversation)
        {
            entry.cancel.cancel();
        }
        Ok(())
    }

    /// Whether a runtime link for `conversation` is registered (the purge waits for it to go).
    pub fn connected(&self, conversation: &ConversationId) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(conversation)
    }

    pub fn remove(&self, conversation: &ConversationId) -> Result<(), GatewayError> {
        let entry = self
            .entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .remove(conversation);
        if let Some(entry) = entry {
            entry.cancel.cancel();
        }
        Ok(())
    }

    pub fn remove_sandbox(
        &self,
        conversation: &ConversationId,
        sandbox: &SandboxId,
    ) -> Result<(), GatewayError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if entries
            .get(conversation)
            .is_some_and(|entry| entry.sandbox == *sandbox)
            && let Some(entry) = entries.remove(conversation)
        {
            entry.cancel.cancel();
        }
        Ok(())
    }

    pub fn saw_heartbeat(
        &self,
        conversation: &ConversationId,
        generation: Uuid,
        run_id: tqa_types::RunId,
        received_at: tokio::time::Instant,
    ) -> Result<(), GatewayError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if let Some(entry) = entries
            .get_mut(conversation)
            .filter(|entry| entry.generation == generation)
        {
            entry.heartbeat = Some(RuntimeHeartbeat {
                run_id,
                received_at,
            });
        }
        Ok(())
    }

    /// The conversations whose runtime is linked and sent a run's heartbeat within `within` of `now`: a supervisor
    /// sends one only while a run is active, so these are the runs going now, give or take one ended since its last.
    pub fn heartbeating(
        &self,
        now: tokio::time::Instant,
        within: std::time::Duration,
    ) -> Result<Vec<ConversationId>, GatewayError> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .iter()
            .filter(|(_, entry)| {
                entry
                    .heartbeat
                    .as_ref()
                    .is_some_and(|beat| now.saturating_duration_since(beat.received_at) <= within)
            })
            .map(|(conversation, _)| conversation.clone())
            .collect())
    }

    pub fn heartbeat(
        &self,
        conversation: &ConversationId,
    ) -> Result<Option<RuntimeHeartbeat>, GatewayError> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .get(conversation)
            .and_then(|entry| entry.heartbeat.clone()))
    }

    pub fn reserve(
        &self,
        conversation: &ConversationId,
    ) -> Result<mpsc::OwnedPermit<Delivery>, GatewayError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let entry = entries
            .get(conversation)
            .filter(|entry| !entry.cancel.is_cancelled())
            .ok_or(GatewayError::Conflict("sandbox_not_ready"))?;
        entry
            .sender
            .clone()
            .try_reserve_owned()
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => GatewayError::Busy,
                mpsc::error::TrySendError::Closed(_) => GatewayError::Conflict("sandbox_not_ready"),
            })
    }

    /// The sandbox whose runtime is linked for the task, if one is, and the revision it announced.
    pub fn sandbox(&self, conversation: &ConversationId) -> Option<(SandboxId, u16)> {
        self.entries.lock().ok().and_then(|entries| {
            entries
                .get(conversation)
                .filter(|entry| !entry.cancel.is_cancelled())
                .map(|entry| (entry.sandbox.clone(), entry.revision))
        })
    }

    pub async fn deliver(
        &self,
        conversation: &ConversationId,
        frame: ClientFrame,
    ) -> Result<(), GatewayError> {
        let permit = self.reserve(conversation)?;
        let (sent, received) = tokio::sync::oneshot::channel();
        permit.send(Delivery { frame, sent });
        received
            .await
            .map_err(|_| GatewayError::Conflict("sandbox_not_ready"))
    }
}

#[async_trait]
impl ReadinessSource for LinkRegistry {
    async fn is_ready(&self, conversation_id: &ConversationId) -> bool {
        self.entries.lock().is_ok_and(|entries| {
            entries
                .get(conversation_id)
                .is_some_and(|entry| !entry.cancel.is_cancelled() && !entry.sender.is_closed())
        })
    }
}
