//! `/api/v1/admin/connectors` (ADR-0031 decision 7, the ADR-0028 pattern): connectors by name. A connector is its
//! record plus an editable display name; the name is immutable and shares the MCP namespace with `[[mcp.servers]]`.
//! Writes are validated, versioned, accept a session or an administrator's API token and an Idempotency-Key, are
//! audited, and apply from the next request. Credentials never appear: `secret_ref` names a secret, and `key_missing`
//! says whether it has a value.
use super::{
    keyed, require_admin,
    route_input::{display_name, version},
    version_query,
};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::Authz,
    connectors::{ConnectorDocument, Origin, Refusal, validate},
    secrets::SecretRef,
    settings::{
        self,
        connectors::{ConnectorWrite, StoredConnector},
    },
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

/// The most connectors an install may hold (ADR-0031 decision 6; also bounds the proxy's metric label).
const MAX_CONNECTORS: usize = 64;

#[derive(Serialize)]
pub(crate) struct Listing {
    items: Vec<Value>,
    /// The settings generation the list was read at.
    generation: i64,
}

/// One connector as the API shows it: the record and its metadata.
fn item(stored: &StoredConnector, refs: &[SecretRef]) -> Result<Value, GatewayError> {
    let Value::Object(mut fields) = serde_json::to_value(&stored.document)? else {
        return Err(GatewayError::Io);
    };
    let key_missing = stored
        .document
        .secret_ref
        .as_deref()
        .is_some_and(|reference| {
            reference
                .parse::<SecretRef>()
                .map_or(true, |reference| !refs.contains(&reference))
        });
    let updated_at = stored
        .updated_at
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| GatewayError::Io)?;
    let extra = serde_json::json!({
        "display_name": stored.display_name,
        "key_missing": key_missing,
        "version": stored.version,
        "seed": stored.seed_state,
        "last_set_by": stored.last_set_by,
        "updated_at": updated_at,
    });
    if let Value::Object(extra) = extra {
        fields.extend(extra);
    }
    Ok(Value::Object(fields))
}

/// The request body: every connector field except `name`, plus `display_name?` and `version?`; validated as the seed
/// is, without the file-only fields.
fn body(
    state: &State,
    name: &str,
    bytes: &[u8],
) -> Result<(ConnectorDocument, Option<String>, Option<i64>), GatewayError> {
    let body_invalid = || GatewayError::Invalid {
        field: "body",
        reason: InvalidReason::BodyInvalid,
    };
    let Ok(Value::Object(mut fields)) = serde_json::from_slice::<Value>(bytes) else {
        return Err(body_invalid());
    };
    if fields.contains_key("name") {
        return Err(body_invalid());
    }
    let display_name = display_name(fields.remove("display_name"))?;
    let expected = version(fields.remove("version"))?;
    fields.insert("name".into(), Value::String(name.to_owned()));
    let document: ConnectorDocument =
        serde_json::from_value(Value::Object(fields)).map_err(|_| body_invalid())?;
    let stdio: Vec<String> = state
        .config
        .mcp
        .servers()
        .iter()
        .map(|server| server.name().to_owned())
        .collect();
    validate(&document, Origin::Admin, &stdio).map_err(|refusal| match refusal {
        Refusal::Invalid(field, reason) => GatewayError::Invalid { field, reason },
        Refusal::NameTaken => GatewayError::Conflict("name_taken"),
    })?;
    Ok((document, display_name, expected))
}

fn path_name(name: &str) -> Result<(), GatewayError> {
    if crate::config::valid_mcp_name(name) {
        Ok(())
    } else {
        Err(GatewayError::Invalid {
            field: "name",
            reason: InvalidReason::NameInvalid,
        })
    }
}

/// `GET /api/v1/admin/connectors` (administrator, session or token): every stored connector, by name.
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
        let all = settings::connectors::entries(&mut transaction).await?;
        let generation = settings::generation(&mut transaction).await?;
        transaction.commit().await?;
        Ok((all, generation))
    })
    .await?;
    let refs = state.secret_store.refs().await?;
    let items = all
        .iter()
        .map(|stored| item(stored, &refs))
        .collect::<Result<_, _>>()?;
    Ok(Json(Listing { items, generation }))
}

/// `PUT /api/v1/admin/connectors/{name}` (administrator, session or token): creates without a version (201) or
/// replaces the version read (200).
///
/// # Errors
/// `400 invalid_request` with details, `403`, `409 conflict`, `409 name_taken`, `409 connector_limit`, `422` for a
/// reused key, store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    path_name(&name)?;
    let (document, display_name, expected) = body(&state, &name, &bytes)?;
    super::refuse_key_shaped("secret_ref", document.secret_ref.as_deref())?;
    let request = serde_json::json!({"connector": document, "display_name": display_name, "version": expected});
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/admin/connectors/{name}"),
        &request,
    )?;
    let write = ConnectorWrite {
        document,
        display_name,
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
        if write.expected.is_none()
            && settings::connectors::entries(&mut transaction).await?.len() >= MAX_CONNECTORS
        {
            return Err(GatewayError::Conflict("connector_limit"));
        }
        let (created, stored) = settings::connectors::write(&mut transaction, write).await?;
        settings::bump(&mut transaction).await?;
        let status = if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        let response = item(&stored, &refs)?;
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

/// `DELETE /api/v1/admin/connectors/{name}?version=` (administrator, session or token). A seeded connector stays
/// deleted until its seed entry changes.
///
/// # Errors
/// `400 invalid_request` with details, `403`, `404`, `409 conflict`, store failure.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<StatusCode, GatewayError> {
    require_admin(&user)?;
    path_name(&name)?;
    let version = version_query(query)?;
    let request = serde_json::json!({"name": name, "version": version});
    let keyed = keyed(
        &headers,
        user.id,
        format!("DELETE /api/v1/admin/connectors/{name}"),
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
        settings::connectors::remove(&mut transaction, &name, version, user.id).await?;
        // Users' own credentials were bound to this connector: a later one under the same name starts without them.
        let forgotten =
            crate::connectors::credentials::forget(&state, &mut transaction, &name).await?;
        if forgotten > 0 {
            tracing::info!(connector = %name, forgotten, "users' connector credentials deleted with the connector");
        }
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

/// Installs the committed change before answering, so the next request already sees it.
async fn refresh(state: &State) -> Result<(), GatewayError> {
    state
        .settings
        .refresh(&state.pool, state.config.io_timeout(), &state.cancel)
        .await
}
