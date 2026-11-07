use crate::{GatewayError, app::State};
use std::{sync::Arc, time::Duration};

pub(crate) fn start(state: &Arc<State>) {
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(
            owned.config.limits.idempotency_purge_seconds,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;
                ()=owned.cancel.cancelled()=>break,
                _=interval.tick()=>if purge(&owned).await.is_err() && !owned.cancel.is_cancelled() {
                    tracing::warn!("idempotency retention sweep failed");
                },
            }
        }
    });
}

async fn purge(state: &State) -> Result<(), GatewayError> {
    let seconds = f64::from(state.config.limits.idempotency_hours) * 3600.0;
    let batch = i64::from(state.config.limits.page_size);
    crate::io::bounded(state.config.io_timeout(),&state.cancel,async {
        sqlx::query!("DELETE FROM idempotency WHERE (key,user_id) IN (SELECT key,user_id FROM idempotency WHERE execution_status<>'queued' AND created_at<=CURRENT_TIMESTAMP-make_interval(secs=>$1::double precision) ORDER BY created_at LIMIT $2)",seconds,batch).execute(&state.pool).await?;
        Ok(())
    }).await
}
