//! `/api/v1/admin/decision/engines` (ADR-0013 point 6, the ADR-0028 pattern): decision engines by name. Writes are
//! validated as the seed is (without the file-only fields), versioned, accept a session or an administrator's API
//! token and an Idempotency-Key, are audited (an exposure change as its own action), and apply from the next
//! request. Credentials never appear: `key_ref` names a secret the engine must be allowed, and `key_missing` says
//! whether it has a value.
use super::{keyed, require_admin, route_input::version, version_query};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::Authz,
    connectors::Origin,
    decision_engines::{EngineDocument, valid_engine_name, validate},
    relay::budget::{Used, used},
    secrets::SecretRef,
    settings::{
        self,
        decision_engines::{EngineWrite, StoredEngine},
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

/// The most decision engines an install may hold, as the file may seed.
const MAX_ENGINES: usize = 16;

#[derive(Serialize)]
pub(crate) struct Listing {
    items: Vec<Value>,
    /// The settings generation the list was read at.
    generation: i64,
}

/// One engine as the API shows it: the record, its metadata, and its budget's use over the current count when it has
/// a budget.
fn item(
    stored: &StoredEngine,
    refs: &[SecretRef],
    used: Option<&Used>,
) -> Result<Value, GatewayError> {
    let Value::Object(mut fields) = serde_json::to_value(&stored.document)? else {
        return Err(GatewayError::Io);
    };
    let key_missing = stored.document.key_ref.as_deref().is_some_and(|reference| {
        reference
            .parse::<SecretRef>()
            .map_or(true, |reference| !refs.contains(&reference))
    });
    let updated_at = stored
        .updated_at
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| GatewayError::Io)?;
    let extra = serde_json::json!({
        "key_missing": key_missing,
        "version": stored.version,
        "seed": stored.seed_state,
        "last_set_by": stored.last_set_by,
        "updated_at": updated_at,
    });
    if let Value::Object(extra) = extra {
        fields.extend(extra);
    }
    if let Some(used) = used {
        fields.insert(
            "budget_used".into(),
            serde_json::json!({"calls": used.calls, "tokens": used.tokens}),
        );
        if let Some(since) = used.since {
            let since = since
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| GatewayError::Io)?;
            fields.insert("budget_since".into(), Value::String(since));
        }
    }
    Ok(Value::Object(fields))
}

/// The engine's budget use over its current count, or `None` without a budget.
async fn use_of<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    stored: &StoredEngine,
) -> Result<Option<Used>, GatewayError> {
    match &stored.document.budget {
        Some(budget) => Ok(Some(used(executor, &stored.document.name, budget).await?)),
        None => Ok(None),
    }
}

/// The request body: every engine field except `name`, plus `version?`; validated as the seed is, without the
/// file-only fields.
fn body(name: &str, bytes: &[u8]) -> Result<(EngineDocument, Option<i64>), GatewayError> {
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
    let expected = version(fields.remove("version"))?;
    fields.insert("name".into(), Value::String(name.to_owned()));
    let document: EngineDocument =
        serde_json::from_value(Value::Object(fields)).map_err(|_| body_invalid())?;
    validate(&document, Origin::Admin)
        .map_err(|(field, reason)| GatewayError::Invalid { field, reason })?;
    Ok((document, expected))
}

fn path_name(name: &str) -> Result<(), GatewayError> {
    if valid_engine_name(name) {
        Ok(())
    } else {
        Err(GatewayError::Invalid {
            field: "name",
            reason: InvalidReason::NameInvalid,
        })
    }
}

/// `GET /api/v1/admin/decision/engines` (administrator, session or token): every stored engine, by name.
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
        let all = settings::decision_engines::entries(&mut transaction).await?;
        let generation = settings::generation(&mut transaction).await?;
        let mut budget_uses = Vec::with_capacity(all.len());
        for stored in &all {
            budget_uses.push(use_of(&mut *transaction, stored).await?);
        }
        transaction.commit().await?;
        Ok((
            all.into_iter().zip(budget_uses).collect::<Vec<_>>(),
            generation,
        ))
    })
    .await?;
    let refs = state.secret_store.refs().await?;
    let items = all
        .iter()
        .map(|(stored, budget_use)| item(stored, &refs, budget_use.as_ref()))
        .collect::<Result<_, _>>()?;
    Ok(Json(Listing { items, generation }))
}

/// `PUT /api/v1/admin/decision/engines/{name}` (administrator, session or token): creates without a version (201) or
/// replaces the version read (200).
///
/// # Errors
/// `400 invalid_request` with details, `403`, `409 conflict`, `409 decision_engine_limit`, the secret binding's
/// refusal, `422` for a reused key, store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    path_name(&name)?;
    let (document, expected) = body(&name, &bytes)?;
    super::refuse_key_shaped("key_ref", document.key_ref.as_deref())?;
    let request = serde_json::json!({"engine": document, "version": expected});
    let keyed = keyed(
        &headers,
        user.id,
        format!("PUT /api/v1/admin/decision/engines/{name}"),
        &request,
    )?;
    let write = EngineWrite {
        document,
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
            && settings::decision_engines::entries(&mut transaction)
                .await?
                .len()
                >= MAX_ENGINES
        {
            return Err(GatewayError::Conflict("decision_engine_limit"));
        }
        let (created, stored) = settings::decision_engines::write(&mut transaction, write).await?;
        settings::bump(&mut transaction).await?;
        let status = if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        let budget_use = use_of(&mut *transaction, &stored).await?;
        let response = item(&stored, &refs, budget_use.as_ref())?;
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

/// `DELETE /api/v1/admin/decision/engines/{name}?version=` (administrator, session or token). A seeded engine stays
/// deleted until its seed entry changes. Profiles naming it become unusable, and their tasks are refused, never run
/// without it.
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
        format!("DELETE /api/v1/admin/decision/engines/{name}"),
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
        settings::decision_engines::remove(&mut transaction, &name, version, user.id).await?;
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
