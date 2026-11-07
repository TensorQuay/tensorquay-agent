//! User administration (task 063, GW-I-046): list, change role, disable or enable. Users are never deleted, and
//! the last enabled administrator can be neither demoted nor disabled. Session only (answers-063 Q3).
use super::{Role, SessionAuthz};
use crate::{GatewayError, app::State, request::ApiJson};
use axum::{
    Json,
    extract::{Path, State as AppState},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Users listed at once.
const MAX_LISTED: i64 = 1000;

#[derive(Serialize)]
pub(crate) struct Account {
    id: uuid::Uuid,
    email: String,
    role: String,
    disabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    /// The last authenticated request, to the minute; null if the account never made one.
    #[serde(with = "time::serde::rfc3339::option")]
    last_active_at: Option<time::OffsetDateTime>,
}

#[derive(Serialize)]
pub(crate) struct Accounts {
    items: Vec<Account>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
    role: Option<Role>,
    disabled: Option<bool>,
}

fn admin(role: Role) -> Result<(), GatewayError> {
    if role == Role::Admin {
        Ok(())
    } else {
        Err(GatewayError::Forbidden("administrator role is required"))
    }
}

/// `GET /api/v1/users` (administrator, session): every account in creation order; never a password hash.
///
/// # Errors
/// Not an administrator, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
) -> Result<Json<Accounts>, GatewayError> {
    admin(user.role)?;
    let items = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_as!(Account, "SELECT id, email::text AS \"email!\", role, disabled_at IS NOT NULL AS \"disabled!\", created_at, last_active_at FROM users ORDER BY created_at, id LIMIT $1", MAX_LISTED)
            .fetch_all(&state.pool).await?)
    }).await?;
    Ok(Json(Accounts { items }))
}

/// `PATCH /api/v1/users/{id}` (administrator, session) `{role?, disabled?}`. Disabling deletes the user's sessions
/// and revokes their tokens in the same transaction, so every credential stops at once.
///
/// # Errors
/// Not an administrator, an empty change, `404`, `409 last_admin`, or a store failure.
pub(crate) async fn change(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<Change>,
) -> Result<Json<Account>, GatewayError> {
    admin(user.role)?;
    let id = super::path_id(&id)?;
    if body.role.is_none() && body.disabled.is_none() {
        return Err(GatewayError::BadRequest(
            "a role or a disabled flag is required",
        ));
    }
    let role = body.role.map(|role| match role {
        Role::Admin => "admin",
        Role::Member => "member",
    });
    let (account, stopping) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        // Lock every enabled administrator: concurrent demotions cannot both pass the last-admin check.
        let admins = sqlx::query_scalar!("SELECT id FROM users WHERE role='admin' AND disabled_at IS NULL ORDER BY id FOR UPDATE")
            .fetch_all(&mut *transaction).await?;
        let target = sqlx::query!("SELECT role, disabled_at IS NOT NULL AS \"disabled!\" FROM users WHERE id=$1 FOR UPDATE", id)
            .fetch_optional(&mut *transaction).await?.ok_or(GatewayError::NotFound)?;
        let enabled_admin = target.role == "admin" && !target.disabled;
        let loses_admin = role == Some("member") || body.disabled == Some(true);
        if enabled_admin && loses_admin && admins.len() <= 1 {
            return Err(GatewayError::Conflict("last_admin"));
        }
        let account = sqlx::query_as!(Account,
            "UPDATE users SET role=COALESCE($2,role), disabled_at=CASE WHEN $3::boolean IS NULL THEN disabled_at WHEN $3 THEN COALESCE(disabled_at,CURRENT_TIMESTAMP) ELSE NULL END, updated_at=CURRENT_TIMESTAMP WHERE id=$1 RETURNING id, email::text AS \"email!\", role, disabled_at IS NOT NULL AS \"disabled!\", created_at, last_active_at",
            id, role, body.disabled).fetch_one(&mut *transaction).await?;
        let mut stopping = Vec::new();
        if body.disabled == Some(true) {
            sqlx::query!("DELETE FROM sessions WHERE user_id=$1", id).execute(&mut *transaction).await?;
            sqlx::query!("UPDATE api_tokens SET revoked_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE user_id=$1 AND revoked_at IS NULL", id)
                .execute(&mut *transaction).await?;
            // Their agent teams stop too, through the one stop path (ADR-0034 point 10).
            stopping = crate::teams::stop::person_disabled(&mut transaction, id).await?;
        }
        transaction.commit().await?;
        Ok((account, stopping))
    }).await?;
    for team in stopping {
        if let Err(error) = crate::teams::stop::finish(&state, team).await {
            tracing::warn!(team = %team, code = error.code(), "a disabled person's team is left to the reconciler to end");
        }
    }
    tracing::info!(user_id = %user.id, target_id = %id, role = ?role, disabled = ?body.disabled, "account changed");
    Ok(Json(account))
}
