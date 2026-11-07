//! `/api/v1/admin/routes` (ADR-0028): model routes by name. A route is the whole `models.toml` record plus an
//! editable display name; the name is immutable, because conversations store it. Writes pass the file's own
//! validator over the whole resulting registry, are versioned, accept a session or an administrator's API token and
//! an Idempotency-Key, are audited, and apply from the next request. Credentials never appear: `key_ref` names a
//! secret, and `key_missing` says whether it has a value.
use super::{
    keyed, require_admin,
    route_input::{body, name},
    version_query,
};
use crate::{
    GatewayError,
    app::State,
    auth::Authz,
    secrets::SecretRef,
    settings::{
        self,
        routes::{RouteWrite, StoredRoute},
    },
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

#[derive(Serialize)]
pub(crate) struct Listing {
    items: Vec<Value>,
    /// The settings generation the list was read at.
    generation: i64,
}

/// One route as the API shows it: the full record, its metadata, which routes list it as a fallback, and its latest
/// probe (`probe`, from [`probes`]).
fn item(
    stored: &StoredRoute,
    fallback_of: &[String],
    refs: &[SecretRef],
    probe: &Value,
) -> Result<Value, GatewayError> {
    let key_missing = settings::routes::key_missing_in(&stored.config, refs);
    let key_ref_invalid = settings::routes::key_ref_invalid(&stored.config);
    let Value::Object(mut fields) = serde_json::to_value(&stored.config)? else {
        return Err(GatewayError::Io);
    };
    // Only a secret reference is ever shown: anything else may be a pasted key (SEC-033).
    let reference = stored
        .config
        .key_ref
        .as_deref()
        .filter(|reference| tqa_types::is_secret_reference(reference));
    fields.insert("key_ref".into(), serde_json::json!(reference));
    let updated_at = stored
        .updated_at
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| GatewayError::Io)?;
    let extra = serde_json::json!({
        "fallbacks": stored.config.fallbacks,
        // The effective tool-schema settings, defaults included.
        "strict": stored.config.strict.as_deref().unwrap_or("auto"),
        "schema_keywords": stored.config.schema_keywords.as_deref().unwrap_or("full"),
        // Usage in the stream: on unless the route turns it off.
        "stream_usage": stored.config.stream_usage.unwrap_or(true),
        "display_name": stored.display_name,
        "description": stored.description,
        "key_missing": key_missing,
        "key_ref_invalid": key_ref_invalid,
        "version": stored.version,
        "seed": stored.seed_state,
        "last_set_by": stored.last_set_by,
        "updated_at": updated_at,
        "fallback_of": fallback_of,
        "probe": probe,
    });
    if let Value::Object(extra) = extra {
        fields.extend(extra);
    }
    Ok(Value::Object(fields))
}

/// Each probed route's latest probe as the list shows it: verified or failed, or stale once the route's document is no
/// longer the one the probe tested (GW-I-140). A route missing here was never probed.
async fn probes(
    connection: &mut sqlx::PgConnection,
) -> Result<std::collections::HashMap<String, Value>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT p.route, p.ok, p.probed_at, p.observed, \
         p.document_digest = sha256(convert_to(r.document::text, 'UTF8')) AS \"current!\" \
         FROM route_probes p JOIN model_routes r ON r.name = p.route"
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let state = match (row.current, row.ok) {
                (false, _) => "stale",
                (true, true) => "verified",
                (true, false) => "failed",
            };
            let probed_at = row
                .probed_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| GatewayError::Io)?;
            Ok((
                row.route,
                serde_json::json!({"state": state, "probed_at": probed_at, "observed": row.observed}),
            ))
        })
        .collect()
}

/// A route that was never probed.
fn never() -> Value {
    serde_json::json!({"state": "never", "probed_at": null, "observed": null})
}

/// The routes that list `name` as a fallback, sorted.
fn fallback_of(name: &str, all: &[StoredRoute]) -> Vec<String> {
    all.iter()
        .filter(|stored| {
            stored
                .config
                .fallbacks
                .iter()
                .any(|fallback| fallback == name)
        })
        .map(|stored| stored.config.name.clone())
        .collect()
}

/// `GET /api/v1/admin/routes` (administrator, session or token): every stored route, by name.
///
/// # Errors
/// Not an administrator, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Listing>, GatewayError> {
    require_admin(&user)?;
    let (all, generation, mut probed) =
        crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            let mut transaction = state.pool.begin().await?;
            let all = settings::routes::entries(&mut transaction).await?;
            let generation = settings::generation(&mut transaction).await?;
            let probed = probes(&mut transaction).await?;
            transaction.commit().await?;
            Ok((all, generation, probed))
        })
        .await?;
    let refs = state.secret_store.refs().await?;
    let items = all
        .iter()
        .map(|stored| {
            let probe = probed.remove(&stored.config.name).unwrap_or_else(never);
            item(
                stored,
                &fallback_of(&stored.config.name, &all),
                &refs,
                &probe,
            )
        })
        .collect::<Result<_, _>>()?;
    Ok(Json(Listing { items, generation }))
}

/// `PUT /api/v1/admin/routes/{name}` (administrator, session or token): creates without a version (201) or replaces
/// the version read (200). The whole resulting registry must pass the file's validator.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `409 conflict`, `422` for a reused key, store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(route): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    name(&route)?;
    let super::route_input::RouteBody {
        config,
        display_name,
        description,
        expected,
    } = body(&route, &bytes)?;
    super::refuse_key_shaped("key_ref", config.key_ref.as_deref())?;
    let request = serde_json::json!({"route": config, "display_name": display_name,
        "description": description, "version": expected});
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/admin/routes/{route}"),
        &request,
    )?;
    let write = RouteWrite {
        config,
        display_name,
        description,
        expected,
        actor: user.id,
    };
    let refs = state.secret_store.refs().await?;
    let (status, response) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        settings::lock(&mut transaction).await?;
        let (created, stored) = settings::routes::write(&mut transaction, write).await?;
        settings::bump(&mut transaction).await?;
        let all = settings::routes::entries(&mut transaction).await?;
        let status = if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        let probe = probes(&mut transaction)
            .await?
            .remove(&route)
            .unwrap_or_else(never);
        let response = item(&stored, &fallback_of(&route, &all), &refs, &probe)?;
        if let Some(keyed) = &keyed {
            keyed.record(&mut transaction, status, &response).await?;
        }
        transaction.commit().await?;
        Ok((status, response))
    })
    .await?;
    refresh(&state).await?;
    // Open tasks on this route, or falling back to it, use it from their next message (F12).
    crate::conversations::route_changed(&state, &route).await;
    Ok((status, Json(response)).into_response())
}

/// `DELETE /api/v1/admin/routes/{name}?version=` (administrator, session or token). Conversations keep the name, and
/// their next request fails plainly (ADR-0028).
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, `409 route_is_fallback`, store failure.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(route): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<StatusCode, GatewayError> {
    require_admin(&user)?;
    name(&route)?;
    let version = version_query(query)?;
    let request = serde_json::json!({"name": route, "version": version});
    let keyed = keyed(
        &headers,
        user.id,
        format!("DELETE /api/v1/admin/routes/{route}"),
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
        settings::routes::remove(&mut transaction, &route, version, user.id).await?;
        settings::bump(&mut transaction).await?;
        if let Some(keyed) = &keyed {
            keyed
                .record(&mut transaction, StatusCode::NO_CONTENT, &Value::Null)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    refresh(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/admin/routes/export` (administrator, session or token): the live routes as `models.toml`, which is
/// the seed and import format. Display names are not part of that format.
///
/// # Errors
/// Not an administrator, or a route TOML cannot represent.
pub(crate) async fn export(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let text = state
        .settings
        .current()
        .registry
        .to_toml()
        .map_err(|_| GatewayError::Io)?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/toml; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"models.toml\""),
            ),
        ],
        text,
    )
        .into_response())
}

/// Installs the committed change before answering, so the next request already sees it; a replayed write refreshes
/// too, completing a first attempt that committed but failed to refresh.
async fn refresh(state: &State) -> Result<(), GatewayError> {
    state
        .settings
        .refresh(&state.pool, state.config.io_timeout(), &state.cancel)
        .await
}
