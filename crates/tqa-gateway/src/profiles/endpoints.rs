//! `/api/v1/profiles` (ADR-0012, GW-I-025, SEC-028): a user's personal profiles and the install's shared ones. Anyone
//! signed in manages their own; only administrators create or change shared ones, and a personal profile is invisible
//! to everyone else, administrators included. Writes are versioned, take an `Idempotency-Key`, and a shared profile's
//! writes are audited without values.
use std::sync::Arc;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{
    document::{Profile, ProfileInput, validate},
    store::{self, Record},
};
use crate::{
    GatewayError, InvalidReason,
    admin::{keyed, require_admin, version_query},
    app::State,
    auth::{Authz, User},
    settings::{Change, audit, tombstones},
};

#[derive(Serialize)]
pub(crate) struct Listing {
    items: Vec<Value>,
}

fn body_invalid() -> GatewayError {
    GatewayError::Invalid {
        field: "body",
        reason: InvalidReason::BodyInvalid,
    }
}

/// A write's body: the whole request (the retry fingerprint), the profile, and the `shared` and `version` fields.
struct Write {
    request: Value,
    profile: Profile,
    shared: Option<bool>,
    version: Option<i64>,
}

fn parse(bytes: &[u8]) -> Result<Write, GatewayError> {
    let request: Value = serde_json::from_slice(bytes).map_err(|_| body_invalid())?;
    let mut fields: Map<String, Value> = request.as_object().cloned().ok_or_else(body_invalid)?;
    let shared = fields
        .remove("shared")
        .map(|value| value.as_bool().ok_or_else(body_invalid))
        .transpose()?;
    let version = fields
        .remove("version")
        .map(|value| {
            value
                .as_i64()
                .filter(|version| *version >= 1)
                .ok_or(GatewayError::Invalid {
                    field: "version",
                    reason: InvalidReason::VersionInvalid,
                })
        })
        .transpose()?;
    let input: ProfileInput =
        serde_json::from_value(Value::Object(fields)).map_err(|_| body_invalid())?;
    let profile =
        validate(input).map_err(|(field, reason)| GatewayError::Invalid { field, reason })?;
    Ok(Write {
        request,
        profile,
        shared,
        version,
    })
}

/// Every name the profile uses must exist in the install now (ADR-0012 decision 1).
fn references(state: &State, profile: &Profile) -> Result<(), GatewayError> {
    let snapshot = state.settings.current();
    let references = super::References::of(profile);
    super::unknown(&snapshot, state.config.mcp.servers(), &references).map_or(Ok(()), |field| {
        Err(GatewayError::UnknownReference { field })
    })
}

/// A stored profile as the API shows it, usable or not against the install's names now (GW-I-086).
fn shown(
    state: &State,
    snapshot: &crate::settings::Snapshot,
    record: &Record,
) -> Result<Value, GatewayError> {
    let unusable = super::unusable(snapshot, state.config.mcp.servers(), &record.references()?);
    record.to_json(unusable)
}

fn id(raw: &str) -> Result<Uuid, GatewayError> {
    Uuid::parse_str(raw).map_err(|_| GatewayError::NotFound)
}

/// A shared profile is changed by administrators only.
fn may_change(user: &User, record: &Record) -> Result<(), GatewayError> {
    if record.owner_id.is_none() {
        require_admin(user)?;
    }
    Ok(())
}

/// Audits a shared profile's write; a personal one is its owner's data, not the install's settings.
async fn audited(
    connection: &mut sqlx::PgConnection,
    user: &User,
    record: Option<&Record>,
    (action, before, after): (&'static str, Option<i64>, Option<i64>),
) -> Result<(), GatewayError> {
    let Some(record) = record.filter(|record| record.owner_id.is_none()) else {
        return Ok(());
    };
    let name = record.document["name"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    audit(
        connection,
        &Change {
            area: "profiles",
            actor: Some(user.id),
            item: &name,
            action,
            before,
            after,
            value_changed: true,
        },
    )
    .await
}

/// `GET /api/v1/profiles`: the caller's own profiles, then the shared ones.
///
/// # Errors
/// Storage failures.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Listing>, GatewayError> {
    let records = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        store::visible(&mut connection, user.id).await
    })
    .await?;
    let snapshot = state.settings.current();
    let items = records
        .iter()
        .map(|record| shown(&state, &snapshot, record))
        .collect::<Result<_, _>>()?;
    Ok(Json(Listing { items }))
}

/// `GET /api/v1/profiles/{id}`: one of the caller's own profiles, or a shared one.
///
/// # Errors
/// `404` for anyone else's; storage failures.
pub(crate) async fn get(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(raw): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    let id = id(&raw)?;
    let record = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let record = store::find(&mut transaction, user.id, id).await?;
        transaction.commit().await?;
        Ok(record)
    })
    .await?;
    Ok(Json(shown(&state, &state.settings.current(), &record)?))
}

/// `POST /api/v1/profiles`: a personal profile, or with `shared: true` (administrators) a shared one.
///
/// # Errors
/// `400` with details, `403` for a member's shared profile, `409 name_taken`, `422 unknown_reference`, `422` for a
/// reused key, storage failures.
pub(crate) async fn create(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, GatewayError> {
    let write = parse(&bytes)?;
    let shared = write.shared.unwrap_or(false);
    if shared {
        require_admin(&user)?;
    }
    if write.version.is_some() {
        return Err(body_invalid());
    }
    references(&state, &write.profile)?;
    let keyed = keyed(
        &headers,
        user.id,
        "POST /api/v1/profiles".to_owned(),
        &write.request,
    )?;
    let (status, response) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        let (owner, set_by) = if shared {
            (None, "admin")
        } else {
            (Some(user.id), "owner")
        };
        store::room(&mut transaction, owner).await?;
        // A shared profile re-created after a seeded one was deleted takes its seed digest over, so an unchanged seed
        // does not overwrite it (ADR-0028 tombstones).
        let seed = if shared {
            tombstones::unearth(&mut transaction, "profiles", &write.profile.name).await?
        } else {
            None
        };
        let record = store::insert(
            &mut transaction,
            owner,
            &write.profile,
            (set_by, seed.as_deref()),
        )
        .await?;
        audited(
            &mut transaction,
            &user,
            Some(&record),
            ("create", None, Some(record.version)),
        )
        .await?;
        let response = record.to_json(None)?;
        if let Some(keyed) = &keyed {
            keyed
                .record(&mut transaction, StatusCode::CREATED, &response)
                .await?;
        }
        transaction.commit().await?;
        Ok((StatusCode::CREATED, response))
    })
    .await?;
    Ok((status, Json(response)).into_response())
}

/// `PUT /api/v1/profiles/{id}`: replaces a profile at the version read; sharing never changes.
///
/// # Errors
/// As [`create`], plus `404` for anyone else's personal profile and `409 conflict` for another version.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(raw): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, GatewayError> {
    let id = id(&raw)?;
    let write = parse(&bytes)?;
    let (Some(expected), None) = (write.version, write.shared) else {
        return Err(body_invalid());
    };
    references(&state, &write.profile)?;
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/profiles/{id}"),
        &write.request,
    )?;
    let response = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some((_, cached)) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        let current = store::find(&mut transaction, user.id, id).await?;
        may_change(&user, &current)?;
        let set_by = if current.owner_id.is_none() {
            "admin"
        } else {
            "owner"
        };
        let record = store::replace(
            &mut transaction,
            &current,
            expected,
            (&write.profile, set_by),
        )
        .await?;
        audited(
            &mut transaction,
            &user,
            Some(&record),
            ("replace", Some(current.version), Some(record.version)),
        )
        .await?;
        let response = record.to_json(None)?;
        if let Some(keyed) = &keyed {
            keyed
                .record(&mut transaction, StatusCode::OK, &response)
                .await?;
        }
        transaction.commit().await?;
        Ok(response)
    })
    .await?;
    Ok(Json(response).into_response())
}

/// `DELETE /api/v1/profiles/{id}?version=`: conversations made from it keep their copy.
///
/// # Errors
/// `404`, `403` for a member on a shared profile, `409 conflict`, storage failures.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(raw): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<StatusCode, GatewayError> {
    let id = id(&raw)?;
    let expected = version_query(query)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let current = store::find(&mut transaction, user.id, id).await?;
        may_change(&user, &current)?;
        let seed = store::delete(&mut transaction, &current, expected).await?;
        if current.owner_id.is_none() {
            let name = current.document["name"].as_str().unwrap_or_default();
            tombstones::bury(&mut transaction, "profiles", name, seed.as_deref()).await?;
        }
        audited(
            &mut transaction,
            &user,
            Some(&current),
            ("delete", Some(current.version), None),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
