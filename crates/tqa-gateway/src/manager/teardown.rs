//! A busy sandbox retired at its lifetime whose teardown fails under load: the teardown is retried in the background,
//! and the run resumes in a new sandbox after it.
use super::{SandboxManager, Slot, failure};
use crate::GatewayError;
use std::{sync::Arc, time::Duration};
use tqa_types::{ConversationId, SandboxState, ServerFrame};

/// The waits before the first background retries of a sandbox's teardown; after them it retries every
/// [`RETRY_EVERY`] until [`RETRY_CAP`] (the R2 soak on candidate 13d: an engine outage of two minutes outlasted 65 s).
const RETIRE_RETRIES: [Duration; 5] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(45),
    Duration::from_secs(90),
    Duration::from_mins(3),
];
const RETRY_EVERY: Duration = Duration::from_mins(3);
/// How long after the first failure the teardown is retried; the last wait is cut short so an attempt falls on it.
const RETRY_CAP: Duration = Duration::from_mins(10);

/// The waits between attempts: [`RETIRE_RETRIES`], then [`RETRY_EVERY`], ending with an attempt at [`RETRY_CAP`].
fn retry_waits() -> impl Iterator<Item = Duration> {
    let mut elapsed = Duration::ZERO;
    RETIRE_RETRIES
        .into_iter()
        .chain(std::iter::repeat(RETRY_EVERY))
        .map_while(move |wait| {
            let left = RETRY_CAP
                .checked_sub(elapsed)
                .filter(|left| !left.is_zero())?;
            let wait = wait.min(left);
            elapsed += wait;
            Some(wait)
        })
}

/// What follows a background teardown that succeeds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Then {
    /// The run that was retiring resumes in a new sandbox (a busy sandbox at its lifetime, a lost one).
    Resume,
    /// Nothing was running (an evicted idle sandbox): its room goes to admission; a new sandbox only for queued work.
    Idle,
}

impl SandboxManager {
    /// Retries a sandbox's teardown with backoff up to [`RETRY_CAP`] ([`retry_waits`]), then resumes its run in a new
    /// sandbox. If every retry
    /// fails, it looks before deciding: a removal that timed out may still have completed, and then the slot is
    /// released; one still present keeps its slot counted and its record, is logged, and is left to reconciliation or
    /// the next action, which tries the teardown first (a phantom slot held until restart would be an outage).
    pub(super) fn retire_later(self: &Arc<Self>, id: ConversationId, slot: Arc<Slot>, then: Then) {
        let manager = self.clone();
        slot.retrying
            .store(true, std::sync::atomic::Ordering::Release);
        self.services.tasks.spawn(async move {
            let retired = manager.retry_retire(&id, &slot).await;
            slot.retrying.store(false, std::sync::atomic::Ordering::Release);
            if retired {
                match then {
                    Then::Resume => manager.after_teardown(&id),
                    Then::Idle => manager.after_idle_teardown(&id).await,
                }
                return;
            }
            if manager.services.cancel.is_cancelled() {
                // The gateway is stopping: the sandbox's row survives, and the start-up reconciliation removes it.
                tracing::info!(conversation_id = %id, "a sandbox's teardown is left to the start-up reconciliation");
                return;
            }
            manager.after_failed_retries(&id, &slot, then).await;
        });
    }

    /// Every retry failed: looks before deciding. A removal that timed out may still have completed; one still present
    /// keeps its slot and record, and its run is reported to its owner.
    async fn after_failed_retries(
        self: &Arc<Self>,
        id: &ConversationId,
        slot: &Arc<Slot>,
        then: Then,
    ) {
        if matches!(self.present(slot).await, Ok(false)) && self.release(id, slot).await.is_ok() {
            tracing::info!(conversation_id = %id, "a sandbox's teardown completed although it timed out");
            match then {
                Then::Resume => self.after_teardown(id),
                Then::Idle => self.after_idle_teardown(id).await,
            }
            return;
        }
        if then == Then::Idle {
            // No run waits on it: nobody to tell. Its slot stays counted, and its next action tries the teardown first.
            tracing::error!(conversation_id = %id,
                "an evicted sandbox could not be removed; its slot stays counted until its next action or a restart");
            return;
        }
        tracing::error!(
            conversation_id = %id,
            "a sandbox could not be removed; its slot stays counted until reconciliation or the next action"
        );
        self.stranded(id);
    }

    /// After an evicted sandbox's late teardown: its room goes to admission, and its conversation gets a sandbox only for
    /// work queued meanwhile.
    async fn after_idle_teardown(self: &Arc<Self>, id: &ConversationId) {
        self.nudge_admission();
        if matches!(self.has_queued(id).await, Ok(true)) && self.schedule(id).is_err() {
            tracing::warn!(conversation_id = %id, "work queued after an eviction could not be scheduled");
        }
    }

    /// After a late teardown: the run retired at its lifetime resumes in a new sandbox.
    fn after_teardown(self: &Arc<Self>, id: &ConversationId) {
        self.nudge_admission();
        if self.schedule(id).is_err() {
            tracing::warn!(conversation_id = %id, "a run could not be rescheduled after a late teardown");
        }
    }

    /// A run whose old sandbox could not be removed: the owner is told, retryably, instead of a run that never moves.
    fn stranded(&self, id: &ConversationId) {
        let told = self
            .services
            .events
            .control(
                id,
                ServerFrame::Error {
                    seq: None,
                    code: tqa_types::ErrorCode::SandboxFailed,
                    message: "this task's sandbox could not be removed, so a new one was not started; send a message \
                              to try again"
                        .into(),
                    retryable: true,
                    target: None,
                },
            )
            .and_then(|()| self.state_notice(id, SandboxState::Failed));
        if told.is_err() {
            tracing::warn!(conversation_id = %id, "a stranded run's owner could not be told");
        }
    }

    /// The teardown again at each of [`retry_waits`]' times, counted from the first failure (now), so an attempt that
    /// takes long never moves the later ones or the last at [`RETRY_CAP`]. The times an attempt overran collapse into
    /// one attempt at once, never a burst against a stalled engine. Whether one attempt succeeded.
    async fn retry_retire(&self, id: &ConversationId, slot: &Arc<Slot>) -> bool {
        let clock = &self.services.idle_clock;
        // The idle policy's clock, so tests advance it instead of waiting in real time.
        let start = clock.now();
        let mut dues = retry_waits()
            .scan(start, |at, wait| {
                *at += wait;
                Some(*at)
            })
            .peekable();
        while let Some(due) = dues.next() {
            while clock.now() < due {
                tokio::select! {
                    biased;
                    () = self.services.cancel.cancelled() => return false,
                    () = clock.changed() => {}
                    () = tokio::time::sleep(due.saturating_sub(clock.now())) => {}
                }
            }
            let now = clock.now();
            while dues.next_if(|next| *next <= now).is_some() {}
            if self.retire(id, slot, super::Teardown::Retry).await.is_ok() {
                return true;
            }
        }
        false
    }

    /// Whether the slot's sandbox still exists on the backend.
    async fn present(&self, slot: &Slot) -> Result<bool, GatewayError> {
        let handle = slot
            .handle
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .clone();
        let Some(handle) = handle else {
            return Ok(false);
        };
        let listed = self
            .bounded(async { self.backend.engine.list().await.map_err(failure("list")) })
            .await?;
        Ok(listed.iter().any(|listed| listed.id == handle.id))
    }
}

#[cfg(test)]
mod tests {
    use super::retry_waits;
    use std::time::Duration;

    // TEST: GW-I-160
    #[test]
    fn gw_i_160_the_teardown_is_retried_with_backoff_up_to_ten_minutes() {
        let at: Vec<u64> = retry_waits()
            .scan(Duration::ZERO, |elapsed, wait| {
                *elapsed += wait;
                Some(elapsed.as_secs())
            })
            .collect();
        assert_eq!(at, [5, 20, 65, 155, 335, 515, 600]);
    }
}
