//! The channels' sweep (ADR-0014 point 9; its amendment of 20 Nov 2026, A3; the daily budget, decided on 21 Nov 2026):
//! every few seconds it decides the proposals whose window closed, renews the budget windows that ended, and sends the
//! wakes that are due.
use super::store;
use crate::{GatewayError, app::State};
use std::sync::Arc;

/// Decides the proposals whose window has closed, one transaction each; how many were decided. The gateway runs it
/// every few seconds (ADR-0014 point 9).
///
/// # Errors
/// Storage failure.
pub(crate) async fn sweep(state: &State) -> Result<usize, GatewayError> {
    let due = {
        let mut connection = state.pool.acquire().await?;
        store::due_proposals(&mut connection, None, false).await?
    };
    let mut decided = 0;
    for (channel, seq) in due {
        let mut transaction = state.pool.begin().await?;
        let decision = store::decide(&mut transaction, channel, seq, false).await?;
        transaction.commit().await?;
        if let Some(decision) = decision {
            state.channels.published(channel, decision.seq);
            decided += 1;
        }
    }
    // Budget windows that ended start again at 0 (C-3), so the wakes they held go out below with no new post.
    super::budget::renew(state).await?;
    // The wakes that are due, the decisions' among them, and any a restart lost or a run held back (A3).
    super::wake::send(state, None).await?;
    Ok(decided)
}

/// How often the sweep decides proposals whose window closed.
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Starts the decision sweep.
pub(crate) fn start(state: &Arc<State>) {
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_EVERY);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;
                () = owned.cancel.cancelled() => break,
                _ = interval.tick() => if sweep(&owned).await.is_err() && !owned.cancel.is_cancelled() {
                    tracing::warn!("channel decision sweep failed");
                },
            }
        }
    });
}
