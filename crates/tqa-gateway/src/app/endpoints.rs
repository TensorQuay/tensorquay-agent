use super::State as AppState;
use crate::GatewayError;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

pub(super) async fn health() -> StatusCode {
    StatusCode::OK
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadyQuery {
    detail: Option<u8>,
}

pub(super) async fn ready(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReadyQuery>,
    auth: Result<crate::auth::Authz, GatewayError>,
) -> Result<Response, GatewayError> {
    if !matches!(query.detail, None | Some(0 | 1)) {
        return Err(GatewayError::BadRequest("detail must be zero or one"));
    }
    if query.detail != Some(1) {
        database(&state).await?;
        return Ok(StatusCode::OK.into_response());
    }
    if auth?.0.role != crate::Role::Admin {
        return Err(GatewayError::Forbidden("administrator role is required"));
    }
    let mut checks = state.manager.diagnostics().await;
    checks["database"] = serde_json::json!(database(&state).await.is_ok());
    checks["secret_store"] = serde_json::json!(state.secret_store.refs().await.is_ok());
    let ready = checks
        .as_object()
        .is_some_and(|checks| checks.values().all(|value| value == true));
    let body = serde_json::json!({"ready":ready,"checks":checks,"sandbox":{"backend":state.config.backend_name(),"network_mode":state.config.sandbox.network_mode},"templates":templates(&state).await,"git_credentials_kept":git_credentials_kept(&state).await,"runs_active":runs_active(&state).await});
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    Ok((status, axum::Json(body)).into_response())
}

/// The template diagnostics (ADR-0032): whether the default template is offered, and why not, and how many template
/// versions await their checks. Diagnostics only: they never make the gateway unready.
async fn templates(state: &AppState) -> serde_json::Value {
    let snapshot = state.settings.current();
    let default = snapshot
        .templates
        .iter()
        .find(|stored| stored.template.is_default());
    let pending = crate::environments::checks::pending(&state.pool).await.ok();
    serde_json::json!({
        "default": default.map(|stored| stored.template.name()),
        "default_offered": default.is_some_and(crate::settings::templates::StoredTemplate::offered),
        "default_reason": default.and_then(crate::settings::templates::StoredTemplate::unavailable),
        "checks_pending": pending,
    })
}

/// The Git credentials kept because they couldn't become keys (ADR-0037 §7's second release), by why; `null` when the
/// count couldn't be read. Diagnostics only: they never make the gateway unready.
async fn git_credentials_kept(state: &AppState) -> serde_json::Value {
    let kept = crate::io::bounded(
        state.config.io_timeout(),
        &state.cancel,
        crate::host_keys::git_credentials_kept(&state.pool),
    )
    .await;
    kept.ok()
        .and_then(|kept| serde_json::to_value(kept).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// How many tasks are mid-step now (a restart would start each of those steps over); `null` when it couldn't be read. A
/// count only, never which tasks or whose. Diagnostics only: it never makes the gateway unready.
async fn runs_active(state: &AppState) -> serde_json::Value {
    let counted = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        crate::conversations::active::count(&state.pool, &state.links, tokio::time::Instant::now())
            .await
    })
    .await;
    counted.map_or(serde_json::Value::Null, |count| serde_json::json!(count))
}

async fn database(state: &AppState) -> Result<(), GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let versions =
            sqlx::query!("SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&state.pool)
                .await?;
        let expected = crate::MIGRATOR.iter().collect::<Vec<_>>();
        if versions.len() != expected.len()
            || versions.iter().zip(expected).any(|(actual, expected)| {
                !actual.success
                    || actual.version != expected.version
                    || actual.checksum.as_slice() != expected.checksum.as_ref()
            })
        {
            return Err(GatewayError::StoreUnavailable);
        }
        Ok(())
    })
    .await
}

pub(super) async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    (
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        state.telemetry.render(),
    )
        .into_response()
}
