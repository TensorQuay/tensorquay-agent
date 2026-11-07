//! Run activity is a projection of stored lifecycle events, never a stored status (ADR-0016).
use super::EventStore;
use crate::GatewayError;
use tqa_types::{ConversationId, SandboxState};

/// The lifecycle position of a conversation's latest run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunActivity {
    /// No run has started.
    None,
    /// The latest run started or resumed and has no terminal event yet.
    Active,
    /// The latest run has its terminal event.
    Ended,
}

impl RunActivity {
    /// The client-facing state of a registered sandbox with this run activity.
    pub(crate) const fn registered_state(self) -> SandboxState {
        match self {
            Self::None => SandboxState::Ready,
            Self::Active => SandboxState::Busy,
            Self::Ended => SandboxState::Idle,
        }
    }

    /// The run has not finished, so its route and environment must not change.
    pub(crate) const fn unfinished(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Reads the latest run lifecycle event through the partial lifecycle index, inside any executor, so a
/// caller holding the conversation lock sees a decision that no concurrent append can change.
///
/// # Errors
/// Fails on a storage error.
pub(crate) async fn run_activity(
    executor: impl sqlx::PgExecutor<'_>,
    id: uuid::Uuid,
) -> Result<RunActivity, GatewayError> {
    let latest = sqlx::query_scalar!(
        "SELECT lifecycle FROM events WHERE conversation_id=$1 AND lifecycle IS NOT NULL ORDER BY seq DESC LIMIT 1",
        id
    )
    .fetch_optional(executor)
    .await?;
    Ok(match latest.flatten().as_deref() {
        None => RunActivity::None,
        Some("run.started" | "run.resumed") => RunActivity::Active,
        Some(_) => RunActivity::Ended,
    })
}

impl EventStore {
    /// Reads the latest run lifecycle event through the partial lifecycle index.
    ///
    /// # Errors
    /// Fails on a malformed identifier or a bounded storage failure.
    pub(crate) async fn run_activity(
        &self,
        conversation: &ConversationId,
    ) -> Result<RunActivity, GatewayError> {
        let id = crate::conversations::uuid(conversation)?;
        crate::io::bounded(self.timeout, &self.cancel, run_activity(&self.pool, id)).await
    }
}

/// At most this many interrupted conversations are resumed by one start-up reconciliation.
pub(crate) const MAX_INTERRUPTED: i64 = 4096;

impl EventStore {
    /// Lists active conversations whose latest run has no terminal event and no sandbox record:
    /// runs a failure cleanup or a lost instance interrupted. Bounded by [`MAX_INTERRUPTED`].
    ///
    /// # Errors
    /// Fails on a bounded storage failure or a corrupt identifier.
    pub(crate) async fn interrupted_without_sandbox(
        &self,
    ) -> Result<Vec<ConversationId>, GatewayError> {
        let rows = crate::io::bounded(self.timeout, &self.cancel, async {
            Ok(sqlx::query_scalar!(
                "SELECT c.id FROM conversations c CROSS JOIN LATERAL (SELECT e.lifecycle AS kind FROM events e WHERE e.conversation_id=c.id AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1) latest WHERE c.state='active' AND latest.kind IN ('run.started','run.resumed') AND NOT EXISTS (SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=c.id) ORDER BY c.id LIMIT $1",
                MAX_INTERRUPTED + 1
            )
            .fetch_all(&self.pool)
            .await?)
        })
        .await?;
        if rows.len() > usize::try_from(MAX_INTERRUPTED).map_err(|_| GatewayError::Busy)? {
            return Err(GatewayError::Busy);
        }
        rows.into_iter()
            .map(|id| {
                id.to_string()
                    .parse()
                    .map_err(|_| GatewayError::StoreUnavailable)
            })
            .collect()
    }
}
