//! GW-I-191: the periodic sweep of sandbox containers nobody recorded. A create the engine finishes after its last
//! attempt gave up (SBX-U-014) leaves one; start-up reconciliation removes it too (GW-I-011), this sweep without a
//! restart.

use super::{SandboxManager, failure};
use crate::GatewayError;
use std::time::SystemTime;
use tqa_sandbox::SandboxHandle;

/// What one sweep did: strays removed, and strays it failed to remove (left for the next sweep).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContainerSweep {
    /// Strays removed, their runtime credentials revoked.
    pub removed: usize,
    /// Strays that could not be checked or removed this time.
    pub failed: usize,
}

impl SandboxManager {
    /// Removes this instance's sandbox containers that no record accounts for, as of `now`, and revokes their runtime
    /// credentials. A container is kept while it is younger than the create budget (its create may still be in
    /// flight), while its conversation's start is in progress (`starting`), or while its record names it. The listing
    /// holds only this instance's containers (`tqa.instance`), as reconciliation's does. One stray that cannot be
    /// checked or removed is logged and counted, and the sweep goes on to the next, as the network sweep does.
    ///
    /// # Errors
    /// The engine cannot list the containers.
    pub async fn sweep_containers(&self, now: SystemTime) -> Result<ContainerSweep, GatewayError> {
        let budget = self.config.sandbox.create_budget();
        let listed = self
            .bounded(async { self.backend.engine.list().await.map_err(failure("list")) })
            .await?;
        let mut sweep = ContainerSweep::default();
        for handle in listed {
            if now
                .duration_since(handle.created_at)
                .map_or(true, |age| age <= budget)
            {
                continue;
            }
            match self.sweep_container(&handle).await {
                Ok(true) => sweep.removed += 1,
                Ok(false) => {}
                Err(error) => {
                    sweep.failed += 1;
                    tracing::warn!(
                        conversation_id = %handle.conversation_id,
                        code = error.code(),
                        "a stray sandbox container could not be removed; the next sweep tries again"
                    );
                }
            }
        }
        Ok(sweep)
    }

    /// Removes one old container unless a record accounts for it; whether it was removed.
    async fn sweep_container(&self, handle: &SandboxHandle) -> Result<bool, GatewayError> {
        let conversation = crate::conversations::uuid(&handle.conversation_id)?;
        let record = self
            .bounded(async {
                sqlx::query!(
                    "SELECT sandbox_id,phase FROM sandbox_instances WHERE conversation_id=$1",
                    conversation
                )
                .fetch_optional(&self.services.pool)
                .await
                .map_err(GatewayError::from)
            })
            .await?;
        let accounted = record.is_some_and(|record| {
            record.phase == "starting" || record.sandbox_id.as_deref() == Some(&handle.id)
        });
        if accounted {
            return Ok(false);
        }
        super::teardown_begun(&handle.conversation_id, super::Teardown::Stray);
        self.services.tokens.revoke(&handle.id.parse()?).await?;
        self.backend
            .engine
            .destroy(handle)
            .await
            .map_err(failure("destroy"))?;
        Ok(true)
    }
}
