//! Shared keys and the administrators' view of host keys (ADR-0037 §1, §9, R6, T12, Amendment 1; the plan §1.2): only
//! administrators make shared keys, for everyone or chosen people; members see the shared keys they may use. An
//! administrator sees that members' own keys exist (owner, address, when last used), never their names, and may delete
//! them but never change or use them. A shared key never takes a name one of its people already uses for their own key,
//! and that refusal names no one.
use super::api::{Checked, HostKey, Row, check, fingerprint, invalid, item_with, new_reference};
use super::uses::{self, Viewer};
use crate::{
    GatewayError, InvalidReason, SecretRef,
    admin::{keyed, require_admin, version_query},
    app::State,
    auth::SessionAuthz,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

/// Shared keys at most, and chosen people per shared key at most (the plan §1.2).
const SHARED_LIMIT: i64 = 256;
const CHOSEN_LIMIT: usize = 256;

/// Who may use a shared key.
enum Users {
    All,
    Chosen(Vec<Uuid>),
}

/// A member's own key as an administrator sees it: that it exists, never its name (least exposure).
#[derive(Serialize)]
struct PersonalRow {
    id: Uuid,
    host: String,
    port: i32,
    path_prefix: Option<String>,
    owner: Value,
    #[serde(with = "time::serde::rfc3339")]
    set_at: time::OffsetDateTime,
    last_use: Option<Value>,
    version: i64,
}

#[derive(Serialize)]
pub(crate) struct AdminHostKeys {
    items: Vec<HostKey>,
    personal: Vec<PersonalRow>,
}

#[derive(Serialize)]
pub(crate) struct SharedHostKeys {
    items: Vec<HostKey>,
}

/// The body's `users`: `"all"` or up to 256 distinct ids.
fn users(value: Option<Value>) -> Result<Users, GatewayError> {
    let refuse = || invalid("users", InvalidReason::BodyInvalid);
    match value {
        Some(Value::String(all)) if all == "all" => Ok(Users::All),
        Some(Value::Array(ids)) => {
            let mut chosen: Vec<Uuid> = ids
                .iter()
                .map(|id| {
                    id.as_str()
                        .and_then(|id| id.parse().ok())
                        .ok_or_else(refuse)
                })
                .collect::<Result<_, _>>()?;
            let listed = chosen.len();
            chosen.sort_unstable();
            chosen.dedup();
            if chosen.len() != listed || chosen.len() > CHOSEN_LIMIT {
                return Err(refuse());
            }
            Ok(Users::Chosen(chosen))
        }
        _ => Err(refuse()),
    }
}

/// Shared rows' people, as the administrators' answers show them: `"all"` or `[{id, email}]`.
async fn people(
    connection: &mut PgConnection,
    keys: &[Uuid],
) -> Result<HashMap<Uuid, Value>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT k.id, k.for_everyone, u.user_id AS \"user_id?\", p.email AS \"email?\" FROM host_keys k LEFT JOIN \
         host_key_users u ON u.key_id=k.id LEFT JOIN users p ON p.id=u.user_id WHERE k.id=ANY($1) ORDER BY k.id, p.email",
        keys
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut people: HashMap<Uuid, Value> = HashMap::new();
    for row in rows {
        if row.for_everyone {
            people.insert(row.id, json!("all"));
            continue;
        }
        let entry = people.entry(row.id).or_insert_with(|| json!([]));
        if let (Some(id), Some(list)) = (row.user_id, entry.as_array_mut()) {
            list.push(json!({"id": id, "email": row.email}));
        }
    }
    Ok(people)
}

/// A shared key as its administrator sees it.
async fn shared_item(
    state: &State,
    connection: &mut PgConnection,
    row: Row,
) -> Result<HostKey, GatewayError> {
    let people = people(connection, &[row.id]).await?;
    let last = uses::last(connection, std::slice::from_ref(&row), Viewer::AdminShared).await?;
    let last_use = last.get(&row.secret_ref).cloned();
    let users = people.get(&row.id).cloned().unwrap_or_else(|| json!([]));
    Ok(item_with(state, row, Some(users), last_use))
}

/// Refuses a shared key's name when another shared key has it (named) or one of its people has an own key of that name
/// (named by no one, Amendment 1).
async fn shared_name_free(
    connection: &mut PgConnection,
    name: &str,
    users: &Users,
    except: Option<Uuid>,
) -> Result<(), GatewayError> {
    if let Some(other) = sqlx::query_scalar!(
        "SELECT id FROM host_keys WHERE owner_id IS NULL AND name=$1 AND id IS DISTINCT FROM $2",
        name,
        except
    )
    .fetch_optional(&mut *connection)
    .await?
    {
        return Err(GatewayError::NameTaken {
            taken_by: Some((other, true)),
        });
    }
    let (everyone, chosen) = match users {
        Users::All => (true, Vec::new()),
        Users::Chosen(chosen) => (false, chosen.clone()),
    };
    let clash = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM host_keys k JOIN users u ON u.id=k.owner_id WHERE k.name=$1 AND \
         u.disabled_at IS NULL AND ($2 OR k.owner_id=ANY($3))) AS \"clash!\"",
        name,
        everyone,
        &chosen
    )
    .fetch_one(&mut *connection)
    .await?;
    if clash {
        return Err(GatewayError::NameTaken { taken_by: None });
    }
    Ok(())
}

/// Stores who may use shared key `id`; every chosen person must be a user.
async fn set_users(
    connection: &mut PgConnection,
    id: Uuid,
    users: &Users,
) -> Result<(), GatewayError> {
    sqlx::query!("DELETE FROM host_key_users WHERE key_id=$1", id)
        .execute(&mut *connection)
        .await?;
    let (everyone, chosen) = match users {
        Users::All => (true, &[][..]),
        Users::Chosen(chosen) => (false, chosen.as_slice()),
    };
    sqlx::query!(
        "UPDATE host_keys SET for_everyone=$2 WHERE id=$1",
        id,
        everyone
    )
    .execute(&mut *connection)
    .await?;
    let known = sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM users WHERE id=ANY($1)",
        chosen
    )
    .fetch_one(&mut *connection)
    .await?;
    if usize::try_from(known).ok() != Some(chosen.len()) {
        return Err(GatewayError::UnknownReference { field: "users" });
    }
    sqlx::query!(
        "INSERT INTO host_key_users (key_id, user_id) SELECT $1, unnest($2::uuid[])",
        id,
        chosen
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// `GET /api/v1/admin/host-keys` (administrator, session): shared keys with who may use them, and members' own keys as
/// that they exist.
///
/// # Errors
/// `403`, a store failure.
pub(crate) async fn admin_list(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
) -> Result<Json<AdminHostKeys>, GatewayError> {
    require_admin(&user)?;
    let answer = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let shared = sqlx::query_as!(
            Row,
            "SELECT id, owner_id, name, host, port, path_prefix, scheme, user_name, header_name, secret_ref, check_name, \
             set_at, version FROM host_keys WHERE owner_id IS NULL ORDER BY name LIMIT $1",
            SHARED_LIMIT
        )
        .fetch_all(&mut *connection)
        .await?;
        let personal = sqlx::query!(
            "SELECT k.id, k.host, k.port, k.path_prefix, k.set_at, k.version, k.secret_ref, u.id AS owner, u.email FROM \
             host_keys k JOIN users u ON u.id=k.owner_id ORDER BY u.email, k.host, k.id"
        )
        .fetch_all(&mut *connection)
        .await?;
        let people = people(&mut connection, &shared.iter().map(|row| row.id).collect::<Vec<_>>()).await?;
        let shared_uses = uses::last(&mut connection, &shared, Viewer::AdminShared).await?;
        let personal_refs: Vec<String> = personal.iter().map(|row| row.secret_ref.clone()).collect();
        let personal_uses = uses::last_by_ref(&mut connection, &personal_refs, Viewer::AdminPersonal).await?;
        let items = shared
            .into_iter()
            .map(|row| {
                let users = people.get(&row.id).cloned().unwrap_or_else(|| json!([]));
                let last_use = shared_uses.get(&row.secret_ref).cloned();
                item_with(&state, row, Some(users), last_use)
            })
            .collect();
        let personal = personal
            .into_iter()
            .map(|row| PersonalRow {
                last_use: personal_uses.get(&row.secret_ref).cloned(),
                id: row.id,
                host: row.host,
                port: row.port,
                path_prefix: row.path_prefix,
                owner: json!({"id": row.owner, "email": row.email}),
                set_at: row.set_at,
                version: row.version,
            })
            .collect();
        Ok(AdminHostKeys { items, personal })
    })
    .await?;
    Ok(Json(answer))
}

/// `GET /api/v1/host-keys/shared` (a member, session): the shared keys they may use, without who else may, and their own
/// last use of each.
///
/// # Errors
/// `403 session_required` with a personal token, a store failure.
pub(crate) async fn shared_list(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
) -> Result<Json<SharedHostKeys>, GatewayError> {
    let items = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let rows = sqlx::query_as!(
            Row,
            "SELECT id, owner_id, name, host, port, path_prefix, scheme, user_name, header_name, secret_ref, check_name, \
             set_at, version FROM host_keys k WHERE owner_id IS NULL AND (for_everyone OR EXISTS (SELECT 1 FROM \
             host_key_users u WHERE u.key_id=k.id AND u.user_id=$1)) ORDER BY name LIMIT $2",
            user.id,
            SHARED_LIMIT
        )
        .fetch_all(&mut *connection)
        .await?;
        let last = uses::last(&mut connection, &rows, Viewer::Member(user.id)).await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let last_use = last.get(&row.secret_ref).cloned();
                item_with(&state, row, None, last_use)
            })
            .collect())
    })
    .await?;
    Ok(Json(SharedHostKeys { items }))
}

/// The admin body: a checked key and who may use it.
fn admin_body(body: &[u8], creating: bool) -> Result<(Checked, Users, Option<i64>), GatewayError> {
    let (mut checked, version) = check(body, creating)?;
    let users = users(checked.users.take())?;
    Ok((checked, users, version))
}

/// `POST /api/v1/admin/host-keys` `{name, address, scheme, user_name?, value, users}` (administrator, session): adds a
/// shared key (201).
///
/// # Errors
/// `400 invalid_request` with details, `403`, `409 name_taken`, `409 name_taken_by_user`, `409 host_key_limit`, `422`
/// for a reused key, a store or secret-store failure.
pub(crate) async fn admin_add(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let (checked, users, _) = admin_body(&body, true)?;
    let reference = new_reference()?;
    let value = checked
        .value
        .clone()
        .ok_or(GatewayError::SecretUnavailable)?;
    let request = json!({"name": checked.name, "host": checked.address.host, "port": checked.address.port,
        "path_prefix": checked.address.path_prefix, "scheme": checked.scheme, "user_name": checked.user_name,
        "users": users_json(&users), "value": fingerprint(&state, Some(&value))?});
    let keyed = keyed(
        &headers,
        user.id,
        "POST /api/v1/admin/host-keys".to_owned(),
        &request,
    )?;
    let (status, answer) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        crate::settings::lock(&mut transaction).await?;
        let held = sqlx::query_scalar!("SELECT count(*) AS \"n!\" FROM host_keys WHERE owner_id IS NULL")
            .fetch_one(&mut *transaction)
            .await?;
        if held >= SHARED_LIMIT {
            return Err(GatewayError::Conflict("host_key_limit"));
        }
        shared_name_free(&mut transaction, &checked.name, &users, None).await?;
        let id = Uuid::now_v7();
        sqlx::query!(
            "INSERT INTO host_keys (id, name, host, port, path_prefix, scheme, user_name, secret_ref) VALUES \
             ($1,$2,$3,$4,$5,$6,$7,$8)",
            id,
            checked.name,
            checked.address.host,
            i32::from(checked.address.port),
            checked.address.path_prefix,
            checked.scheme,
            checked.user_name,
            reference.as_str()
        )
        .execute(&mut *transaction)
        .await?;
        set_users(&mut transaction, id, &users).await?;
        state.secret_store.put(&reference, value).await?;
        crate::settings::audit_user(&mut transaction, user.id, "host_keys", &id.to_string(), "create").await?;
        let row = row(&mut transaction, id).await?;
        let answer = serde_json::to_value(shared_item(&state, &mut transaction, row).await?)?;
        if let Some(keyed) = &keyed {
            keyed.record(&mut transaction, StatusCode::CREATED, &answer).await?;
        }
        transaction.commit().await?;
        Ok((StatusCode::CREATED, answer))
    })
    .await?;
    Ok((status, Json(answer)).into_response())
}

fn users_json(users: &Users) -> Value {
    match users {
        Users::All => json!("all"),
        Users::Chosen(chosen) => json!(chosen),
    }
}

/// Any key `id`, locked for a change.
async fn row(connection: &mut PgConnection, id: Uuid) -> Result<Row, GatewayError> {
    sqlx::query_as!(
        Row,
        "SELECT id, owner_id, name, host, port, path_prefix, scheme, user_name, header_name, secret_ref, check_name, \
         set_at, version FROM host_keys WHERE id=$1 FOR UPDATE",
        id
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)
}

/// `PUT /api/v1/admin/host-keys/{id}` `{version, name, address, scheme, user_name?, value?, users}` (administrator,
/// session): a shared key, whole, at the version read. A member's own key is `409 personal_key`: an administrator may
/// delete it, never change it.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, `409 personal_key`, `409 name_taken`, `409
/// name_taken_by_user`, `422` for a reused key, a store or secret-store failure.
pub(crate) async fn admin_put(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let (checked, users, version) = admin_body(&body, false)?;
    let version = version.ok_or(invalid("version", InvalidReason::VersionInvalid))?;
    let request = json!({"id": id, "version": version, "name": checked.name, "host": checked.address.host,
        "port": checked.address.port, "path_prefix": checked.address.path_prefix, "scheme": checked.scheme,
        "user_name": checked.user_name, "users": users_json(&users), "value": fingerprint(&state, checked.value.as_ref())?});
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/admin/host-keys/{id}"),
        &request,
    )?;
    let (answer, forgotten) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some((_, cached)) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok((cached, None));
        }
        crate::settings::lock(&mut transaction).await?;
        let current = row(&mut transaction, id).await?;
        if current.owner_id.is_some() {
            return Err(GatewayError::Conflict("personal_key"));
        }
        if current.version != version {
            return Err(GatewayError::Conflict("conflict"));
        }
        shared_name_free(&mut transaction, &checked.name, &users, Some(id)).await?;
        let replacing = checked.value.is_some();
        sqlx::query!(
            "UPDATE host_keys SET name=$2, host=$3, port=$4, path_prefix=$5, scheme=$6, user_name=$7, check_name=false, \
             set_at=CASE WHEN $8 THEN CURRENT_TIMESTAMP ELSE set_at END, updated_at=CURRENT_TIMESTAMP, version=version+1 \
             WHERE id=$1",
            id,
            checked.name,
            checked.address.host,
            i32::from(checked.address.port),
            checked.address.path_prefix,
            checked.scheme,
            checked.user_name,
            replacing
        )
        .execute(&mut *transaction)
        .await?;
        set_users(&mut transaction, id, &users).await?;
        if let Some(value) = checked.value {
            let reference: SecretRef = current.secret_ref.parse().map_err(|_| GatewayError::SecretUnavailable)?;
            state.secret_store.put(&reference, value).await?;
        }
        crate::settings::audit_user(&mut transaction, user.id, "host_keys", &id.to_string(), "replace").await?;
        let row = row(&mut transaction, id).await?;
        let answer = serde_json::to_value(shared_item(&state, &mut transaction, row).await?)?;
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

/// `DELETE /api/v1/admin/host-keys/{id}?version=` (administrator, session): a shared key, or a member's own key (R6: an
/// administrator may delete it), and its sealed value.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, `422` for a reused key, a store failure.
pub(crate) async fn admin_remove(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<Uuid>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<StatusCode, GatewayError> {
    require_admin(&user)?;
    let version = version_query(query)?;
    let request = json!({"id": id, "version": version});
    let keyed = keyed(
        &headers,
        user.id,
        format!("DELETE /api/v1/admin/host-keys/{id}"),
        &request,
    )?;
    let removed = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && keyed.begin(&state, &mut transaction).await?.is_some()
        {
            return Ok(None);
        }
        let current = row(&mut transaction, id).await?;
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
                .record(&mut transaction, StatusCode::NO_CONTENT, &Value::Null)
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

/// `GET /api/v1/host-keys/{id}/reads?limit&before` (a member, session): the uses of one of their own keys, or their own
/// uses of a shared key they may use; never whose (ADR-0037 §8).
///
/// # Errors
/// `400 invalid_request` for the paging, `404` for a key that is neither, a store failure.
pub(crate) async fn reads(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<Uuid>,
    RawQuery(query): RawQuery,
) -> Result<Json<crate::secrets::reads::ReadPage>, GatewayError> {
    let paging = crate::secrets::reads::paging(query.as_deref())?;
    let key = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT secret_ref, owner_id FROM host_keys k WHERE id=$1 AND (owner_id=$2 OR (owner_id IS NULL AND \
             (for_everyone OR EXISTS (SELECT 1 FROM host_key_users u WHERE u.key_id=k.id AND u.user_id=$2))))",
            id,
            user.id
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?
    .ok_or(GatewayError::NotFound)?;
    let reference: SecretRef = key
        .secret_ref
        .parse()
        .map_err(|_| GatewayError::SecretUnavailable)?;
    // Of a shared key, only the member's own uses.
    let only = key.owner_id.is_none().then_some(user.id);
    Ok(Json(
        crate::secrets::reads::page_of(&state, &reference, &paging, false, only).await?,
    ))
}

/// `GET /api/v1/admin/host-keys/{id}/reads?limit&before` (administrator, session): every use of a key, refusals
/// included, with whose (ADR-0037 §8).
///
/// # Errors
/// `400 invalid_request` for the paging, `403`, `404`, a store failure.
pub(crate) async fn admin_reads(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<Uuid>,
    RawQuery(query): RawQuery,
) -> Result<Json<crate::secrets::reads::ReadPage>, GatewayError> {
    require_admin(&user)?;
    let paging = crate::secrets::reads::paging(query.as_deref())?;
    let secret = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(
            sqlx::query_scalar!("SELECT secret_ref FROM host_keys WHERE id=$1", id)
                .fetch_optional(&state.pool)
                .await?,
        )
    })
    .await?
    .ok_or(GatewayError::NotFound)?;
    let reference: SecretRef = secret
        .parse()
        .map_err(|_| GatewayError::SecretUnavailable)?;
    Ok(Json(
        crate::secrets::reads::page_of(&state, &reference, &paging, true, None).await?,
    ))
}
