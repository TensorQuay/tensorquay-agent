//! `/api/v1/admin/egress/presets` (GW-I-156): the built-in internet-access presets. The list says how much of each is
//! applied; applying one adds its missing rules tagged with it, removing one deletes the rules still tagged. Both take
//! an Idempotency-Key, are audited per rule and apply from each sandbox's next new connection, as a rule's own write
//! does.
use super::{
    egress::{refresh, writable},
    keyed, require_admin,
};
use crate::{
    GatewayError,
    app::State,
    auth::Authz,
    settings::{
        self,
        egress_presets::{self, Applied, Preset, Rule},
    },
};
use axum::{
    Json,
    extract::{Path, State as AppState},
    http::{HeaderMap, StatusCode},
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

#[derive(Serialize)]
pub(crate) struct Item {
    id: &'static str,
    rules: Vec<Rule>,
    applied: Applied,
}

#[derive(Serialize)]
pub(crate) struct Listing {
    items: Vec<Item>,
}

/// `GET /api/v1/admin/egress/presets` (administrator, session or token): every preset, its rules and how much is
/// applied. The web names and describes each by its id.
///
/// # Errors
/// Not an administrator, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Listing>, GatewayError> {
    require_admin(&user)?;
    let applied = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        egress_presets::applied(&mut connection).await
    })
    .await?;
    let items = applied
        .into_iter()
        .map(|(preset, applied)| Item {
            id: preset.id,
            rules: preset
                .rules
                .iter()
                .map(|(host, ports)| Rule {
                    host: (*host).to_owned(),
                    ports: Some(ports.to_vec()),
                })
                .collect(),
            applied,
        })
        .collect();
    Ok(Json(Listing { items }))
}

fn preset(id: &str) -> Result<&'static Preset, GatewayError> {
    egress_presets::find(id).ok_or(GatewayError::NotFound)
}

/// `POST /api/v1/admin/egress/presets/{id}` (administrator, session or token, Idempotency-Key): adds the preset's
/// missing rules, tagged with it, and keeps a host that already has one. Answers `{added: [{host, ports}], kept:
/// [{host}]}`.
///
/// # Errors
/// `403`, `404` for an unknown preset, `409 egress_rule_limit`, `409 egress_disabled`, `422` for a reused key, store
/// failure.
pub(crate) async fn apply(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    require_admin(&user)?;
    let preset = preset(&id)?;
    write(&state, &headers, user.id, preset, Direction::Apply).await
}

/// `DELETE /api/v1/admin/egress/presets/{id}` (administrator, session or token, Idempotency-Key): removes the rules
/// still tagged with the preset. Answers `{removed: [{host}], kept: [{host}]}`, kept being rules an administrator
/// edited since, which are theirs.
///
/// # Errors
/// `403`, `404` for an unknown preset, `409 egress_disabled`, `422` for a reused key, store failure.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    require_admin(&user)?;
    let preset = preset(&id)?;
    write(&state, &headers, user.id, preset, Direction::Remove).await
}

#[derive(Clone, Copy)]
enum Direction {
    Apply,
    Remove,
}

async fn write(
    state: &State,
    headers: &HeaderMap,
    actor: uuid::Uuid,
    preset: &'static Preset,
    direction: Direction,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    writable(state)?;
    let method = match direction {
        Direction::Apply => "POST",
        Direction::Remove => "DELETE",
    };
    let request = serde_json::json!({"preset": preset.id});
    let keyed = keyed(
        headers,
        actor,
        format!("{method} /api/v1/admin/egress/presets/{}", preset.id),
        &request,
    )?;
    let (status, response) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(keyed) = &keyed
            && let Some(cached) = keyed.begin(state, &mut transaction).await?
        {
            return Ok(cached);
        }
        settings::lock(&mut transaction).await?;
        let response = match direction {
            Direction::Apply => {
                let (added, kept) = egress_presets::apply(&mut transaction, preset, actor).await?;
                serde_json::json!({"added": added, "kept": kept})
            }
            Direction::Remove => {
                let (removed, kept) =
                    egress_presets::remove(&mut transaction, preset, actor).await?;
                serde_json::json!({"removed": removed, "kept": kept})
            }
        };
        settings::bump(&mut transaction).await?;
        if let Some(keyed) = &keyed {
            keyed
                .record(&mut transaction, StatusCode::OK, &response)
                .await?;
        }
        transaction.commit().await?;
        Ok((StatusCode::OK, response))
    })
    .await?;
    refresh(state).await?;
    Ok((status, Json(response)))
}
