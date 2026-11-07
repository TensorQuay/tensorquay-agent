//! `/api/v1/admin/egress` (ADR-0028): the egress allow-list, one rule per host pattern. Rules pass the validator the
//! file uses and are stored canonically. Writes are versioned, accept a session or an administrator's API token and an
//! Idempotency-Key, are audited, and apply from the next new connection of every sandbox; an open tunnel keeps its own
//! bounds. With `[egress] disabled = true` the list is shown but cannot change, since no rule would apply.
use super::{keyed, require_admin, version_query};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::Authz,
    egress::{RuleError, RuleInput, canonical, canonical_host},
    settings::{
        self,
        egress::{MAX_RULES, RuleWrite, StoredRule},
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
use std::sync::Arc;

#[derive(Serialize)]
pub(crate) struct Item {
    host: String,
    ports: Vec<u16>,
    /// The administrator lets this exact host name reach a private address (ADR-0035 amendment).
    internal: bool,
    /// For an internal rule that can't be used right now, why: `internal_never_missing` (name this machine's own
    /// addresses in `[egress] internal_never`) or `engine_unknown` (the container engine's networks can't be read).
    #[serde(skip_serializing_if = "Option::is_none")]
    internal_state: Option<&'static str>,
    version: i64,
    seed: Option<String>,
    /// The internet-access preset that added it, until an administrator edits it.
    preset: Option<String>,
    last_set_by: String,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: time::OffsetDateTime,
}

impl From<StoredRule> for Item {
    fn from(rule: StoredRule) -> Self {
        Self {
            host: rule.host,
            ports: rule.ports,
            internal: rule.internal,
            internal_state: None,
            version: rule.version,
            seed: rule.seed_state,
            preset: rule.preset,
            last_set_by: rule.last_set_by,
            updated_at: rule.updated_at,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Listing {
    /// Egress is off by configuration: sandboxes are internal-only and these rules do not apply.
    disabled: bool,
    items: Vec<Item>,
    limit: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    ports: Vec<u16>,
    /// Off unless set; only for one exact host name.
    #[serde(default)]
    internal: bool,
    version: Option<i64>,
}

fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}

pub(super) fn writable(state: &State) -> Result<(), GatewayError> {
    if state.config.egress.disabled {
        Err(GatewayError::Conflict("egress_disabled"))
    } else {
        Ok(())
    }
}

/// `GET /api/v1/admin/egress` (administrator, session or token): every rule, by host.
///
/// # Errors
/// Not an administrator, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Listing>, GatewayError> {
    require_admin(&user)?;
    let rules = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        settings::egress::rules(&mut connection).await
    })
    .await?;
    let unusable = state.internal.state(std::time::Instant::now());
    let items = rules
        .into_iter()
        .map(|rule| {
            let internal = rule.internal;
            let mut item = Item::from(rule);
            item.internal_state = unusable.filter(|_| internal);
            item
        })
        .collect();
    Ok(Json(Listing {
        disabled: state.config.egress.disabled,
        items,
        limit: MAX_RULES,
    }))
}

/// `PUT /api/v1/admin/egress/{host}` `{ports, version?}` (administrator, session or token): creates the rule for a
/// host pattern without a version (201) or replaces the version read (200). The host is stored in canonical form.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `409 conflict`, `409 egress_rule_limit`, `409 egress_disabled`, `422` for
/// a reused key, store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(host): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let host = canonical_host(&host).map_err(RuleError::refusal)?;
    let body: Body =
        serde_json::from_slice(&body).map_err(|_| invalid("body", InvalidReason::BodyInvalid))?;
    if body.version.is_some_and(|version| version < 1) {
        return Err(invalid("version", InvalidReason::VersionInvalid));
    }
    let rule = canonical(RuleInput {
        host,
        ports: body.ports,
        internal: body.internal,
    })
    .map_err(RuleError::refusal)?;
    writable(&state)?;
    let request =
        serde_json::json!({"host": rule.host, "ports": rule.ports, "version": body.version});
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/admin/egress/{}", rule.host),
        &request,
    )?;
    let write = RuleWrite {
        rule,
        expected: body.version,
        actor: user.id,
    };
    let (status, response) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        settings::lock(&mut transaction).await?;
        let (created, rule) = settings::egress::write(&mut transaction, write).await?;
        settings::bump(&mut transaction).await?;
        let status = if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        let response = serde_json::to_value(Item::from(rule))?;
        if let Some(keyed) = &keyed {
            keyed.record(&mut transaction, status, &response).await?;
        }
        transaction.commit().await?;
        Ok((status, response))
    })
    .await?;
    refresh(&state).await?;
    Ok((status, Json(response)).into_response())
}

/// `DELETE /api/v1/admin/egress/{host}?version=` (administrator, session or token): the next new connection to that
/// host is denied; a tunnel already open runs to its own bounds.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, `409 egress_disabled`, store failure.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(host): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<StatusCode, GatewayError> {
    require_admin(&user)?;
    let host = canonical_host(&host).map_err(RuleError::refusal)?;
    let version = version_query(query)?;
    writable(&state)?;
    let request = serde_json::json!({"host": host, "version": version});
    let keyed = keyed(
        &headers,
        user.id,
        format!("DELETE /api/v1/admin/egress/{host}"),
        &request,
    )?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && keyed.begin(&state, &mut transaction).await?.is_some()
        {
            return Ok(());
        }
        settings::lock(&mut transaction).await?;
        settings::egress::remove(&mut transaction, &host, version, user.id).await?;
        settings::bump(&mut transaction).await?;
        if let Some(keyed) = &keyed {
            keyed
                .record(
                    &mut transaction,
                    StatusCode::NO_CONTENT,
                    &serde_json::Value::Null,
                )
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    refresh(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Installs the committed change before answering, so the caller's next connection already sees it. A replayed write
/// refreshes too: if the first attempt committed but failed to refresh, the retry completes it.
pub(super) async fn refresh(state: &State) -> Result<(), GatewayError> {
    state
        .settings
        .refresh(&state.pool, state.config.io_timeout(), &state.cancel)
        .await
}
