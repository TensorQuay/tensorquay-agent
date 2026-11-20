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
use std::sync::Arc;
use tqa_types::{ChannelNoticeKind, ChannelRole};

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
            (ChannelNoticeKind::WakeBudgetSet, Some(user.email.clone())),
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
