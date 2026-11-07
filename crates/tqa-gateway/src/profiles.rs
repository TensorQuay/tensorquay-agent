//! Agent profiles (ADR-0012): one record that narrows the install, copied into each conversation made from it.
mod document;
mod effective;
mod endpoints;
mod seed;
mod store;
mod usage;
pub(crate) use document::{Profile, ProfileInput, validate};
pub(crate) use effective::effective;
pub(crate) use endpoints::{create, get, list, put, remove};
pub(crate) use seed::seed;
pub(crate) use store::MAX_SHARED_PROFILES;
pub(crate) use usage::{Chosen, chosen, set};

/// The names a profile uses from the install, from a validated profile or a stored document.
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct References {
    #[serde(default)]
    model_route: Option<String>,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    mcp_servers: Vec<String>,
    #[serde(default)]
    connectors: Vec<String>,
    #[serde(default)]
    decision_engine: Option<String>,
}

impl References {
    pub(crate) fn of(profile: &Profile) -> Self {
        Self {
            model_route: profile.model_route.clone(),
            template: profile.template.clone(),
            mcp_servers: profile.mcp_servers.clone(),
            connectors: profile.connectors.clone(),
            decision_engine: profile.decision_engine.clone(),
        }
    }
}

/// The first field naming something the install does not have: a route, a template, a catalogue server or a
/// connector. Skills are discovered by each run, so an unknown skill only narrows to nothing. Checked at every write,
/// and at every read and use, since the install's names change through the API (GW-I-086).
pub(crate) fn unknown(
    snapshot: &crate::settings::Snapshot,
    servers: &[crate::config::McpServer],
    profile: &References,
) -> Option<&'static str> {
    if profile
        .model_route
        .as_deref()
        .is_some_and(|route| snapshot.registry.get(route).is_none())
    {
        return Some("model_route");
    }
    if profile.template.as_deref().is_some_and(|name| {
        !snapshot
            .templates
            .iter()
            .any(|stored| stored.template.name() == name)
    }) {
        return Some("template");
    }
    if !profile
        .mcp_servers
        .iter()
        .all(|name| servers.iter().any(|server| server.name() == name))
    {
        return Some("mcp_servers");
    }
    if !profile.connectors.iter().all(|name| {
        snapshot
            .connectors
            .iter()
            .any(|connector| &connector.name == name)
    }) {
        return Some("connectors");
    }
    if profile.decision_engine.as_deref().is_some_and(|name| {
        !snapshot
            .decision_engines
            .iter()
            .any(|engine| engine.name == name)
    }) {
        return Some("decision_engine");
    }
    None
}

/// The first field that stops a conversation being made from the profile now (ADR-0012 decision 5, GW-I-086): a
/// name the install does not have, a connector it has disabled, or a template it does not offer.
pub(crate) fn unusable(
    snapshot: &crate::settings::Snapshot,
    servers: &[crate::config::McpServer],
    profile: &References,
) -> Option<&'static str> {
    if let Some(field) = unknown(snapshot, servers, profile) {
        return Some(field);
    }
    if profile
        .template
        .as_deref()
        .is_some_and(|name| crate::environments::choose(snapshot, Some(name)).is_err())
    {
        return Some("template");
    }
    if profile.connectors.iter().any(|name| {
        snapshot
            .connectors
            .iter()
            .any(|connector| &connector.name == name && !connector.enabled)
    }) {
        return Some("connectors");
    }
    if profile.decision_engine.as_deref().is_some_and(|name| {
        snapshot
            .decision_engines
            .iter()
            .any(|engine| engine.name == name && !engine.enabled)
    }) {
        return Some("decision_engine");
    }
    None
}
