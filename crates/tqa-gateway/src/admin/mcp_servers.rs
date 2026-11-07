//! `GET /api/v1/admin/mcp/servers`: the install catalogue's servers by name, for an administrator's shared-profile form
//! (ADR-0012). Names and where each applies only: commands, arguments and environment stay in gateway.toml. Members get
//! no such list, and their personal profiles name no server.
use std::sync::Arc;

use axum::{Json, extract::State as AppState};
use serde::Serialize;

use super::require_admin;
use crate::{GatewayError, app::State, auth::Authz};

/// One catalogue server.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Server {
    name: String,
    /// The templates it runs in; empty means every template.
    templates: Vec<String>,
    /// Whether a conversation made without a profile starts with it.
    enabled_by_default: bool,
}

/// The catalogue, in file order.
#[derive(Debug, Serialize)]
pub(crate) struct Servers {
    items: Vec<Server>,
}

pub(crate) fn servers(catalogue: &crate::config::McpCatalogue) -> Servers {
    Servers {
        items: catalogue
            .servers()
            .iter()
            .map(|server| Server {
                name: server.name().to_owned(),
                templates: server.templates().to_vec(),
                enabled_by_default: server.enabled_by_default(),
            })
            .collect(),
    }
}

/// # Errors
/// Not an administrator.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Servers>, GatewayError> {
    require_admin(&user)?;
    Ok(Json(servers(&state.config.mcp)))
}
