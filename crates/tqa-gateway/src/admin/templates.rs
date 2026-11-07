//! `/api/v1/admin/templates` (ADR-0028, ADR-0032): environment templates by name. Writes pass the file's validator
//! (GW-U-041), are versioned, accept a session or an administrator's API token and an Idempotency-Key, are audited
//! without values, and apply from the next request. A write resets the template's check; the checker runs again.
use super::{keyed, require_admin, version_query};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::Authz,
    settings::{self, templates::TemplateWrite},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
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
    /// The installation's default size and its ceiling (file-only), for the environment form.
    defaults: Value,
    ceiling: Value,
}

fn body_invalid() -> GatewayError {
    GatewayError::Invalid {
        field: "body",
        reason: InvalidReason::BodyInvalid,
    }
}

/// The `[[mcp.servers]]` entries that list `name` (they keep it from being deleted).
fn mcp_servers(state: &State, name: &str) -> Vec<String> {
    state
        .config
        .mcp
        .servers()
        .iter()
        .filter(|server| server.templates().iter().any(|template| template == name))
        .map(|server| server.name().to_owned())
        .collect()
}

async fn refresh(state: &State) -> Result<(), GatewayError> {
    state
        .settings
        .refresh(&state.pool, state.config.io_timeout(), &state.cancel)
        .await
}

/// `GET /api/v1/admin/templates` (administrator, session or token): every stored template, by name.
///
/// # Errors
/// Not an administrator, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Listing>, GatewayError> {
    require_admin(&user)?;
    let (all, generation) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let all = settings::templates::entries(&mut transaction).await?;
        let generation = settings::generation(&mut transaction).await?;
        transaction.commit().await?;
        Ok((all, generation))
    })
    .await?;
    let ceiling = &state.config.sandbox.ceiling;
    let items = all
        .iter()
        .map(|template| template.to_json(ceiling))
        .collect::<Result<_, GatewayError>>()?;
    let mut limits = crate::config::sizes::limits(&state.config);
    Ok(Json(Listing {
        items,
        generation,
        defaults: limits["defaults"].take(),
        ceiling: limits["ceiling"].take(),
    }))
}

/// `PUT /api/v1/admin/templates/{name}` (administrator, session or token): creates without a version (201) or
/// replaces the version read (200).
///
/// # Errors
/// `400 invalid_request` with details, `403`, `409` (`conflict`, `name_taken`, `default_count`, `template_count`),
/// `422` for a reused key, store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let request: Value = serde_json::from_slice(&bytes).map_err(|_| body_invalid())?;
    let write: TemplateWrite =
        serde_json::from_value(request.clone()).map_err(|_| body_invalid())?;
    // Acknowledged credential files are file-only (ADR-0032), like a connector's CA file.
    if write.acknowledged_secret_files.is_some() {
        return Err(GatewayError::Invalid {
            field: "acknowledged_secret_files",
            reason: crate::InvalidReason::FileOnly,
        });
    }
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/admin/templates/{name}"),
        &request,
    )?;
    let (status, response) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        settings::lock(&mut transaction).await?;
        let (created, stored) = settings::templates::write(
            &mut transaction,
            &name,
            write,
            (user.id, &state.config.sandbox.ceiling),
        )
        .await?;
        settings::bump(&mut transaction).await?;
        let status = if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        let response = stored.to_json(&state.config.sandbox.ceiling)?;
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

/// `DELETE /api/v1/admin/templates/{name}?version=` (administrator, session or token). A seeded template stays
/// deleted until its seed entry changes (ADR-0028 tombstones).
///
/// # Errors
/// `404`, `409 conflict`, `409 template_in_use` with what uses it, store failure.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<StatusCode, GatewayError> {
    require_admin(&user)?;
    let version = version_query(query)?;
    let request = serde_json::json!({"name": name, "version": version});
    let keyed = keyed(
        &headers,
        user.id,
        format!("DELETE /api/v1/admin/templates/{name}"),
        &request,
    )?;
    let listing = mcp_servers(&state, &name);
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && keyed.begin(&state, &mut transaction).await?.is_some()
        {
            return Ok(());
        }
        settings::lock(&mut transaction).await?;
        settings::templates::remove(&mut transaction, &name, version, (user.id, listing)).await?;
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

/// `POST /api/v1/admin/templates/{name}/default` with `{"version": n}` (administrator, session or token): makes it
/// the default, taking the default from the current one atomically. Only an offered template can be the default.
///
/// # Errors
/// `400`, `404`, `409 conflict`, `409 template_unavailable`, `422` for a reused key, store failure.
pub(crate) async fn make_default(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, GatewayError> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        version: i64,
    }
    require_admin(&user)?;
    let request: Value = serde_json::from_slice(&bytes).map_err(|_| body_invalid())?;
    let body: Body = serde_json::from_value(request.clone()).map_err(|_| body_invalid())?;
    let keyed = keyed(
        &headers,
        user.id,
        format!("POST /api/v1/admin/templates/{name}/default"),
        &request,
    )?;
    let (status, response) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        settings::lock(&mut transaction).await?;
        let stored =
            settings::templates::make_default(&mut transaction, &name, body.version, user.id)
                .await?;
        settings::bump(&mut transaction).await?;
        let response = stored.to_json(&state.config.sandbox.ceiling)?;
        if let Some(keyed) = &keyed {
            keyed
                .record(&mut transaction, StatusCode::OK, &response)
                .await?;
        }
        transaction.commit().await?;
        Ok((StatusCode::OK, response))
    })
    .await?;
    refresh(&state).await?;
    Ok((status, Json(response)).into_response())
}

/// `POST /api/v1/admin/templates/{name}/check` (administrator, session or token): queues the template's checks
/// again, for example after its rootfs was built. The checker runs at once; the result arrives in the list. It takes
/// no Idempotency-Key: a repeat only queues the same check again.
///
/// # Errors
/// `404`, store failure.
pub(crate) async fn check(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
) -> Result<StatusCode, GatewayError> {
    require_admin(&user)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        settings::lock(&mut transaction).await?;
        settings::templates::requeue(&mut transaction, &name).await?;
        // The bump installs a snapshot, which wakes the checker.
        settings::bump(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    refresh(&state).await?;
    Ok(StatusCode::ACCEPTED)
}

/// `POST /api/v1/admin/templates/{name}/reset` with `{"version": n}` (administrator, session or token): hands a
/// customised built-in back to the rebuilds, on the image its canonical tag names now (the built-in environments
/// design). Audited; retried by Idempotency-Key.
///
/// # Errors
/// `400`, `404`; `409` `conflict`, `not_customised`, `template_is_default`, or `builtin_image_missing` when no usable
/// build of it is on the host; `503 engine_unavailable`; `422` for a reused key; store failure.
pub(crate) async fn reset(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, GatewayError> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        version: i64,
    }
    require_admin(&user)?;
    let request: Value = serde_json::from_slice(&bytes).map_err(|_| body_invalid())?;
    let body: Body = serde_json::from_value(request.clone()).map_err(|_| body_invalid())?;
    let keyed = keyed(
        &headers,
        user.id,
        format!("POST /api/v1/admin/templates/{name}/reset"),
        &request,
    )?;
    let (status, response) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(&state, &mut transaction).await?
        {
            return Ok(cached);
        }
        let built = current_build(&state, &name).await;
        settings::lock(&mut transaction).await?;
        let stored =
            settings::builtins::reset(&mut transaction, (&name, body.version), built, user.id)
                .await?;
        settings::bump(&mut transaction).await?;
        let response = stored.to_json(&state.config.sandbox.ceiling)?;
        if let Some(keyed) = &keyed {
            keyed
                .record(&mut transaction, StatusCode::OK, &response)
                .await?;
        }
        transaction.commit().await?;
        Ok((StatusCode::OK, response))
    })
    .await?;
    refresh(&state).await?;
    Ok((status, Json(response)).into_response())
}

/// The built-in `name` as the engine host holds it now.
async fn current_build(
    state: &State,
    name: &str,
) -> Result<crate::environments::builtin::BuiltIn, GatewayError> {
    let source = state
        .environment_images
        .get()
        .ok_or(GatewayError::EngineUnavailable)?;
    let images = source
        .list()
        .await
        .map_err(|_| GatewayError::EngineUnavailable)?;
    crate::environments::builtin::built_ins(&images, state.config.builtin_images())
        .into_iter()
        .find(|built| built.name == name)
        .ok_or(GatewayError::Conflict("builtin_image_missing"))
}
