//! `/api/v1/admin/agent-teams` (ADR-0034, the slice S6 contract): the two agent-team settings that aren't numbers, a
//! helper's model route (for a helper started without a profile) and the acceptance check run on the lead's `done`.
//! One versioned row (migration 0078); administrators only, audited under `agent_teams`; `403 teams_disabled` while
//! agent teams are off.
use super::{keyed, require_admin};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::Authz,
    settings::{Change, audit},
    teams::people::enabled,
};
use axum::{
    Json,
    body::Bytes,
    extract::State as AppState,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use std::sync::Arc;

/// The settings as stored and answered.
#[derive(Debug, Serialize)]
struct Settings {
    member_route: Option<String>,
    acceptance_check: Option<String>,
    version: i64,
}

/// A write: both settings and the version read.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Write {
    member_route: Option<String>,
    acceptance_check: Option<String>,
    version: i64,
}

/// `GET /api/v1/admin/agent-teams`.
///
/// # Errors
/// `403` for anyone but an administrator, `403 teams_disabled`, store failure.
pub(crate) async fn get(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    enabled(&state)?;
    let settings = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        read(&mut connection, false).await
    })
    .await?;
    Ok(Json(settings).into_response())
}

/// `PUT /api/v1/admin/agent-teams` `{member_route, acceptance_check, version}`: both settings, with the version read.
///
/// # Errors
/// `400 invalid_request` naming the field (an acceptance check that is empty, longer than 4,096 bytes or more than
/// one line; a malformed body), `403`, `403 teams_disabled`, `409 conflict` for a version that is no longer current,
/// `422 unknown_reference` (`member_route`) for a route the install doesn't have, `422` for a reused key, store
/// failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    enabled(&state)?;
    let write: Write =
        serde_json::from_slice(&body).map_err(|_| invalid("body", InvalidReason::BodyInvalid))?;
    if write.version < 0 {
        return Err(invalid("version", InvalidReason::VersionInvalid));
    }
    if let Some(reason) = write
        .acceptance_check
        .as_deref()
        .and_then(super::agent_teams_rules::refused_check)
    {
        return Err(invalid("acceptance_check", reason));
    }
    if let Some(route) = &write.member_route
        && state.settings.current().registry.get(route).is_none()
    {
        return Err(GatewayError::UnknownReference {
            field: "member_route",
        });
    }
    let request = serde_json::to_value(&write)?;
    let keyed = keyed(
        &headers,
        user.id,
        "PUT /api/v1/admin/agent-teams".to_owned(),
        &request,
    )?;
    let answer = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some((_, cached)) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        let before = read(&mut transaction, true).await?;
        if before.version != write.version {
            return Err(GatewayError::Conflict("conflict"));
        }
        let changed = before.member_route != write.member_route
            || before.acceptance_check != write.acceptance_check;
        let version = sqlx::query_scalar!(
            "UPDATE agent_team_settings SET member_route=$1, acceptance_check=$2, version=version+1, \
             updated_at=CURRENT_TIMESTAMP WHERE id=1 RETURNING version",
            write.member_route,
            write.acceptance_check
        )
        .fetch_one(&mut *transaction)
        .await?;
        audit(
            &mut transaction,
            &Change {
                area: "agent_teams",
                actor: Some(user.id),
                item: "settings",
                action: "replace",
                before: Some(before.version),
                after: Some(version),
                value_changed: changed,
            },
        )
        .await?;
        let answer = serde_json::to_value(Settings {
            member_route: write.member_route.clone(),
            acceptance_check: write.acceptance_check.clone(),
            version,
        })?;
        if let Some(keyed) = &keyed {
            keyed
                .record(&mut transaction, StatusCode::OK, &answer)
                .await?;
        }
        transaction.commit().await?;
        Ok(answer)
    })
    .await?;
    Ok((StatusCode::OK, Json(answer)).into_response())
}

/// The route a helper started without a profile uses, as the administrator set it, read fresh: `None` when unset (the
/// file's `[agent_teams] member_route`, else the lead's own route, applies).
///
/// # Errors
/// Storage failure.
pub(crate) async fn member_route(
    connection: &mut PgConnection,
) -> Result<Option<String>, GatewayError> {
    Ok(read(connection, false).await?.member_route)
}

/// The acceptance check the lead's `done` runs (GW-I-198), read fresh: `None` when none is set.
///
/// # Errors
/// Storage failure.
pub(crate) async fn acceptance_check(
    connection: &mut PgConnection,
) -> Result<Option<String>, GatewayError> {
    Ok(read(connection, false).await?.acceptance_check)
}

/// The stored row (the migration writes it), locked for a write when `lock`.
async fn read(connection: &mut PgConnection, lock: bool) -> Result<Settings, GatewayError> {
    let row = if lock {
        sqlx::query_as!(
            Settings,
            "SELECT member_route, acceptance_check, version FROM agent_team_settings WHERE id=1 FOR UPDATE"
        )
        .fetch_optional(&mut *connection)
        .await?
    } else {
        sqlx::query_as!(
            Settings,
            "SELECT member_route, acceptance_check, version FROM agent_team_settings WHERE id=1"
        )
        .fetch_optional(&mut *connection)
        .await?
    };
    Ok(row.unwrap_or(Settings {
        member_route: None,
        acceptance_check: None,
        version: 0,
    }))
}

fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}
