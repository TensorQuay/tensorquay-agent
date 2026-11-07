//! Administration endpoints under `/api/v1/admin` (090, ADR-0028). Administrators only: a member gets `403`.
pub(crate) mod agent_teams;
mod agent_teams_rules;
mod connectors;
mod decision_engines;
mod egress;
mod egress_presets;
mod limits;
mod mcp_servers;
mod route_input;
pub(crate) use route_input::display_name_clean as route_display_name_clean;
pub(crate) use route_input::name_valid as route_name_valid;
mod environment_images;
mod routes;
mod secrets;
mod templates;
mod usage;

pub(crate) use connectors::{list as connectors, put as connector_put, remove as connector_remove};
pub(crate) use decision_engines::{
    list as decision_engines, put as decision_engine_put, remove as decision_engine_remove,
};
pub(crate) use egress::{list as egress, put as egress_put, remove as egress_remove};
pub(crate) use egress_presets::{
    apply as egress_preset_apply, list as egress_presets, remove as egress_preset_remove,
};
pub(crate) use environment_images::list as environment_images;
pub(crate) use limits::{list as limits, put as limit_put, reset as limit_reset};
pub(crate) use mcp_servers::list as mcp_servers;
pub(crate) use routes::{
    export as routes_export, list as routes, put as route_put, remove as route_remove,
};
pub(crate) use secrets::{
    bind as secret_bind, list as secrets, put as secret_put, reads as secret_reads,
    remind as secret_remind, remove as secret_remove,
};
pub(crate) use templates::{
    check as template_check, list as templates, make_default as template_default,
    put as template_put, remove as template_remove, reset as template_reset,
};
pub(crate) use usage::usage;

use crate::{
    GatewayError, InvalidReason,
    auth::{Role, User},
    conversations::idempotent::Keyed,
};
use axum::http::HeaderMap;

/// Refuses a caller who is not an administrator.
pub(crate) fn require_admin(user: &User) -> Result<(), GatewayError> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(GatewayError::Forbidden("administrator role is required"))
    }
}

/// Refuses a secret reference whose name looks like a key (SEC-033): a key pasted where its name belongs is refused before
/// anything is written, bound, audited or kept for a replay, and the details name the field, never the value. A user's
/// own credential's reference is left to the write's own check, which refuses it as reserved: the gateway names those
/// from a digest that can look like a key at random (SEC-031).
pub(crate) fn refuse_key_shaped(
    field: &'static str,
    reference: Option<&str>,
) -> Result<(), GatewayError> {
    if reference.is_some_and(|reference| {
        !tqa_types::is_per_user_secret(reference) && tqa_types::is_key_shaped_name(reference)
    }) {
        return Err(GatewayError::Invalid {
            field,
            reason: InvalidReason::NameKeyShaped,
        });
    }
    Ok(())
}

/// An optional Idempotency-Key for a settings write; `request` is the canonical body it is bound to.
pub(crate) fn keyed<'a>(
    headers: &'a HeaderMap,
    owner: uuid::Uuid,
    route: String,
    request: &serde_json::Value,
) -> Result<Option<Keyed<'a>>, GatewayError> {
    if headers.contains_key("idempotency-key") {
        Keyed::new(owner, headers, route, request).map(Some)
    } else {
        Ok(None)
    }
}

/// The required `?version=` of a delete: exactly one positive integer; missing is `409 conflict`.
pub(crate) fn version_query(query: Option<String>) -> Result<i64, GatewayError> {
    let mut version = None;
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        if key != "version" || version.is_some() {
            return Err(GatewayError::Invalid {
                field: "query",
                reason: InvalidReason::ParameterInvalid,
            });
        }
        let parsed = value
            .parse::<i64>()
            .ok()
            .filter(|number| *number >= 1)
            .ok_or(GatewayError::Invalid {
                field: "version",
                reason: InvalidReason::VersionInvalid,
            })?;
        version = Some(parsed);
    }
    version.ok_or(GatewayError::Conflict("conflict"))
}

#[cfg(test)]
#[path = "admin/key_shaped_tests.rs"]
mod key_shaped_tests;
