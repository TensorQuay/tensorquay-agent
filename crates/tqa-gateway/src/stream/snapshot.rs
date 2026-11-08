//! The client-facing sandbox snapshot a stream opens with, and re-sends after replay.
use crate::{GatewayError, app, conversations};
use tqa_sandbox::ReadinessSource;
use tqa_types::{
    ConversationId, ErrorCode, QueuePosition, SandboxBackend, SandboxState, SandboxSummary,
    ServerFrame,
};

/// The current `session.ready`, and the wire code of the latest action when it failed.
///
/// # Errors
/// Fails on a bounded storage error.
pub(super) async fn ready(
    state: &app::State,
    id: &ConversationId,
    backend: SandboxBackend,
    resumed: bool,
) -> Result<(ServerFrame, Option<ErrorCode>), GatewayError> {
    let (sandbox, queue, failed) = sandbox_state(state, id).await?;
    Ok((
        ServerFrame::SessionReady {
            conversation_id: id.clone(),
            resumed,
            sandbox: SandboxSummary::new(backend, sandbox),
            queue,
        },
        failed,
    ))
}

async fn sandbox_state(
    state: &app::State,
    conversation: &ConversationId,
) -> Result<(SandboxState, Option<QueuePosition>, Option<ErrorCode>), GatewayError> {
    if state.links.is_ready(conversation).await {
        return Ok((
            state
                .events
                .run_activity(conversation)
                .await?
                .registered_state(),
            None,
            None,
        ));
    }
    let id = conversations::uuid(conversation)?;
    let phase = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT phase FROM sandbox_instances WHERE conversation_id=$1",
            id
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    // A record being destroyed is not starting: the conversation is failing, waiting or going to sleep.
    if phase.as_deref().is_some_and(|phase| phase != "destroying") {
        return Ok((SandboxState::Creating, None, None));
    }
    if let Some(code) = latest_action_failure(state, id).await? {
        return Ok((SandboxState::Failed, None, Some(code)));
    }
    // Accepted work without a sandbox record waits in the fair queue.
    if let Some(position) = state.manager.queue_position(conversation).await? {
        return Ok((SandboxState::Queued, Some(position), None));
    }
    // Allocation is lazy: no record means the first message must still be allowed to start it.
    Ok((SandboxState::Destroyed, None, None))
}

/// A failure is derived, not stored as state: the conversation's latest accepted action failed, with this code.
/// A later action has a higher ordinal and clears it.
async fn latest_action_failure(
    state: &app::State,
    id: uuid::Uuid,
) -> Result<Option<ErrorCode>, GatewayError> {
    let latest = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT execution_status,execution_error FROM idempotency WHERE conversation_id=$1 ORDER BY ordinal DESC LIMIT 1",
            id
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    Ok(latest.and_then(|row| {
        let code = match row.execution_error.as_deref() {
            Some("runtime_start_failed") => ErrorCode::RuntimeStartFailed,
            Some("decision_engine_unavailable") => ErrorCode::DecisionEngineUnavailable,
            _ => ErrorCode::SandboxFailed,
        };
        (row.execution_status == "failed").then_some(code)
    }))
}

/// Tells a stream opened after the failure what happened and that sending again retries.
pub(super) fn failed_action_notice(code: ErrorCode) -> ServerFrame {
    let message = match code {
        ErrorCode::RuntimeStartFailed => {
            "The last message was not delivered: the runtime exited before it started."
        }
        ErrorCode::DecisionEngineUnavailable => {
            "The last message was not delivered: this task's decision engine is no longer available; an administrator \
             must restore or re-enable it."
        }
        _ => "The last message was not delivered: its sandbox failed.",
    };
    ServerFrame::Error {
        seq: None,
        code,
        message: format!("{message} Send it again to retry."),
        retryable: true,
        target: None,
    }
}
