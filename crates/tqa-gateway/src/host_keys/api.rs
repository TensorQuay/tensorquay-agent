//! `/api/v1/host-keys` (ADR-0037 §1, §9; the host-keys plan §1.2; GW-I-161): a member's own keys. Metadata only in every
//! answer; a value is written to the sealed store and never read back. Writes need a signed-in session, accept an
//! Idempotency-Key, are versioned and audited.
use super::rules::{self, Address};
use crate::{
    GatewayError, InvalidReason, SecretRef,
    admin::{keyed, version_query},
    app::State,
    auth::SessionAuthz,
    egress::Decision,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use rand::TryRngCore;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use std::sync::Arc;
use uuid::Uuid;

/// A member's own keys at most (the plan §1.2).
pub(super) const OWN_LIMIT: i64 = 64;
/// The longest value: a header's worth.
const VALUE_BYTES: usize = 8 * 1024;

/// A key as its owner sees it (the plan §1.1): never a value.
#[derive(Serialize)]
pub(crate) struct HostKey {
    id: Uuid,
    name: String,
    host: String,
    port: i32,
    path_prefix: Option<String>,
    scheme: String,
    user_name: Option<String>,
    header_name: Option<String>,
    shared: bool,
    users: Option<serde_json::Value>,
    #[serde(with = "time::serde::rfc3339")]
    set_at: time::OffsetDateTime,
    last_use: Option<serde_json::Value>,
    allowed: bool,
    check_name: bool,
    version: i64,
}

#[derive(Serialize)]
pub(crate) struct HostKeys {
    items: Vec<HostKey>,
}

/// A stored row, as the queries read it.
pub(super) struct Row {
    pub(super) id: Uuid,
    pub(super) owner_id: Option<Uuid>,
    pub(super) name: String,
    pub(super) host: String,
    pub(super) port: i32,
    pub(super) path_prefix: Option<String>,
    pub(super) scheme: String,
    pub(super) user_name: Option<String>,
    pub(super) header_name: Option<String>,
    pub(super) secret_ref: String,
    pub(super) check_name: bool,
    pub(super) set_at: time::OffsetDateTime,
    pub(super) version: i64,
}

/// The body of an add or a change: the whole item, and a value only when one is set.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    version: Option<i64>,
    /// Shared keys only (the administrators' endpoints): "all" or the chosen people's ids.
    users: Option<serde_json::Value>,
    name: String,
    address: String,
    scheme: String,
    user_name: Option<String>,
    header_name: Option<String>,
    value: Option<String>,
}

/// A checked body: its address parsed, its value bounded.
pub(super) struct Checked {
    pub(super) name: String,
    pub(super) address: Address,
    pub(super) scheme: &'static str,
    pub(super) user_name: Option<String>,
    pub(super) value: Option<SecretString>,
    /// The body's `users`, unchecked: the administrators' endpoints read it, the members' refuse it.
    pub(super) users: Option<serde_json::Value>,
}

pub(super) const fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}

/// The request as a checked body, field by field, in the contract's order (the plan §1.2).
pub(super) fn check(body: &[u8], creating: bool) -> Result<(Checked, Option<i64>), GatewayError> {
    let body: Body =
        serde_json::from_slice(body).map_err(|_| invalid("body", InvalidReason::BodyInvalid))?;
    let address =
        rules::address(&body.address).map_err(|(field, reason)| invalid(field, reason))?;
    rules::name(&body.name).map_err(|reason| invalid("name", reason))?;
    let scheme = match body.scheme.as_str() {
        "bearer" => "bearer",
        "basic" => "basic",
        // The `header` scheme comes with the proxy's second slice (ADR-0037 Consequences).
        "header" => return Err(invalid("scheme", InvalidReason::SchemeNotSupported)),
        _ => return Err(invalid("scheme", InvalidReason::BodyInvalid)),
    };
    if body.header_name.is_some() {
        return Err(invalid("header_name", InvalidReason::BodyInvalid));
    }
    let user_name = match (scheme, body.user_name) {
        ("basic", Some(user))
            if (1..=256).contains(&user.len())
                && user
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && byte != b':') =>
        {
            Some(user)
        }
        (_, None) => None,
        _ => return Err(invalid("user_name", InvalidReason::BodyInvalid)),
    };
    let value = match body.value {
        None if creating => return Err(invalid("body", InvalidReason::BodyInvalid)),
        None => None,
        Some(value) => Some(checked_value(value)?),
    };
    if !creating && body.version.is_none_or(|version| version < 1) {
        return Err(invalid("version", InvalidReason::VersionInvalid));
    }
    if creating && body.version.is_some() {
        return Err(invalid("body", InvalidReason::BodyInvalid));
    }
    Ok((
        Checked {
            name: body.name,
            address,
            scheme,
            user_name,
            value,
            users: body.users,
        },
        body.version,
    ))
}

/// A value within the store's bounds that a header can carry.
fn checked_value(value: String) -> Result<SecretString, GatewayError> {
    if value.is_empty() {
        return Err(invalid("value", InvalidReason::ValueEmpty));
    }
    if value.len() > VALUE_BYTES {
        return Err(invalid("value", InvalidReason::ValueTooLong));
    }
    if value.contains('\0') {
        return Err(invalid("value", InvalidReason::ValueNul));
    }
    if value.contains(['\n', '\r']) {
        return Err(invalid("value", InvalidReason::ValueInvalid));
    }
    Ok(SecretString::from(value))
}

/// A new sealed reference, naming neither the owner nor the host.
pub(super) fn new_reference() -> Result<SecretRef, GatewayError> {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| GatewayError::SecretUnavailable)?;
    format!("secrets/host-{}", crate::hex::hex(&bytes))
        .parse()
        .map_err(|_| GatewayError::SecretUnavailable)
}

/// The keyed digest of a value for a request's Idempotency-Key fingerprint; never the value or a plain hash of it.
pub(super) fn fingerprint(
    state: &State,
    value: Option<&SecretString>,
) -> Result<Option<String>, GatewayError> {
    let reference: SecretRef = "secrets/host-request"
        .parse()
        .map_err(|_| GatewayError::SecretUnavailable)?;
    value
        .map(|value| {
            state
                .secret_store
                .fingerprint(&reference, value)
                .map(|digest| crate::hex::hex(&digest))
        })
        .transpose()
}

/// Whether the installation's network list allows the key's host and port today.
fn allowed(state: &State, host: &str, port: i32) -> bool {
    u16::try_from(port).is_ok_and(|port| {
        matches!(
            state.settings.current().egress.decide(host, port),
            Decision::Allowed { .. }
        )
    })
}

/// A key as its viewer sees it, with who may use it (administrators' answers only) and the viewer's last use.
pub(super) fn item_with(
    state: &State,
    row: Row,
    users: Option<serde_json::Value>,
    last_use: Option<serde_json::Value>,
) -> HostKey {
    HostKey {
        allowed: allowed(state, &row.host, row.port),
        shared: row.owner_id.is_none(),
        users,
        last_use,
        id: row.id,
        name: row.name,
        host: row.host,
        port: row.port,
        path_prefix: row.path_prefix,
        scheme: row.scheme,
        user_name: row.user_name,
        header_name: row.header_name,
        set_at: row.set_at,
        check_name: row.check_name,
        version: row.version,
    }
}

fn item(state: &State, row: Row) -> HostKey {
    item_with(state, row, None, None)
}

/// The key that already has `name` among `user`'s own keys (other than `except`) and the shared keys they may use.
pub(super) async fn name_holder(
    connection: &mut PgConnection,
    user: Uuid,
    name: &str,
    except: Option<Uuid>,
) -> Result<Option<(Uuid, bool)>, GatewayError> {
    let holder = sqlx::query!(
        "SELECT id, owner_id FROM host_keys k WHERE name=$2 AND id IS DISTINCT FROM $3 AND (owner_id=$1 OR (owner_id IS \
         NULL AND (for_everyone OR EXISTS (SELECT 1 FROM host_key_users u WHERE u.key_id=k.id AND u.user_id=$1)))) \
         ORDER BY owner_id NULLS LAST LIMIT 1",
        user,
        name,
        except
    )
    .fetch_optional(&mut *connection)
    .await?;
    Ok(holder.map(|row| (row.id, row.owner_id.is_none())))
}

/// `GET /api/v1/host-keys` (a member, session): their own keys, by name.
///
/// # Errors
/// `403 session_required` with a personal token, a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
) -> Result<Json<HostKeys>, GatewayError> {
    let (rows, recent) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let rows = sqlx::query_as!(
            Row,
            "SELECT id, owner_id, name, host, port, path_prefix, scheme, user_name, header_name, secret_ref, check_name, \
             set_at, version FROM host_keys WHERE owner_id=$1 ORDER BY name LIMIT $2",
            user.id,
            OWN_LIMIT
        )
        .fetch_all(&mut *connection)
        .await?;
        let recent = super::uses::last(&mut connection, &rows, super::uses::Viewer::Member(user.id)).await?;
        Ok((rows, recent))
    })
    .await?;
    Ok(Json(HostKeys {
        items: rows
            .into_iter()
            .map(|row| {
                let last_use = recent.get(&row.secret_ref).cloned();
                item_with(&state, row, None, last_use)
            })
            .collect(),
    }))
}

/// `POST /api/v1/host-keys` `{name, address, scheme, user_name?, value}` (a member, session): adds one of their own
/// keys (201).
///
/// # Errors
/// `400 invalid_request` with details, `409 name_taken`, `409 host_key_limit`, `422` for a reused key, a store or
/// secret-store failure.
pub(crate) async fn add(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let (checked, _) = check(&body, true)?;
    if checked.users.is_some() {
        return Err(invalid("users", InvalidReason::BodyInvalid));
    }
    let reference = new_reference()?;
    let value = checked
        .value
        .clone()
        .ok_or(GatewayError::SecretUnavailable)?;
    let request = serde_json::json!({"name": checked.name, "host": checked.address.host, "port": checked.address.port,
        "path_prefix": checked.address.path_prefix, "scheme": checked.scheme, "user_name": checked.user_name,
        "value": fingerprint(&state, Some(&value))?});
    let keyed = keyed(
        &headers,
        user.id,
        "POST /api/v1/host-keys".to_owned(),
        &request,
    )?;
    let (status, answer) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        // One member's key changes one at a time, so the count and the names hold.
        sqlx::query!("SELECT id FROM users WHERE id=$1 FOR UPDATE", user.id)
            .fetch_one(&mut *transaction)
            .await?;
        let held = sqlx::query_scalar!("SELECT count(*) AS \"n!\" FROM host_keys WHERE owner_id=$1", user.id)
            .fetch_one(&mut *transaction)
            .await?;
        if held >= OWN_LIMIT {
            return Err(GatewayError::Conflict("host_key_limit"));
        }
        if let Some(holder) = name_holder(&mut transaction, user.id, &checked.name, None).await? {
            return Err(GatewayError::NameTaken { taken_by: Some(holder) });
        }
        let row = sqlx::query_as!(
            Row,
            "INSERT INTO host_keys (id, owner_id, name, host, port, path_prefix, scheme, user_name, secret_ref) VALUES \
             ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id, owner_id, name, host, port, path_prefix, scheme, user_name, \
             header_name, secret_ref, check_name, set_at, version",
            Uuid::now_v7(),
            user.id,
            checked.name,
            checked.address.host,
            i32::from(checked.address.port),
            checked.address.path_prefix,
            checked.scheme,
            checked.user_name,
            reference.as_str()
        )
        .fetch_one(&mut *transaction)
        .await?;
        // The value is stored before the row commits: a committed key always has its value.
        state.secret_store.put(&reference, value).await?;
        crate::settings::audit_user(&mut transaction, user.id, "host_keys", &row.id.to_string(), "create").await?;
        let answer = serde_json::to_value(item(&state, row))?;
        if let Some(keyed) = &keyed {
            keyed.record(&mut transaction, StatusCode::CREATED, &answer).await?;
        }
        transaction.commit().await?;
        Ok((StatusCode::CREATED, answer))
    })
    .await?;
    tracing::info!(user_id = %user.id, "host key added");
    Ok((status, Json(answer)).into_response())
}

/// The caller's own key `id`, locked for a change.
async fn own(connection: &mut PgConnection, user: Uuid, id: Uuid) -> Result<Row, GatewayError> {
    sqlx::query_as!(
        Row,
        "SELECT id, owner_id, name, host, port, path_prefix, scheme, user_name, header_name, secret_ref, check_name, \
         set_at, version FROM host_keys WHERE id=$1 AND owner_id=$2 FOR UPDATE",
        id,
        user
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)
}

/// `PUT /api/v1/host-keys/{id}` `{version, name, address, scheme, user_name?, value?}` (the key's owner, session): the
/// whole item at the version read; the value only when it is replaced. Any successful save clears `check_name`.
///
/// # Errors
/// `400 invalid_request` with details, `404`, `409 conflict`, `409 name_taken`, `422` for a reused key, a store or
/// secret-store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let (checked, version) = check(&body, false)?;
    if checked.users.is_some() {
        return Err(invalid("users", InvalidReason::BodyInvalid));
    }
    let version = version.ok_or(invalid("version", InvalidReason::VersionInvalid))?;
    let request = serde_json::json!({"id": id, "version": version, "name": checked.name, "host": checked.address.host,
        "port": checked.address.port, "path_prefix": checked.address.path_prefix, "scheme": checked.scheme,
        "user_name": checked.user_name, "value": fingerprint(&state, checked.value.as_ref())?});
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/host-keys/{id}"),
        &request,
    )?;
    let (answer, forgotten) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some((_, cached)) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok((cached, None));
        }
        sqlx::query!("SELECT id FROM users WHERE id=$1 FOR UPDATE", user.id)
            .fetch_one(&mut *transaction)
            .await?;
        let current = own(&mut transaction, user.id, id).await?;
        if current.version != version {
            return Err(GatewayError::Conflict("conflict"));
        }
        if let Some(holder) = name_holder(&mut transaction, user.id, &checked.name, Some(id)).await? {
            return Err(GatewayError::NameTaken { taken_by: Some(holder) });
        }
        let replacing = checked.value.is_some();
        let row = sqlx::query_as!(
            Row,
            "UPDATE host_keys SET name=$3, host=$4, port=$5, path_prefix=$6, scheme=$7, user_name=$8, check_name=false, \
             set_at=CASE WHEN $9 THEN CURRENT_TIMESTAMP ELSE set_at END, updated_at=CURRENT_TIMESTAMP, version=version+1 \
             WHERE id=$1 AND owner_id=$2 RETURNING id, owner_id, name, host, port, path_prefix, scheme, user_name, \
             header_name, secret_ref, check_name, set_at, version",
            id,
            user.id,
            checked.name,
            checked.address.host,
            i32::from(checked.address.port),
            checked.address.path_prefix,
            checked.scheme,
            checked.user_name,
            replacing
        )
        .fetch_one(&mut *transaction)
        .await?;
        if let Some(value) = checked.value {
            let reference: SecretRef = row.secret_ref.parse().map_err(|_| GatewayError::SecretUnavailable)?;
            state.secret_store.put(&reference, value).await?;
        }
        crate::settings::audit_user(&mut transaction, user.id, "host_keys", &id.to_string(), "replace").await?;
        let answer = serde_json::to_value(item(&state, row))?;
        if let Some(keyed) = &keyed {
            keyed.record(&mut transaction, StatusCode::OK, &answer).await?;
        }
        transaction.commit().await?;
        Ok((answer, Some((current.host, current.port))))
    })
    .await?;
    state.host_keys.forget(forgotten.as_ref());
    Ok(Json(answer).into_response())
}

/// `DELETE /api/v1/host-keys/{id}?version=` (the key's owner, session): the key and its sealed value.
///
/// # Errors
/// `400 invalid_request` with details, `404`, `409 conflict`, `422` for a reused key, a store or secret-store failure.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<Uuid>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<StatusCode, GatewayError> {
    let version = version_query(query)?;
    let request = serde_json::json!({"id": id, "version": version});
    let keyed = keyed(
        &headers,
        user.id,
        format!("DELETE /api/v1/host-keys/{id}"),
        &request,
    )?;
    let removed = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && keyed.begin(&state, &mut transaction).await?.is_some()
        {
            return Ok(None);
        }
        let current = own(&mut transaction, user.id, id).await?;
        if current.version != version {
            return Err(GatewayError::Conflict("conflict"));
        }
        sqlx::query!("DELETE FROM host_keys WHERE id=$1", id)
            .execute(&mut *transaction)
            .await?;
        crate::settings::audit_user(
            &mut transaction,
            user.id,
            "host_keys",
            &id.to_string(),
            "delete",
        )
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
        Ok(Some((current.secret_ref, (current.host, current.port))))
    })
    .await?;
    state
        .host_keys
        .forget(removed.as_ref().map(|(_, address)| address));
    let reference = removed.map(|(reference, _)| reference);
    // The value goes once the key is gone; a failure here leaves an orphan the store's sweep may remove, never a key
    // without its value.
    if let Some(reference) = reference.and_then(|reference| reference.parse::<SecretRef>().ok())
        && let Err(error) = state.secret_store.delete(&reference).await
    {
        tracing::warn!(
            code = error.code(),
            "a deleted host key's value could not be removed from the store"
        );
    }
    Ok(StatusCode::NO_CONTENT)
}
