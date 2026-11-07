//! `/api/v1/admin/limits` (ADR-0028 slice 5): the overridable admission and sandbox limits. Every limit always has a
//! value (seeded from the configuration file), so a write names the version read, and `DELETE` resets a limit to the
//! file's value. A change applies from the next admission pass or the next sandbox created; a running sandbox keeps its
//! limits. Writes are versioned, accept a session or an administrator's API token and an Idempotency-Key, and are
//! audited.
use super::{keyed, require_admin, version_query};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::Authz,
    settings::{
        self,
        limit_rows::{LimitWrite, StoredLimit},
        limits::{LimitKey, Limits, admin_bounds, parse_value},
    },
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Serialize)]
pub(crate) struct Item {
    key: &'static str,
    value: u64,
    /// The configuration file's value (or its built-in default): what a reset restores.
    default: u64,
    /// The bounds an administrator's write must meet, so the page can bound its input.
    min: u64,
    max: u64,
    unit: &'static str,
    applies: &'static str,
    version: i64,
    seed: Option<String>,
    last_set_by: String,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: time::OffsetDateTime,
    /// The value the load moved into the file's bounds ("lowered to fit the configuration file"), until the next write.
    clamped_from: Option<u64>,
    /// The value is below the floor an administrator may set: the file's explicit choice.
    below_floor: bool,
}

#[derive(Serialize)]
pub(crate) struct Listing {
    items: Vec<Item>,
    generation: i64,
    /// The installation's default environment size and its ceiling (file-only), shown read-only.
    defaults: Value,
    ceiling: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    value: Value,
    version: i64,
}

fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}

fn key(name: &str) -> Result<LimitKey, GatewayError> {
    LimitKey::parse(name).ok_or_else(|| invalid("key", InvalidReason::LimitUnknown))
}

/// A key an administrator may write now: the agent teams' limits only while agent teams are on (ADR-0034 S6).
fn writable(state: &State, name: &str) -> Result<LimitKey, GatewayError> {
    let key = key(name)?;
    if key.is_team() && !state.config.agent_teams.enabled {
        return Err(GatewayError::Denied("teams_disabled"));
    }
    Ok(key)
}

fn item(stored: StoredLimit, effective: &Limits, file: &Limits, max_live: u32) -> Item {
    let bounds = admin_bounds(stored.key, effective, max_live);
    Item {
        key: stored.key.as_str(),
        value: stored.value,
        default: file.get(stored.key),
        min: bounds.min,
        max: bounds.max,
        unit: stored.key.unit(),
        applies: stored.key.applies(),
        below_floor: stored.value < bounds.min,
        version: stored.version,
        seed: stored.seed_state,
        last_set_by: stored.last_set_by,
        updated_at: stored.updated_at,
        clamped_from: stored.clamped_from,
    }
}

/// `GET /api/v1/admin/limits` (administrator, session or token): every overridable limit, in a fixed order.
///
/// # Errors
/// Not an administrator, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Listing>, GatewayError> {
    require_admin(&user)?;
    let (rows, effective, generation) =
        crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            let mut transaction = state.pool.begin().await?;
            let rows = settings::limit_rows::rows(&mut transaction).await?;
            let effective = settings::limit_rows::effective(&mut transaction).await?;
            let generation = settings::generation(&mut transaction).await?;
            transaction.commit().await?;
            Ok((rows, effective, generation))
        })
        .await?;
    let file = settings::file_limits(&state.config);
    let max_live = state.config.sandbox.max_live;
    // The agent teams' limits leave no trace while agent teams are off (ADR-0034 S6).
    let teams = state.config.agent_teams.enabled;
    let items = rows
        .into_iter()
        .filter(|stored| teams || !stored.key.is_team())
        .map(|stored| item(stored, &effective, &file, max_live))
        .collect();
    let mut sizes = crate::config::sizes::limits(&state.config);
    Ok(Json(Listing {
        items,
        generation,
        defaults: sizes["defaults"].take(),
        ceiling: sizes["ceiling"].take(),
    }))
}

/// `PUT /api/v1/admin/limits/{key}` `{value, version}` (administrator, session or token): sets a limit after checking
/// the whole resulting set against the bounds, the floors and the order.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, `422` for a reused key, store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let key = writable(&state, &name)?;
    let body: Body =
        serde_json::from_slice(&body).map_err(|_| invalid("body", InvalidReason::BodyInvalid))?;
    let value =
        parse_value(&body.value).ok_or_else(|| invalid("value", InvalidReason::LimitOutOfRange))?;
    if body.version < 1 {
        return Err(invalid("version", InvalidReason::VersionInvalid));
    }
    let request = serde_json::json!({"key": key.as_str(), "value": value, "version": body.version});
    let path = format!("PUT /api/v1/admin/limits/{}", key.as_str());
    let keyed = keyed(&headers, user.id, path, &request)?;
    let response = change(
        &state,
        keyed.as_ref(),
        LimitWrite {
            key,
            value,
            expected: body.version,
            actor: user.id,
            max_live: state.config.sandbox.max_live,
            reset: false,
        },
    )
    .await?;
    Ok((StatusCode::OK, Json(response)).into_response())
}

/// `DELETE /api/v1/admin/limits/{key}?version=` (administrator, session or token): resets a limit to the configuration
/// file's value and returns it (a limit always has a value, so nothing is deleted).
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, `422` for a reused key, store failure.
pub(crate) async fn reset(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let key = writable(&state, &name)?;
    let version = version_query(query)?;
    let value = settings::file_limits(&state.config).get(key);
    let request = serde_json::json!({"key": key.as_str(), "reset": true, "version": version});
    let path = format!("DELETE /api/v1/admin/limits/{}", key.as_str());
    let keyed = keyed(&headers, user.id, path, &request)?;
    let response = change(
        &state,
        keyed.as_ref(),
        LimitWrite {
            key,
            value,
            expected: version,
            actor: user.id,
            max_live: state.config.sandbox.max_live,
            reset: true,
        },
    )
    .await?;
    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Applies one write in a settings transaction (replaying a retried one), then installs the new snapshot before
/// answering, so the next admission or sandbox sees it.
async fn change(
    state: &State,
    keyed: Option<&crate::conversations::idempotent::Keyed<'_>>,
    write: LimitWrite,
) -> Result<Value, GatewayError> {
    let file = settings::file_limits(&state.config);
    let max_live = write.max_live;
    let response = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = keyed
            && let Some((_, cached)) = keyed.begin(state, &mut transaction).await?
        {
            return Ok(cached);
        }
        settings::lock(&mut transaction).await?;
        let stored = settings::limit_rows::write(&mut transaction, write).await?;
        settings::bump(&mut transaction).await?;
        let effective = settings::limit_rows::effective(&mut transaction).await?;
        let response = serde_json::to_value(item(stored, &effective, &file, max_live))?;
        if let Some(keyed) = keyed {
            keyed
                .record(&mut transaction, StatusCode::OK, &response)
                .await?;
        }
        transaction.commit().await?;
        Ok(response)
    })
    .await?;
    state
        .settings
        .refresh(&state.pool, state.config.io_timeout(), &state.cancel)
        .await?;
    Ok(response)
}
