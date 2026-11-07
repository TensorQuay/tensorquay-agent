//! Exact TOML parsing, validated route lookup and deterministic export.

mod invalid;

pub use invalid::RouteInvalid;

use crate::{Route, RouteConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tqa_types::MAX_FALLBACK_ROUTES;

/// Maximum encoded registry size; bounds parser allocation before deserialization.
pub const MAX_REGISTRY_BYTES: usize = 1024 * 1024;

/// An invalid registry with a parser or route location when available.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("registry error: {detail}")]
pub struct RegistryError {
    line: Option<usize>,
    column: Option<usize>,
    detail: String,
    route: Option<String>,
    reason: Option<RouteInvalid>,
}

impl RegistryError {
    pub(crate) fn at(detail: &str, source: &str, offset: Option<usize>) -> Self {
        let prefix = offset.and_then(|index| source.get(..index));
        let line = prefix.map(|text| text.bytes().filter(|byte| *byte == b'\n').count() + 1);
        let column = prefix.map(|text| {
            text.rsplit('\n')
                .next()
                .map_or(1, |last| last.chars().count() + 1)
        });
        let detail = crate::redact(detail);
        Self {
            line,
            column,
            detail,
            route: None,
            reason: None,
        }
    }

    /// A route refused for a typed reason; the message stays "route <name>: <reason>".
    pub(crate) fn invalid(
        route: &str,
        reason: RouteInvalid,
        source: &str,
        offset: Option<usize>,
    ) -> Self {
        Self {
            route: Some(route.to_owned()),
            reason: Some(reason),
            ..Self::at(&format!("route {route}: {reason}"), source, offset)
        }
    }
    /// Returns the one-based source line, when a source location was available.
    #[must_use]
    pub const fn line(&self) -> Option<usize> {
        self.line
    }
    /// Returns the one-based source column, when a source location was available.
    #[must_use]
    pub const fn column(&self) -> Option<usize> {
        self.column
    }
    /// The refused route's name, when a route rule failed.
    #[must_use]
    pub fn route(&self) -> Option<&str> {
        self.route.as_deref()
    }
    /// The typed reason, when a route rule failed; parse errors and duplicate names have none.
    #[must_use]
    pub const fn reason(&self) -> Option<&RouteInvalid> {
        self.reason.as_ref()
    }
    /// Returns the stable registry-error classification.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        "registry"
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryInput {
    routes: Vec<toml::Spanned<RouteConfig>>,
}

#[derive(Serialize)]
struct RegistryOutput {
    routes: Vec<RouteConfig>,
}

/// Validated, uniquely named routes; iteration and export use name order.
#[derive(Clone, Debug, PartialEq)]
pub struct Registry {
    routes: BTreeMap<String, Route>,
    /// The display names the file gave, by route; none for a route without one.
    display_names: BTreeMap<String, String>,
}

impl Registry {
    /// Parses exact TOML and validates every route before making it available.
    ///
    /// # Errors
    /// Rejects oversized input, unknown fields, duplicate names and invalid route policies.
    pub fn from_toml(source: &str) -> Result<Self, RegistryError> {
        if source.len() > MAX_REGISTRY_BYTES {
            return Err(RegistryError::at(
                "registry exceeds byte limit",
                source,
                None,
            ));
        }
        let parsed: RegistryInput = toml::from_str(source).map_err(|error: toml::de::Error| {
            RegistryError::at(error.message(), source, error.span().map(|span| span.start))
        })?;
        let entries = parsed.routes.into_iter().map(|entry| {
            let offset = entry.span().start;
            (entry.into_inner(), Some(offset))
        });
        Self::build(entries, source)
    }

    /// Validates route records, for example rows from a database, exactly as [`Self::from_toml`] validates the
    /// file: one validator, the same reasons.
    ///
    /// # Errors
    /// Rejects duplicate names and invalid route policies; a route rule's failure names the route and its
    /// [`RouteInvalid`] reason.
    pub fn from_configs(configs: Vec<RouteConfig>) -> Result<Self, RegistryError> {
        Self::build(configs.into_iter().map(|config| (config, None)), "")
    }

    fn build(
        entries: impl Iterator<Item = (RouteConfig, Option<usize>)>,
        source: &str,
    ) -> Result<Self, RegistryError> {
        let (mut routes, mut display_names) = (BTreeMap::new(), BTreeMap::new());
        for (mut config, offset) in entries {
            if let Some(display_name) = config.display_name.take() {
                display_names.insert(config.name.clone(), display_name);
            }
            let route = Route::from(config);
            validate_route(&route)
                .and_then(|()| configured_key_ref(&route))
                .map_err(|reason| RegistryError::invalid(&route.name, reason, source, offset))?;
            if routes.contains_key(&route.name) {
                return Err(RegistryError::at("duplicate route name", source, offset));
            }
            routes.insert(route.name.clone(), route);
        }
        validate_fallbacks(&routes)
            .map_err(|(route, reason)| RegistryError::invalid(&route, reason, source, None))?;
        Ok(Self {
            routes,
            display_names,
        })
    }

    /// Serializes routes to exact registry TOML in stable name order.
    ///
    /// # Errors
    /// Returns a typed error if a field cannot be represented in TOML.
    pub fn to_toml(&self) -> Result<String, RegistryError> {
        toml::to_string_pretty(&RegistryOutput {
            routes: self
                .routes
                .values()
                .cloned()
                .map(|route| self.config(route))
                .collect(),
        })
        .map_err(|error| RegistryError::at(&error.to_string(), "", None))
    }

    /// The display name the file gave `name`, if any.
    #[must_use]
    pub fn display_name(&self, name: &str) -> Option<&str> {
        self.display_names.get(name).map(String::as_str)
    }

    /// `route` as its record, with the display name the file gave it.
    #[must_use]
    pub fn config(&self, route: Route) -> RouteConfig {
        let display_name = self.display_names.get(&route.name).cloned();
        RouteConfig {
            display_name,
            ..RouteConfig::from(route)
        }
    }

    /// Looks up a logical name without changing the registry.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Route> {
        self.routes.get(name)
    }
    /// Iterates all route names in sorted order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.routes.keys().map(String::as_str)
    }
}

/// Checks every route's `fallbacks`: at most [`MAX_FALLBACK_ROUTES`], existing, not itself, unique,
/// and no cycle through the fallback graph.
fn validate_fallbacks(routes: &BTreeMap<String, Route>) -> Result<(), (String, RouteInvalid)> {
    for route in routes.values() {
        let fallbacks = &route.fallbacks;
        let problem = if fallbacks.len() > MAX_FALLBACK_ROUTES {
            Some("declares more than 3 fallbacks")
        } else if fallbacks.contains(&route.name) {
            Some("lists itself as a fallback")
        } else if fallbacks.iter().any(|name| !routes.contains_key(name)) {
            Some("names an unknown fallback route")
        } else if (1..fallbacks.len()).any(|index| fallbacks[..index].contains(&fallbacks[index])) {
            Some("lists a fallback twice")
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err((route.name.clone(), RouteInvalid::FallbackInvalid(problem)));
        }
    }
    // Depth-first search with three states finds any cycle in linear time.
    let mut state: BTreeMap<&str, bool> = BTreeMap::new(); // false: on the current path, true: done
    for name in routes.keys() {
        if let Some(cycle) = cycle_from(routes, name, &mut state) {
            return Err((
                cycle.to_owned(),
                RouteInvalid::FallbackInvalid("its fallbacks form a cycle"),
            ));
        }
    }
    Ok(())
}

/// Returns a route on a fallback cycle reachable from `start`, marking visited routes in `state`.
fn cycle_from<'a>(
    routes: &'a BTreeMap<String, Route>,
    start: &'a str,
    state: &mut BTreeMap<&'a str, bool>,
) -> Option<&'a str> {
    if state.contains_key(start) {
        return None;
    }
    let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
    state.insert(start, false);
    while let Some((name, next)) = stack.pop() {
        let fallbacks = routes
            .get(name)
            .map_or(&[][..], |route| &route.fallbacks[..]);
        let Some(child) = fallbacks.get(next) else {
            state.insert(name, true);
            continue;
        };
        stack.push((name, next + 1));
        match state.get(child.as_str()) {
            Some(false) => return Some(child.as_str()),
            Some(true) => {}
            None => {
                state.insert(child.as_str(), false);
                stack.push((child.as_str(), 0));
            }
        }
    }
    None
}

pub(crate) const OWNED_FIELDS: &[&str] = &[
    "messages",
    "tools",
    "stream",
    "model",
    "tool_choice",
    "stream_options",
    "max_tokens",
    "temperature",
    "stop",
    "metadata",
    "parallel_tool_calls",
];

/// Responses request fields the harness sets; `store` and `previous_response_id` keep requests
/// stateless, and `include` would ask for server state the history does not keep.
const RESPONSES_OWNED_FIELDS: &[&str] = &[
    "model",
    "input",
    "instructions",
    "stream",
    "store",
    "previous_response_id",
    "conversation",
    "include",
    "tools",
    "tool_choice",
    "parallel_tool_calls",
    "max_output_tokens",
    "temperature",
    "metadata",
];

/// Messages request fields the harness sets; `thinking` comes only from the route's budget table.
const MESSAGES_OWNED_FIELDS: &[&str] = &[
    "model",
    "messages",
    "system",
    "tools",
    "tool_choice",
    "stream",
    "max_tokens",
    "stop_sequences",
    "temperature",
    "metadata",
    "thinking",
];

/// The smallest thinking budget the Messages protocol accepts, and a sanity ceiling.
const BUDGET_TOKENS: std::ops::RangeInclusive<u32> = 1024..=1_048_576;

/// Whether an `extra_body` field would override a field the harness sets for the route's protocol.
/// A Responses route may add to `reasoning` (for example `summary`), never set its `effort`.
fn owned_extra(route: &Route, key: &str, value: &serde_json::Value) -> bool {
    match route.protocol {
        // An adaptive-thinking route's effort is the harness's; extras may add other `output_config` fields.
        crate::Protocol::AnthropicMessages => {
            MESSAGES_OWNED_FIELDS.contains(&key)
                || (key == "output_config"
                    && route.reasoning.is_some()
                    && value
                        .as_object()
                        .is_none_or(|config| config.contains_key("effort")))
        }
        crate::Protocol::OpenAiResponses => {
            RESPONSES_OWNED_FIELDS.contains(&key)
                || (key == "reasoning"
                    && value
                        .as_object()
                        .is_none_or(|reasoning| reasoning.contains_key("effort")))
        }
        _ => {
            OWNED_FIELDS.contains(&key)
                || key == "reasoning_effort"
                || route
                    .reasoning
                    .as_ref()
                    .is_some_and(|value| value.field == key)
        }
    }
}

/// A route's `key_ref` is required when it authenticates.
fn key_ref(route: &Route) -> Result<(), RouteInvalid> {
    if route.auth != crate::Auth::None && route.key_ref.as_ref().is_none_or(String::is_empty) {
        return Err(RouteInvalid::KeyRefRequired);
    }
    Ok(())
}

/// A configured route's `key_ref` (the models file, the admin API, the stored routes): always a secret reference, never
/// a pasted key (SEC-033), and never a user's own credential (FR-19, FR-24). Only a registry applies it: a route a
/// process builds for itself (the runtime's, whose credential is its token file) names no secret.
fn configured_key_ref(route: &Route) -> Result<(), RouteInvalid> {
    if route
        .key_ref
        .as_deref()
        .is_some_and(|reference| !tqa_types::is_secret_reference(reference))
    {
        return Err(RouteInvalid::KeyRefNotReference);
    }
    if route
        .key_ref
        .as_deref()
        .is_some_and(tqa_types::is_per_user_secret)
    {
        return Err(RouteInvalid::KeyRefReserved);
    }
    Ok(())
}

/// The name no route may take: the web's Add route page lives at `/settings/models/new` (GW-U-098).
const RESERVED_ROUTE_NAME: &str = "new";

/// A route's name: present, and not the web's Add route page's address (GW-U-098).
fn validate_name(name: &str) -> Result<(), RouteInvalid> {
    if name.is_empty() {
        return Err(RouteInvalid::RouteNameInvalid);
    }
    if name == RESERVED_ROUTE_NAME {
        return Err(RouteInvalid::RouteNameReserved);
    }
    Ok(())
}

pub(crate) fn validate_route(route: &Route) -> Result<(), RouteInvalid> {
    validate_name(&route.name)?;
    if route.model.is_empty() {
        return Err(RouteInvalid::ModelEmpty);
    }
    let url = url::Url::parse(&route.base_url)
        .map_err(|_| RouteInvalid::BaseUrlInvalid("invalid base URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(RouteInvalid::BaseUrlInvalid(
            "base URL must be HTTP(S) without credentials, query or fragment",
        ));
    }
    key_ref(route)?;
    if let crate::Auth::Header { name } = &route.auth {
        reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| RouteInvalid::AuthHeaderInvalid)?;
    }
    if let crate::Auth::Query { name } = &route.auth
        && (name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
    {
        return Err(RouteInvalid::AuthQueryInvalid);
    }
    if let Some(reasoning) = &route.reasoning
        && (reasoning.field.is_empty()
            || OWNED_FIELDS.contains(&reasoning.field.as_str())
            || !reasoning.allowed.contains(&reasoning.default))
    {
        return Err(RouteInvalid::ReasoningInvalid);
    }
    validate_limits(route)?;
    validate_budget(route)?;
    for (key, value) in &route.extra_body {
        if owned_extra(route, key, value) {
            return Err(RouteInvalid::ExtraBodyReserved);
        }
    }
    for (name, value) in &route.extra_headers {
        reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| RouteInvalid::ExtraHeaderInvalid("invalid extra header name"))?;
        reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| RouteInvalid::ExtraHeaderInvalid("invalid extra header value"))?;
        if name.eq_ignore_ascii_case(crate::MESSAGES_VERSION_HEADER) {
            return Err(RouteInvalid::ExtraHeaderReserved);
        }
        if ["authorization", "x-api-key", "api-key"]
            .iter()
            .any(|key| name.eq_ignore_ascii_case(key))
            || matches!(&route.auth, crate::Auth::Header { name: auth } if name.eq_ignore_ascii_case(auth))
        {
            return Err(RouteInvalid::ExtraHeaderCredential);
        }
    }
    if !route.quirks.is_empty() {
        return Err(RouteInvalid::QuirkUnsupported);
    }
    if crate::tool_schemas::StrictTools::parse(route.strict.as_deref()).is_none() {
        return Err(RouteInvalid::StrictInvalid);
    }
    if crate::tool_schemas::SchemaKeywords::parse(route.schema_keywords.as_deref()).is_none() {
        return Err(RouteInvalid::SchemaKeywordsInvalid);
    }
    crate::instructions_role::validate(route.protocol, route.instructions_role.as_deref())?;
    if route
        .first_response_secs
        .is_some_and(|secs| !crate::route::FIRST_RESPONSE_RANGE.contains(&secs))
    {
        return Err(RouteInvalid::FirstResponseInvalid);
    }
    Ok(())
}

/// A Messages route declares its output limit (the protocol requires `max_tokens`). With a reasoning
/// policy it either maps one thinking budget per allowed value, each below that limit, or has no
/// budget table and thinks adaptively, its allowed values sent as `output_config.effort` (models
/// that refuse a budget). Other protocols take an effort, so a budget table there is a
/// configuration error.
fn validate_budget(route: &Route) -> Result<(), RouteInvalid> {
    let budgets = route
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.budget_tokens.as_ref());
    if route.protocol != crate::Protocol::AnthropicMessages {
        return match budgets {
            Some(_) => Err(RouteInvalid::BudgetInvalid(
                "budget_tokens applies only to anthropic_messages routes",
            )),
            None => Ok(()),
        };
    }
    let output = route
        .limits
        .max_output
        .ok_or(RouteInvalid::MaxOutputRequired(
            "anthropic_messages routes require max_output",
        ))?;
    let Some(reasoning) = &route.reasoning else {
        return Ok(());
    };
    let Some(budgets) = budgets else {
        return Ok(());
    };
    if budgets.len() != reasoning.allowed.len()
        || reasoning
            .allowed
            .iter()
            .any(|value| !budgets.contains_key(value))
        || budgets
            .values()
            .any(|budget| !BUDGET_TOKENS.contains(budget) || *budget >= output)
    {
        return Err(RouteInvalid::BudgetInvalid(
            "budget_tokens needs one budget of at least 1024 per allowed value, below max_output",
        ));
    }
    Ok(())
}

/// Declared limits are positive, and the output limit fits the context window (the same rule the route snapshot
/// enforces, so a route that loads here is never refused later when a conversation starts).
fn validate_limits(route: &Route) -> Result<(), RouteInvalid> {
    let limits = route.limits;
    if limits.context_window == Some(0)
        || limits.max_output == Some(0)
        || limits.concurrency == Some(0)
        || limits.requests_per_minute == Some(0)
        || limits
            .max_output
            .zip(limits.context_window)
            .is_some_and(|(output, window)| output > window)
    {
        return Err(RouteInvalid::LimitOutOfRange);
    }
    Ok(())
}
