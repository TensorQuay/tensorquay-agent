use async_trait::async_trait;
use tqa_types::ConversationId;

use crate::SandboxError;
use futures::future::BoxFuture;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Gateway-owned runtime registration lookup; the backend owns polling and deadlines.
#[async_trait]
pub trait ReadinessSource: Send + Sync {
    /// Whether the runtime has registered for this conversation.
    async fn is_ready(&self, conversation_id: &ConversationId) -> bool;
}

type LivenessCheck<'a> = dyn FnMut() -> BoxFuture<'a, Result<(), SandboxError>> + Send + 'a;

pub(crate) async fn poll(
    conversation: &ConversationId,
    source: &dyn ReadinessSource,
    (deadline, interval): (Duration, Duration),
    cancel: &CancellationToken,
    running: &mut LivenessCheck<'_>,
) -> Result<(), SandboxError> {
    let registration = async {
        loop {
            if source.is_ready(conversation).await {
                return Ok(());
            }
            tokio::time::sleep(interval).await;
        }
    };
    let observed = async {
        // Require a live process even when a registration snapshot already says ready.
        running().await?;
        let liveness = async {
            loop {
                tokio::time::sleep(interval).await;
                running().await?;
            }
        };
        tokio::select! {
            biased;
            result = liveness => result,
            result = registration => result,
        }
    };
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(SandboxError::Cancelled),
        result = tokio::time::timeout(deadline, observed) => result.map_err(|_| SandboxError::Timeout)?,
    }
}

#[cfg(test)]
mod tests;
