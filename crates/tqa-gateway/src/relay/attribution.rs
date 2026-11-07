//! Which run a relayed call is for, as the key-read audit and the usage meter record it (the run-id contract). The
//! runtime is untrusted: the run it names is kept only when it is a run of the caller's own conversation, and it never
//! authorizes anything. A run the store doesn't hold yet (its `run.started` can be stored just after the call) is
//! waited for, briefly and only then; a call naming no run, or one that isn't this task's, is recorded as none and
//! counted, so old runtimes and misbehaving ones show up in `tqa_relay_unattributed_total`.
use crate::{GatewayError, app::State};
use std::time::Duration;
use tokio::sync::broadcast;
use tqa_types::{ConversationId, RunId};

/// The longest a call waits for its run to be stored; a stored run adds no wait at all.
pub(crate) const RUN_WAIT: Duration = Duration::from_secs(1);

/// The run a call may be recorded under: `claimed`, when it is a run of `conversation`, else none.
///
/// # Errors
/// A store failure.
pub(crate) async fn attributed(
    state: &State,
    conversation: &ConversationId,
    claimed: Option<RunId>,
    consumer: &str,
) -> Result<Option<RunId>, GatewayError> {
    let Some(run) = claimed else {
        state.telemetry.relay_unattributed(consumer, "missing");
        tracing::debug!(conversation_id = %conversation, consumer, "a relayed call named no run");
        return Ok(None);
    };
    let id = crate::conversations::uuid(conversation)?;
    if state.events.has_run(id, run.as_str()).await? {
        return Ok(Some(run));
    }
    if stored_within_wait(state, conversation, id, &run).await? {
        return Ok(Some(run));
    }
    state.telemetry.relay_unattributed(consumer, "foreign");
    tracing::debug!(conversation_id = %conversation, consumer, "a relayed call named a run that isn't its task's");
    Ok(None)
}

/// Waits, within [`RUN_WAIT`], for the run to be stored: the subscription starts before the read, and a frame is
/// published only after its row is committed, so a run stored meanwhile is never missed. Holds no database connection
/// while waiting, and ends when the request is dropped or the gateway stops.
async fn stored_within_wait(
    state: &State,
    conversation: &ConversationId,
    id: uuid::Uuid,
    run: &RunId,
) -> Result<bool, GatewayError> {
    let mut published = state.events.subscribe(conversation)?;
    let deadline = tokio::time::Instant::now() + RUN_WAIT;
    loop {
        if state.events.has_run(id, run.as_str()).await? {
            return Ok(true);
        }
        let received = tokio::select! {
            () = state.cancel.cancelled() => return Ok(false),
            received = tokio::time::timeout_at(deadline, published.receiver.recv()) => received,
        };
        match received {
            Err(_) | Ok(Err(broadcast::error::RecvError::Closed)) => return Ok(false),
            Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
        }
    }
}
