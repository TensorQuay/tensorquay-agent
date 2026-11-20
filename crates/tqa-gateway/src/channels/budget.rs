//! A channel's wake budget, set by the people who run it (ADR-0014's amendment, A5; decided on 20 Nov 2026, C-1).
use super::api::{ChannelView, caller, channel_id};
use super::store;
use crate::{
    GatewayError,
    app::State,
    auth::{Authz, Role},
    request::ApiJson,
};
use axum::{
    Json,
    extract::{Path, State as AppState},
};
use serde::Deserialize;
use sqlx::PgConnection;
use std::sync::Arc;
use tqa_types::{ChannelNoticeKind, ChannelRole, MessageBody};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BudgetBody {
    wake_budget: i64,
}

/// `n` with its thousands grouped, as a notice and a refusal say a budget ("1,000").
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `PATCH /api/v1/channels/{id} {wake_budget}`: the channel's owner, a person who leads it, or an administrator sets
/// the tokens its woken runs may spend (ADR-0014's amendment, A5; decided on 20 Nov 2026, C-1), from 0 (waking off)
/// to the installation's `[channels] max_wake_budget`. The channel says who set it, and a wake the budget held goes
/// out at once, without a new post.
///
/// # Errors
/// `404`, `403 not_lead`, `400` on `wake_budget`, `409 channel_ended`, storage failure.
pub(crate) async fn set_budget(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<BudgetBody>,
) -> Result<Json<ChannelView>, GatewayError> {
    let id = channel_id(&id)?;
    let ceiling = state.config.channels.max_wake_budget;
    let budget = u64::try_from(body.wake_budget)
        .ok()
        .filter(|budget| *budget <= ceiling)
        .ok_or_else(|| GatewayError::ParameterRefused {
            field: "wake_budget",
            said: format!(
                "A channel's budget is between 0 (no agent is woken) and {} tokens.",
                grouped(ceiling)
            ),
        })?;
    let stored = i64::try_from(budget).map_err(|_| GatewayError::StoreUnavailable)?;
    let seq = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let seen = store::visible(&mut transaction, caller(&user), id).await?;
        let leads = seen.channel.owner_id == user.id
            || user.role == Role::Admin
            || seen.role == Some(ChannelRole::Lead);
        if !leads {
            return Err(GatewayError::Denied("not_lead"));
        }
        sqlx::query!("UPDATE channels SET budget = $2 WHERE id = $1", id, stored)
            .execute(&mut *transaction)
            .await?;
        let seq = super::wake::notice(
            &mut transaction,
            id,
            (
                ChannelNoticeKind::WakeBudgetSet,
                Some((user.email.clone(), None)),
            ),
            format!(
                "{} set this channel's budget to {} tokens.",
                user.email,
                grouped(budget)
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(seq)
    })
    .await?;
    state.channels.published(id, seq);
    // A wake the budget held goes out now, as after a post (the sweep would find it later).
    super::wake::soon(&state, Some(id));
    tracing::info!(user_id = %user.id, channel_id = %id, budget, "channel wake budget set");
    super::api::get(AppState(state), Authz(user), Path(id.to_string())).await
}

/// Adds `tokens` to the channel a charged run of `conversation` spends for, in the meter's transaction (A5): 80 % and
/// 100 % are each said once, by the spend that crosses them. The budget is a daily allowance (decided on 21 Nov 2026,
/// C-3): a charge with no window open, or after it ended, opens a new one at 0, which lasts `budget_window`. Answers the
/// notices to publish after the commit.
///
/// # Errors
/// Storage failure.
pub(crate) async fn spend(
    connection: &mut PgConnection,
    conversation: Uuid,
    tokens: i64,
    (default_budget, window): (u64, std::time::Duration),
) -> Result<Vec<(Uuid, u64)>, GatewayError> {
    if tokens <= 0 {
        return Ok(Vec::new());
    }
    let window = window.as_secs_f64();
    let Some(channel) = sqlx::query!(
        "UPDATE channels c SET \
           spent_tokens = CASE WHEN c.budget_window_start IS NULL \
             OR c.budget_window_start + make_interval(secs => $3) <= CURRENT_TIMESTAMP THEN $2 \
             ELSE c.spent_tokens + $2 END, \
           budget_window_start = CASE WHEN c.budget_window_start IS NULL \
             OR c.budget_window_start + make_interval(secs => $3) <= CURRENT_TIMESTAMP THEN CURRENT_TIMESTAMP \
             ELSE c.budget_window_start END \
         FROM channel_charges g WHERE g.conversation_id = $1 AND c.id = g.channel_id \
         RETURNING c.id, c.spent_tokens, c.budget, c.budget_window_start + make_interval(secs => $3) AS \"resets!\"",
        conversation,
        tokens,
        window
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(Vec::new());
    };
    let budget = channel
        .budget
        .unwrap_or_else(|| i64::try_from(default_budget).unwrap_or(i64::MAX));
    let before = channel.spent_tokens - tokens;
    let mut published = Vec::new();
    if budget > 0 && crossed(before, channel.spent_tokens, budget, 80) {
        let text = "Woken agents have used 80 % of this channel's budget for today.".to_owned();
        published.push((
            channel.id,
            super::wake::notice(
                connection,
                channel.id,
                (ChannelNoticeKind::WakeBudget80, None),
                text,
            )
            .await?,
        ));
    }
    if budget > 0 && crossed(before, channel.spent_tokens, budget, 100) {
        published.push((
            channel.id,
            spent_notice(connection, channel.id, channel.resets).await?,
        ));
    }
    Ok(published)
}

/// The channel's notice that its budget for today is used up (A5, C-14): when it renews, in UTC for whoever reads the
/// text and as `resets_at` for a page to say in its reader's own time, and who can raise it sooner.
async fn spent_notice(
    connection: &mut PgConnection,
    channel: Uuid,
    resets: time::OffsetDateTime,
) -> Result<u64, GatewayError> {
    let at = resets.to_offset(time::UtcOffset::UTC);
    let clock = format!(
        "{:02}:{:02} UTC on {} {} {}",
        at.hour(),
        at.minute(),
        at.day(),
        at.month(),
        at.year()
    );
    let stamp = at
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let message = super::store::append_body(
        connection,
        channel,
        &super::store::gateway_author(),
        (
            None,
            &MessageBody::Notice {
                kind: ChannelNoticeKind::WakeBudgetSpent,
                member: None,
                member_id: None,
                code: None,
                resets_at: Some(stamp),
                text: format!(
                    "Woken agents have used this channel's budget for today: no agent is woken here until it renews at \
                     {clock}, or until the channel's owner, a lead or an administrator raises it."
                ),
            },
        ),
    )
    .await?;
    Ok(message.seq)
}

/// Starts again at 0 every channel whose budget window has ended (decided on 21 Nov 2026, C-3); the sweep runs it
/// before it sends what is due, so the wakes the spent budget held go out without a new post. Answers how many.
///
/// # Errors
/// Storage failure.
pub(crate) async fn renew(state: &State) -> Result<u64, GatewayError> {
    let renewed = sqlx::query!(
        "UPDATE channels SET spent_tokens = 0, budget_window_start = NULL WHERE budget_window_start IS NOT NULL \
         AND budget_window_start + make_interval(secs => $1) <= CURRENT_TIMESTAMP",
        state.config.channels.budget_window.as_secs_f64()
    )
    .execute(&state.pool)
    .await?
    .rows_affected();
    if renewed > 0 {
        tracing::info!(renewed, "channel budget windows renewed");
    }
    Ok(renewed)
}

/// Refuses a model call from a run charged to a channel whose budget is spent: `409 channel_budget` (A5).
///
/// # Errors
/// `409 channel_budget`, a store failure.
pub(crate) async fn check(state: &State, conversation: Uuid) -> Result<(), GatewayError> {
    let default = i64::try_from(state.config.channels.wake_budget).unwrap_or(i64::MAX);
    let window = state.config.channels.budget_window.as_secs_f64();
    let spent = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        // Today's spend only: a window that ended counts 0 even before the sweep renews it (C-3).
        Ok(sqlx::query_scalar!(
            "SELECT COALESCE(c.budget, $2) > 0 AND CASE WHEN c.budget_window_start + make_interval(secs => $3) \
                    > CURRENT_TIMESTAMP THEN c.spent_tokens ELSE 0 END >= COALESCE(c.budget, $2) AS \"spent!\" \
             FROM channel_charges g JOIN channels c ON c.id = g.channel_id WHERE g.conversation_id = $1",
            conversation,
            default,
            window
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    if spent == Some(true) {
        return Err(GatewayError::Conflict("channel_budget"));
    }
    Ok(())
}

/// Whether `before` < `percent` % of `budget` <= `after`, in whole numbers.
fn crossed(before: i64, after: i64, budget: i64, percent: i64) -> bool {
    let line = i128::from(budget) * i128::from(percent);
    i128::from(before) * 100 < line && line <= i128::from(after) * 100
}
