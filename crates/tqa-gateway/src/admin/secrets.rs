//! `/api/v1/admin/secrets` (090, ADR-0028): shared secrets by name. Values are write-only: never listed, echoed,
//! logged or hashed in plain. Writes are versioned, need a signed-in session, accept an Idempotency-Key and are audited.
//! Users' own credentials (`git-` and `connector-` names) are never listed or accepted here.
use super::{keyed, require_admin, version_query};
use crate::{
    GatewayError, InvalidReason, SecretEntry, SecretRef, SecretWrite,
    app::State,
    auth::{Authz, SessionAuthz},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Longest secret name, as the reference grammar allows.
const NAME_BYTES: usize = 120;

#[derive(Serialize)]
pub(crate) struct Item {
    #[serde(rename = "ref")]
    reference: String,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: time::OffsetDateTime,
    version: i64,
    seed: Option<String>,
    last_set_by: String,
    routes: Vec<String>,
    /// When the value last changed.
    #[serde(with = "time::serde::rfc3339")]
    rotated_at: time::OffsetDateTime,
    /// The rotation reminder's interval in days, or `null` for none.
    rotate_after_days: Option<i32>,
    /// Whether the reminder's interval has passed since the value last changed.
    rotation_due: bool,
    /// The routes and connectors that may use it; a new secret allows none.
    consumers: Vec<crate::secrets::bindings::Consumer>,
}

#[derive(Serialize)]
pub(crate) struct Items {
    items: Vec<Item>,
    limit: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    value: String,
    version: Option<i64>,
}

fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}

fn reference(name: &str) -> Result<SecretRef, GatewayError> {
    let valid = (1..=NAME_BYTES).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !valid {
        return Err(invalid("name", InvalidReason::NameInvalid));
    }
    if is_reserved(name) {
        return Err(invalid("name", InvalidReason::ReservedPrefix));
    }
    format!("secrets/{name}")
        .parse()
        .map_err(|_| invalid("name", InvalidReason::NameInvalid))
}

/// Names reserved for users' own credentials (Git and connectors), compared without case: an administrator can
/// neither see, write nor delete them (SEC-031).
fn is_reserved(name: &str) -> bool {
    tqa_types::is_per_user_secret(name)
}

fn shared(entry: &SecretEntry) -> bool {
    entry
        .reference
        .as_str()
        .strip_prefix("secrets/")
        .is_some_and(|name| !is_reserved(name))
}

/// The routes whose `key_ref` names this reference, sorted.
fn routes_using(state: &State, reference: &SecretRef) -> Vec<String> {
    let settings = state.settings.current();
    settings
        .registry
        .names()
        .filter(|name| {
            settings
                .registry
                .get(name)
                .and_then(|route| route.key_ref.as_deref())
                == Some(reference.as_str())
        })
        .map(str::to_owned)
        .collect()
}

fn item(state: &State, entry: SecretEntry) -> Item {
    Item {
        rotation_due: crate::secrets::rotation::due(
            entry.rotated_at,
            entry.rotate_after_days,
            time::OffsetDateTime::now_utc(),
        ),
        rotated_at: entry.rotated_at,
        rotate_after_days: entry.rotate_after_days,
        consumers: crate::secrets::bindings::of(
            &state.settings.current().bindings,
            entry.reference.as_str(),
        ),
        routes: routes_using(state, &entry.reference),
        reference: entry.reference.as_str().to_owned(),
        updated_at: entry.updated_at,
        version: entry.version,
        seed: entry.seed_state,
        last_set_by: entry.last_set_by,
    }
}

/// `GET /api/v1/admin/secrets` (administrator, session or token): shared references with metadata, never values.
///
/// # Errors
/// Not an administrator, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Items>, GatewayError> {
    require_admin(&user)?;
    let entries = state.secret_store.entries().await?;
    let items = entries
        .into_iter()
        .filter(shared)
        .map(|entry| item(&state, entry))
        .collect();
    Ok(Json(Items {
        items,
        limit: state.config.secret_store.references,
    }))
}

/// The request's value, checked against the store's bounds without echoing it.
fn value(body: &Body, state: &State) -> Result<SecretString, GatewayError> {
    if body.value.is_empty() {
        return Err(invalid("value", InvalidReason::ValueEmpty));
    }
    if body.value.len() > state.config.secret_store.value_bytes {
        return Err(invalid("value", InvalidReason::ValueTooLong));
    }
    if body.value.contains('\0') {
        return Err(invalid("value", InvalidReason::ValueNul));
    }
    if body.version.is_some_and(|version| version < 1) {
        return Err(invalid("version", InvalidReason::VersionInvalid));
    }
    Ok(SecretString::from(body.value.clone()))
}

/// `PUT /api/v1/admin/secrets/{name}` `{value, version?}` (administrator, session): creates without a version (201)
/// or replaces the version read (200).
///
/// # Errors
/// `400 invalid_request` with details, `403`, `409 conflict`, `409 secret_limit`, `422` for a reused key, store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let reference = reference(&name)?;
    let body: Body =
        serde_json::from_slice(&body).map_err(|_| invalid("body", InvalidReason::BodyInvalid))?;
    // A key pasted into a new shared key's name (SEC-033) would be listed everywhere the key is. Only a create is
    // refused: one stored before this check can still be given a value or deleted, so it can be replaced.
    if body.version.is_none() && tqa_types::is_key_shaped_name(&name) {
        return Err(invalid("name", InvalidReason::NameKeyShaped));
    }
    let value = value(&body, &state)?;
    let digest = state.secret_store.fingerprint(&reference, &value)?;
    let request = serde_json::json!({"ref": reference.as_str(), "version": body.version, "value": crate::hex::hex(&digest)});
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/admin/secrets/{name}"),
        &request,
    )?;
    let write = SecretWrite {
        reference,
        value,
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
        let (created, entry) = state.secret_store.write(&mut transaction, write).await?;
        let status = if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        let response = serde_json::to_value(item(&state, entry))?;
        if let Some(keyed) = &keyed {
            keyed.record(&mut transaction, status, &response).await?;
        }
        transaction.commit().await?;
        Ok((status, response))
    })
    .await?;
    Ok((status, Json(response)).into_response())
}

/// The reminder request `{rotate_after_days: 1..=3650 | null, version}`, checked field by field.
fn reminder(body: &[u8]) -> Result<(Option<i32>, i64), GatewayError> {
    let body: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(body).map_err(|_| invalid("body", InvalidReason::BodyInvalid))?;
    if body
        .keys()
        .any(|key| key != "rotate_after_days" && key != "version")
    {
        return Err(invalid("body", InvalidReason::BodyInvalid));
    }
    let days = match body.get("rotate_after_days") {
        None => return Err(invalid("body", InvalidReason::BodyInvalid)),
        Some(serde_json::Value::Null) => None,
        Some(value) => Some(
            value
                .as_i64()
                .filter(|days| (1..=3650).contains(days))
                .and_then(|days| i32::try_from(days).ok())
                .ok_or(invalid(
                    "rotate_after_days",
                    InvalidReason::RotateAfterInvalid,
                ))?,
        ),
    };
    let version = body
        .get("version")
        .and_then(serde_json::Value::as_i64)
        .filter(|version| *version >= 1)
        .ok_or(invalid("version", InvalidReason::VersionInvalid))?;
    Ok((days, version))
}

/// `PUT /api/v1/admin/secrets/{name}/reminder` `{rotate_after_days, version}` (administrator, session): sets or clears
/// the rotation reminder of the version read. A reminder never stops a secret from working.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, store failure.
pub(crate) async fn remind(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(name): Path<String>,
    body: Bytes,
) -> Result<Json<Item>, GatewayError> {
    require_admin(&user)?;
    let reference = reference(&name)?;
    let (days, version) = reminder(&body)?;
    let entry = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let entry = state
            .secret_store
            .remind(&mut transaction, &reference, days, (version, user.id))
            .await?;
        transaction.commit().await?;
        Ok(entry)
    })
    .await?;
    tracing::info!(
        secret_ref = crate::secrets::loggable(reference.as_str()),
        rotate_after_days = days,
        "secret rotation reminder set"
    );
    Ok(Json(item(&state, entry)))
}

/// `GET /api/v1/admin/secrets/{name}/reads?limit&before` (administrator, session or token): which consumers read a
/// shared secret, for which conversation, user and run, newest first; never its value. Users' own
/// credentials are not shown here.
///
/// # Errors
/// `400 invalid_request` with details, `403`, store failure.
pub(crate) async fn reads(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Json<crate::secrets::reads::ReadPage>, GatewayError> {
    require_admin(&user)?;
    let reference = reference(&name)?;
    let paging = crate::secrets::reads::paging(query.as_deref())?;
    Ok(Json(
        crate::secrets::reads::page(&state, &reference, &paging, true).await?,
    ))
}

/// `PUT /api/v1/admin/secrets/{name}/consumers` `{consumers: [{kind: route|connector, name}], version}`
/// (administrator, session): the routes and connectors that may use a shared secret. Versioned and
/// audited; a route or connector naming a secret that is not bound to it cannot be saved, and cannot read it.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, store failure.
pub(crate) async fn bind(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(name): Path<String>,
    body: Bytes,
) -> Result<Json<Item>, GatewayError> {
    require_admin(&user)?;
    let reference = reference(&name)?;
    let (consumers, version) = crate::secrets::bindings::parse(&body)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        crate::settings::lock(&mut transaction).await?;
        crate::secrets::bindings::replace(
            &mut transaction,
            reference.as_str(),
            &consumers,
            (version, user.id),
        )
        .await?;
        crate::settings::bump(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    state
        .settings
        .refresh(&state.pool, state.config.io_timeout(), &state.cancel)
        .await?;
    tracing::info!(
        secret_ref = crate::secrets::loggable(reference.as_str()),
        consumers = consumers.len(),
        "secret consumers set"
    );
    let entry = state
        .secret_store
        .entries()
        .await?
        .into_iter()
        .find(|entry| entry.reference == reference)
        .ok_or(GatewayError::NotFound)?;
    Ok(Json(item(&state, entry)))
}

/// `DELETE /api/v1/admin/secrets/{name}?version=` (administrator, session).
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, `409 secret_in_use`, store failure.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<StatusCode, GatewayError> {
    require_admin(&user)?;
    let reference = reference(&name)?;
    let version = version_query(query)?;
    if !routes_using(&state, &reference).is_empty() {
        return Err(GatewayError::Conflict("secret_in_use"));
    }
    let request = serde_json::json!({"ref": reference.as_str(), "version": version});
    let keyed = keyed(
        &headers,
        user.id,
        format!("DELETE /api/v1/admin/secrets/{name}"),
        &request,
    )?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && keyed.begin(&state, &mut transaction).await?.is_some()
        {
            return Ok(());
        }
        state
            .secret_store
            .remove(&mut transaction, &reference, version, user.id)
            .await?;
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
    Ok(StatusCode::NO_CONTENT)
}
