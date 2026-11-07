//! Secret rotation reminders (vault slice 1, D1; GW-U-054, GW-I-093). A secret may carry `rotate_after_days`; once its
//! value is older than that, it is due: the admin list shows it, a WARN names it once a day, and the gauge
//! `tqa_secrets_rotation_due` counts it. A due reminder never stops a secret from working.
use crate::{GatewayError, app::State};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use time::OffsetDateTime;

/// How often reminders are checked.
const CHECK: Duration = Duration::from_hours(1);
/// A due secret is named in the log at most this often.
const WARN_EVERY: time::Duration = time::Duration::days(1);

/// Whether a value last changed at `rotated_at` is due for rotation at `now` under a reminder of `days`.
pub(crate) fn due(rotated_at: OffsetDateTime, days: Option<i32>, now: OffsetDateTime) -> bool {
    days.is_some_and(|days| now >= rotated_at + time::Duration::days(i64::from(days)))
}

/// When each due secret was last named in the log (in memory: a restart names each once more).
#[derive(Default)]
pub(crate) struct Reminders {
    warned: Mutex<HashMap<String, OffsetDateTime>>,
}

/// Checks every reminder now: sets the gauge and names each newly due secret. Returns how many are due.
///
/// # Errors
/// A store failure.
pub(crate) async fn check(state: &State) -> Result<u64, GatewayError> {
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT ref,rotated_at,rotate_after_days FROM secrets WHERE rotate_after_days IS NOT NULL ORDER BY ref"
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let now = OffsetDateTime::now_utc();
    let due: Vec<_> = rows
        .into_iter()
        .filter(|row| due(row.rotated_at, row.rotate_after_days, now))
        .collect();
    let count = u64::try_from(due.len()).unwrap_or(u64::MAX);
    state.telemetry.secrets_rotation_due(count);
    let mut warned = state
        .reminders
        .warned
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    warned.retain(|reference, _| due.iter().any(|row| &row.r#ref == reference));
    for row in &due {
        if warned
            .get(&row.r#ref)
            .is_some_and(|at| now - *at < WARN_EVERY)
        {
            continue;
        }
        warned.insert(row.r#ref.clone(), now);
        tracing::warn!(
            secret_ref = crate::secrets::loggable(&row.r#ref),
            rotate_after_days = row.rotate_after_days,
            "a secret is due for rotation"
        );
    }
    Ok(count)
}

/// Checks reminders at start and then hourly.
pub(crate) fn start(state: &Arc<State>) {
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut interval = tokio::time::interval(CHECK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;
                () = owned.cancel.cancelled() => break,
                _ = interval.tick() => if check(&owned).await.is_err() && !owned.cancel.is_cancelled() {
                    tracing::warn!("secret rotation reminder check failed");
                },
            }
        }
    });
}

#[cfg(test)]
#[path = "rotation_tests.rs"]
mod tests;
