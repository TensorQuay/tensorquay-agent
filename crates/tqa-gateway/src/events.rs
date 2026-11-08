//! Durable, contiguous event storage followed by bounded in-process publication.

mod gate;
mod hub;
mod persist;
mod read;
mod runs;
pub(crate) use runs::{RunActivity, run_activity};

use crate::{GatewayConfig, GatewayError, storage::Storage, telemetry::Telemetry};
use async_trait::async_trait;
pub use gate::{OpenPoint, StreamOpenHold};
pub(crate) use hub::Subscription;
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tqa_types::{ConversationId, Seq, ServerFrame};

pub(crate) struct StoreResources {
    pub pool: PgPool,
    pub storage: Storage,
    pub cancel: CancellationToken,
    pub telemetry: Arc<Telemetry>,
}

/// A `PostgreSQL` journal with commit-before-publication and ordered per-conversation append admission.
#[derive(Clone)]
pub struct EventStore {
    pool: PgPool,
    storage: Storage,
    hub: Arc<hub::Hub>,
    /// A test-only hold point around a stream's subscription (a delete racing it); nothing holds it otherwise.
    open_gate: Arc<gate::OpenGate>,
    timeout: std::time::Duration,
    cancel: CancellationToken,
    payload_cap: usize,
    page_cap: u32,
    telemetry: Arc<Telemetry>,
    /// The agent-team reads waiting for their results; absent when agent teams are off (ADR-0034).
    team_reads: Option<Arc<crate::teams::pending::Pending>>,
}

impl EventStore {
    pub(crate) fn new(config: &GatewayConfig, resources: StoreResources) -> Self {
        Self {
            pool: resources.pool,
            storage: resources.storage,
            hub: Arc::new(hub::Hub::new(
                config.limits.live_conversations,
                config.limits.broadcast_capacity,
            )),
            open_gate: Arc::default(),
            timeout: config.io_timeout(),
            cancel: resources.cancel,
            payload_cap: config.event_payload_cap_bytes,
            page_cap: config.limits.page_size,
            telemetry: resources.telemetry,
            team_reads: config
                .agent_teams
                .enabled
                .then(|| Arc::new(crate::teams::pending::Pending::default())),
        }
    }

    /// The outstanding agent-team reads, when agent teams are on.
    pub(crate) fn team_reads(&self) -> Option<&Arc<crate::teams::pending::Pending>> {
        self.team_reads.as_ref()
    }

    /// Persists exactly the next sequence, then publishes it; control frames are not journal entries.
    ///
    /// # Errors
    /// Rejects invalid frames, duplicates, gaps, deleted conversations and bounded storage failures.
    pub async fn append(
        &self,
        conversation: &ConversationId,
        frame: ServerFrame,
    ) -> Result<(), GatewayError> {
        let frame = crate::wire::validated(&frame)?;
        let sequence = crate::wire::sequence(&frame)
            .ok_or(GatewayError::BadRequest("journal frame must be sequenced"))?;
        let sequence = crate::wire::db_sequence(sequence)?;
        let id = crate::conversations::uuid(conversation)?;
        let node = self.hub.node(conversation)?;
        crate::io::bounded(self.timeout, &self.cancel, async {
            let _admission = node
                .append
                .acquire()
                .await
                .map_err(|_| GatewayError::Cancelled)?;
            crate::delivery::commit_then_publish(
                Box::pin(self.persist(id, sequence, &frame)),
                || {
                    self.telemetry.batch(1);
                    self.telemetry.event(&frame);
                    node.publish(frame.clone());
                },
            )
            .await
        })
        .await
    }

    /// Persists consecutive plain journal frames in one transaction, then publishes them in sequence order
    /// (GW-I-177). A frame that isn't a sequenced event, or a run lifecycle event (it has per-event effects of its
    /// own and is appended alone), refuses the batch; so do a gap, a duplicate or disorder anywhere in it, and then
    /// nothing of it is stored or published.
    ///
    /// # Errors
    /// As [`Self::append`], for any frame of the batch.
    pub async fn append_batch(
        &self,
        conversation: &ConversationId,
        frames: Vec<ServerFrame>,
    ) -> Result<(), GatewayError> {
        let mut rows = Vec::with_capacity(frames.len());
        for frame in &frames {
            let frame = crate::wire::validated(frame)?;
            if !matches!(frame, ServerFrame::Event { .. })
                || crate::wire::run_transition(&frame).is_some()
            {
                return Err(GatewayError::BadRequest(
                    "a batch holds only plain journal events",
                ));
            }
            let sequence = crate::wire::sequence(&frame)
                .ok_or(GatewayError::BadRequest("journal frame must be sequenced"))?;
            rows.push((crate::wire::db_sequence(sequence)?, frame));
        }
        if rows.is_empty() {
            return Ok(());
        }
        let id = crate::conversations::uuid(conversation)?;
        let node = self.hub.node(conversation)?;
        crate::io::bounded(self.timeout, &self.cancel, async {
            let _admission = node
                .append
                .acquire()
                .await
                .map_err(|_| GatewayError::Cancelled)?;
            crate::delivery::commit_then_publish(Box::pin(self.persist_batch(id, &rows)), || {
                self.publish_batch(&node, &rows);
            })
            .await
        })
        .await
    }

    /// A committed batch's frames to clients, in sequence order.
    fn publish_batch(&self, node: &hub::Node, rows: &[(i64, ServerFrame)]) {
        self.telemetry.batch(rows.len());
        for (_, frame) in rows {
            self.telemetry.event(frame);
            node.publish(frame.clone());
        }
    }

    /// Reads a bounded page after an exclusive sequence, resolving blob-backed rows transparently.
    ///
    /// # Errors
    /// Rejects out-of-range pages, corrupt rows or unavailable storage.
    pub async fn read(
        &self,
        conversation: &ConversationId,
        after: Seq,
        limit: u32,
    ) -> Result<Vec<ServerFrame>, GatewayError> {
        self.read_through(conversation, after, i64::MAX, limit)
            .await
    }

    /// The hold point a stream's open passes before and after it subscribes (for tests; see [`gate::OpenGate`]).
    pub(crate) fn open_gate(&self) -> &gate::OpenGate {
        &self.open_gate
    }

    /// Ends the deleted task `conversation`'s open streams (the purge design, delete time).
    pub(crate) fn close(&self, conversation: &ConversationId) {
        self.hub.close(conversation);
    }

    /// Starts the hub's sweep of idle nodes, owned by `tasks` and stopped with the gateway (GW-U-178).
    pub(crate) fn start_hub_sweep(&self, tasks: &tokio_util::task::TaskTracker) {
        hub::start_sweep(
            self.hub.clone(),
            hub::SWEEP_EVERY,
            self.cancel.clone(),
            tasks,
        );
    }

    pub(crate) fn subscribe(
        &self,
        conversation: &ConversationId,
    ) -> Result<Subscription, GatewayError> {
        self.hub.subscribe(conversation)
    }

    pub(crate) fn control(
        &self,
        conversation: &ConversationId,
        frame: ServerFrame,
    ) -> Result<(), GatewayError> {
        self.hub.node(conversation)?.publish(frame);
        Ok(())
    }
}

#[async_trait]
impl crate::link::FrameSink for EventStore {
    async fn append(
        &self,
        conversation: &ConversationId,
        frame: ServerFrame,
    ) -> Result<(), GatewayError> {
        Self::append(self, conversation, frame).await
    }
    async fn recover_committed(
        &self,
        conversation: &ConversationId,
        frame: &ServerFrame,
    ) -> Result<bool, GatewayError> {
        let sequence = crate::wire::sequence(frame).ok_or(GatewayError::StoreUnavailable)?;
        let previous = crate::wire::db_sequence(sequence)?
            .checked_sub(1)
            .ok_or(GatewayError::StoreUnavailable)?;
        let stored = self
            .read(conversation, crate::wire::from_db_sequence(previous)?, 1)
            .await?;
        if stored.first() != Some(frame) {
            return Ok(false);
        }
        // Only an uncertain append reaches this path; its commit succeeded before publication.
        self.telemetry.event(frame);
        self.control(conversation, frame.clone())?;
        Ok(true)
    }
    async fn append_batch(
        &self,
        conversation: &ConversationId,
        frames: &[ServerFrame],
    ) -> Result<(), GatewayError> {
        Self::append_batch(self, conversation, frames.to_vec()).await
    }
    async fn recover_committed_batch(
        &self,
        conversation: &ConversationId,
        frames: &[ServerFrame],
    ) -> Result<bool, GatewayError> {
        let Some(first) = frames.first() else {
            return Ok(true);
        };
        let sequence = crate::wire::sequence(first).ok_or(GatewayError::StoreUnavailable)?;
        let previous = crate::wire::db_sequence(sequence)?
            .checked_sub(1)
            .ok_or(GatewayError::StoreUnavailable)?;
        let limit = u32::try_from(frames.len()).map_err(|_| GatewayError::StoreUnavailable)?;
        let stored = self
            .read(
                conversation,
                crate::wire::from_db_sequence(previous)?,
                limit,
            )
            .await?;
        // Every frame must be stored as sent before any is published; one that differs fails closed.
        if stored != frames {
            return Ok(false);
        }
        self.telemetry.batch(frames.len());
        for frame in frames {
            self.telemetry.event(frame);
            self.control(conversation, frame.clone())?;
        }
        Ok(true)
    }
    fn control(
        &self,
        conversation: &ConversationId,
        frame: ServerFrame,
    ) -> Result<(), GatewayError> {
        Self::control(self, conversation, frame)
    }
}
