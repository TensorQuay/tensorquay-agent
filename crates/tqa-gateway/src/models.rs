//! Public model capabilities, using the same registry membership as conversation creation.

use crate::{GatewayError, app::State, auth::Authz};
use axum::{Json, extract::State as AppState};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};

#[derive(Serialize)]
pub(crate) struct RouteSummary {
    name: String,
    /// The administrator's label for pickers; `None` shows the name.
    display_name: Option<String>,
    /// The administrator's one-line purpose (GW-I-132); `None` when there is none.
    description: Option<String>,
    /// Whether a current route probe passed; `None` until the route probe (099) records one, or once the route changed.
    verified: Option<bool>,
    model: String,
    protocol: tqa_llm::Protocol,
    context_window: Option<u32>,
    vision: bool,
    /// Whether the route takes structured tool calls; without them the runtime uses its text fallback.
    native_tools: bool,
    reasoning: Option<Reasoning>,
    /// Where the model runs, for the picker's second line; never its address (GW-I-152).
    runs_on: RunsOn,
}

#[derive(Serialize)]
struct RunsOn {
    #[serde(rename = "where")]
    place: crate::route_place::Place,
    /// A well-known public API's name, else `None` (the web says "Hosted").
    provider: Option<&'static str>,
}

#[derive(Serialize)]
struct Reasoning {
    allowed: Vec<String>,
    default: String,
}

#[derive(Serialize)]
pub(crate) struct Models {
    items: Vec<RouteSummary>,
}

/// Each route's current check (GW-I-042): `true` when the route's latest probe passed and tested the route as it is now,
/// `false` when it failed; a route never probed, or changed since (stale, as the admin list says, GW-I-140), is absent.
async fn checks(state: &State) -> Result<HashMap<String, bool>, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let rows = sqlx::query!(
            "SELECT p.route, p.ok FROM route_probes p JOIN model_routes r ON r.name = p.route \
             WHERE p.document_digest = sha256(convert_to(r.document::text, 'UTF8'))"
        )
        .fetch_all(&state.pool)
        .await?;
        Ok(rows.into_iter().map(|row| (row.route, row.ok)).collect())
    })
    .await
}

pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(_user): Authz,
) -> Json<Models> {
    // A check that can't be read leaves every route unknown (null), never the list unanswered: New task then picks as
    // it did before routes were checked.
    let checks = checks(&state).await.unwrap_or_else(|error| {
        tracing::warn!(
            code = error.code(),
            "route checks weren't read for the model list"
        );
        HashMap::new()
    });
    let settings = state.settings.current();
    let items = settings
        .registry
        .names()
        .filter_map(|name| settings.registry.get(name))
        .map(|route| RouteSummary {
            name: route.name.clone(),
            display_name: settings.display_names.get(&route.name).cloned(),
            description: settings.descriptions.get(&route.name).cloned(),
            verified: checks.get(&route.name).copied(),
            model: route.model.clone(),
            protocol: route.protocol,
            context_window: route.limits.context_window,
            vision: route.vision,
            native_tools: route.native_tools,
            reasoning: route.reasoning.as_ref().map(|value| Reasoning {
                allowed: value.allowed.clone(),
                default: value.default.clone(),
            }),
            runs_on: RunsOn {
                place: crate::route_place::place(&route.base_url),
                provider: crate::route_place::provider(&route.base_url),
            },
        })
        .collect();
    Json(Models { items })
}
