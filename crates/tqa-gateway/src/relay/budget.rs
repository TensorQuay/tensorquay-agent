//! An engine's optional budget in the relay (ADR-0013, the adapter amendment): the engine's usage rows over its current
//! count (since the budget was set or last changed, or the calendar month in UTC) against its calls or tokens.
//! Checked before any call leaves the gateway; once spent, a consultation is refused and the task's owner told once per
//! period. Consultations in flight when it runs out may pass it, by at most their number.
use std::{
    collections::HashSet,
    sync::{Mutex, PoisonError},
};
use time::{OffsetDateTime, Time};
use tqa_types::{ConversationId, ErrorCode, ServerFrame};

use crate::{
    GatewayError, app,
    decision_engines::{BudgetPeriod, EngineBudget, EngineDocument},
};

/// The most (task, engine, period) notices remembered before the set starts over; a notice may then repeat once.
const MAX_NOTICES: usize = 4096;

/// The tasks already told that an engine's budget is spent, by task, engine and period.
#[derive(Default)]
pub(crate) struct Notices(Mutex<HashSet<String>>);

impl Notices {
    /// Whether this is the first notice for the key; remembers it.
    fn first(&self, key: String) -> bool {
        let mut told = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if told.len() >= MAX_NOTICES {
            told.clear();
        }
        told.insert(key)
    }
}

/// An engine's use over its budget's current count, and where that count starts.
pub(crate) struct Used {
    /// Consultations counted.
    pub calls: i64,
    /// Their tokens, input and output, as the service reported them.
    pub tokens: i64,
    /// The count's start: the month's first instant, or when the budget was set or last changed.
    pub since: Option<OffsetDateTime>,
}

/// The engine's use over its budget's current count: this calendar month in UTC for `month`, whatever changed in it;
/// for `total`, since the budget was set or last changed (`budget_since`, migration 0058).
///
/// # Errors
/// A store failure.
pub(crate) async fn used<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    name: &str,
    budget: &EngineBudget,
) -> Result<Used, GatewayError> {
    let month = match budget.period {
        BudgetPeriod::Total => None,
        BudgetPeriod::Month => {
            let now = OffsetDateTime::now_utc();
            Some(
                now.replace_day(1)
                    .map_err(|_| GatewayError::Io)?
                    .replace_time(Time::MIDNIGHT),
            )
        }
    };
    let route = format!("decision:{name}");
    let row = sqlx::query!(
        "SELECT count(u.id) AS \"calls!\", \
         COALESCE(sum(COALESCE(u.prompt_tokens,0)+COALESCE(u.completion_tokens,0)),0)::bigint AS \"tokens!\", \
         COALESCE($2::timestamptz, e.budget_since) AS since \
         FROM decision_engines e LEFT JOIN usage u ON u.route=$1 \
         AND u.created_at >= COALESCE($2::timestamptz, e.budget_since, '-infinity'::timestamptz) \
         WHERE e.name=$3 GROUP BY e.budget_since",
        route,
        month,
        name
    )
    .fetch_optional(executor)
    .await?;
    Ok(row.map_or(
        Used {
            calls: 0,
            tokens: 0,
            since: month,
        },
        |row| Used {
            calls: row.calls,
            tokens: row.tokens,
            since: row.since,
        },
    ))
}

/// Refuses the consultation when the engine's budget for the period is spent, telling the task's owner once per count.
///
/// # Errors
/// `409 decision_engine_budget_spent`, or a store failure.
pub(super) async fn check(
    state: &app::State,
    document: &EngineDocument,
    conversation: &ConversationId,
) -> Result<(), GatewayError> {
    let Some(budget) = document.budget else {
        return Ok(());
    };
    let used = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        used(&state.pool, &document.name, &budget).await
    })
    .await?;
    let spent = budget
        .calls
        .is_some_and(|calls| u64::try_from(used.calls).unwrap_or(u64::MAX) >= calls)
        || budget
            .tokens
            .is_some_and(|tokens| u64::try_from(used.tokens).unwrap_or(u64::MAX) >= tokens);
    if !spent {
        return Ok(());
    }
    // One notice per task and count: a new month, or a new budget, may be told again.
    let count = used.since.map_or(0, OffsetDateTime::unix_timestamp);
    if state.relay.budget_notices.first(format!(
        "{conversation}\u{1f}{}\u{1f}{count}",
        document.name
    )) {
        tracing::info!(engine = %document.name, "decision engine budget spent");
        // The notice is a courtesy to those watching; failing to publish it never replaces the refusal.
        if let Err(error) = state.events.control(
            conversation,
            ServerFrame::Error {
                seq: None,
                code: ErrorCode::DecisionEngineBudgetSpent,
                message:
                    "this task's decision engine has used its budget; the agent decides on its own"
                        .into(),
                retryable: false,
                target: None,
            },
        ) {
            tracing::warn!(engine = %document.name, %error, "the budget notice could not be published");
        }
    }
    Err(GatewayError::Conflict("decision_engine_budget_spent"))
}
