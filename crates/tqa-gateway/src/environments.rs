//! Environment templates (ARCHITECTURE §4.7): the registry and the template list.
pub(crate) mod builtin;
mod builtin_retry;
pub(crate) mod checks;
mod engine_check;
mod images;
pub use engine_check::check_template_image;
pub(crate) use engine_check::{ENGINE_READ_TIMEOUT, EngineCheck};
pub(crate) use images::{Images, keep_registering, register_built_ins};
mod registry;
pub(crate) use registry::{Binding, binding, choose, refresh};

use crate::{GatewayError, app::State, auth::Authz};
use axum::{Json, extract::State as AppState};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub(crate) struct Item<'a> {
    name: &'a str,
    display_name: &'a str,
    /// The administrator's one-line purpose (GW-I-132); `null` when there is none.
    description: Option<&'a str>,
    capabilities: Vec<&'static str>,
    is_default: bool,
    /// What a task in it gets (the environment sizes design): its own size, or the installation's default, within the
    /// ceiling. Display only.
    cpus: f64,
    memory_mib: u64,
}

#[derive(Serialize)]
pub(crate) struct Items<'a> {
    items: Vec<Item<'a>>,
}

/// `GET /api/v1/templates`: the templates the user may choose, default first; no image references or digests.
pub(crate) async fn templates(
    AppState(state): AppState<Arc<State>>,
    Authz(_): Authz,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let snapshot = state.settings.current();
    let (sandbox, ceiling) = (&state.config.sandbox, &state.config.sandbox.ceiling);
    let items = registry::listed(&snapshot)
        .into_iter()
        .map(|template| Item {
            name: template.name(),
            display_name: template.display_name(),
            description: template.description(),
            capabilities: template
                .environment()
                .capabilities
                .iter()
                .map(|capability| capability.as_str())
                .collect(),
            is_default: template.is_default(),
            cpus: crate::config::sizes::cpus(
                template
                    .cpu_millicores()
                    .unwrap_or(sandbox.cpu_millicores)
                    .min(ceiling.cpu_millicores()),
            ),
            memory_mib: template
                .memory_bytes()
                .unwrap_or(sandbox.memory_bytes)
                .min(ceiling.memory_bytes())
                / (1024 * 1024),
        })
        .collect();
    Ok(Json(serde_json::to_value(Items { items })?))
}
