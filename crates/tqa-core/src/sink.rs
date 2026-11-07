//! Durable append boundary; the runtime supplies sequencing, logging and delivery.
use async_trait::async_trait;
use tqa_types::{ErrorCode, Event, RunId, Seq};

/// The same run and step envelope used for durable events and local projection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventContext {
    /// The associated run, absent for a new user message.
    pub run_id: Option<RunId>,
    /// The associated step, absent before a step begins.
    pub step: Option<u32>,
}
/// A failed append; the core must stop writing immediately.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("event sink failed: {code}")]
pub struct SinkError {
    /// Stable failure classification without event contents.
    pub code: ErrorCode,
}
/// Assigns the sequence, writes the journal and delivers the event.
/// An append is never cancelled by the core; implementations bound their own write latency.
#[async_trait]
pub trait EventSink: Send + Sync {
    /// Appends exactly once and returns its durable sequence.
    ///
    /// # Errors
    /// Reports an unsuccessful append; callers must not attempt a compensating event.
    async fn emit(&self, event: Event, context: EventContext) -> Result<Seq, SinkError>;
}
