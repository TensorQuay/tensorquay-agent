//! The decision engine record (ADR-0013 point 6): an external engine the gateway relays `decision/v1` to, configured like
//! a route in its own table (never a model protocol, so no model path can reach it) and validated by one function
//! whether it comes from the `[[decision_engines]]` seed or the admin API. It holds no credential: `key_ref` names a
//! secret in the store, as a route's does.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use tqa_types::DecisionExposure;

use crate::{
    InvalidReason,
    connectors::{Auth, Origin, Scheme, check_auth, check_url},
};

/// The longest display name, in characters, as for connectors.
const MAX_DISPLAY_NAME_CHARS: usize = 80;
/// The default and the longest time an engine has to answer (ADR-0013 point 5).
const DEFAULT_TIMEOUT_MS: u32 = 2000;
const MAX_TIMEOUT_MS: u32 = tqa_types::MAX_DECISION_TIMEOUT_MS;
/// A typed-question engine's defaults and bounds (ADR-0013, the adapter amendment).
const DEFAULT_MIN_CONFIDENCE: f64 = 0.6;
const MAX_MODEL_BYTES: usize = 128;
const MAX_ROUTE_HINTS: usize = 16;
const MAX_ROUTE_HINT_CHARS: usize = 200;

/// The protocol the gateway speaks to an engine.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EngineProtocol {
    /// `decision/v1`, forwarded as is.
    #[default]
    DecisionV1,
    /// A typed-question service, asked one multiple-choice question per step and translated in the relay.
    TypedQuestionsV1,
}

impl EngineProtocol {
    /// The points an engine of this protocol answers, for the runtime's snapshot; `None` means both.
    pub(crate) fn points(self) -> Option<Vec<tqa_types::DecisionPoint>> {
        match self {
            Self::DecisionV1 => None,
            // It only chooses the step's route, so the runtime never asks it after a response.
            Self::TypedQuestionsV1 => Some(vec![tqa_types::DecisionPoint::BeforeStep]),
        }
    }
}

/// An engine's optional budget, an administrator's setting (ADR-0013, the adapter amendment): at most this many
/// consultations, or this many tokens as the service reports them (input and output), per period. Counted from the
/// engine's usage rows in the relay.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngineBudget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(default)]
    pub period: BudgetPeriod,
}

/// The period a budget counts over.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BudgetPeriod {
    /// Since the budget was set.
    #[default]
    Total,
    /// A calendar month in UTC.
    Month,
}

/// One engine as stored and as the API shows it (without its metadata).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngineDocument {
    pub name: String,
    /// What people read: the settings list and the profile's choice show it; the name stays the identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub protocol: EngineProtocol,
    /// The engine's base URL; the relay posts to `<url>/v1/before_step` and `/v1/after_response`.
    pub url: String,
    #[serde(default)]
    pub auth: Auth,
    /// The installation's credential for the engine, a secret reference; never a user's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_ref: Option<String>,
    /// How long the engine has to answer, 1 through 10000 ms.
    #[serde(default = "default_timeout")]
    pub timeout_ms: u32,
    /// How much of the conversation the engine may see; `none` by default.
    #[serde(default)]
    pub exposure: DecisionExposure,
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// File-only: plain `http` for tests and on-premises engines.
    #[serde(default, skip_serializing_if = "is_false")]
    pub insecure_http: bool,
    /// File-only: a private certificate authority for this engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<PathBuf>,
    /// A typed-question engine's model identifier, which its service requires; pinned for a measurement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A typed-question engine's threshold: a choice below it takes the conversation's own route. 0.6 when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f64>,
    /// A typed-question engine's one-line description of each route, offered with its option.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub route_hints: BTreeMap<String, String>,
    /// The optional budget, any kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<EngineBudget>,
}

impl EngineDocument {
    /// The confidence below which a typed-question engine's choice gives way to the conversation's own route.
    pub(crate) fn min_confidence(&self) -> f64 {
        self.min_confidence.unwrap_or(DEFAULT_MIN_CONFIDENCE)
    }
}

const fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_MS
}

const fn enabled() -> bool {
    true
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_false(value: &bool) -> bool {
    !*value
}

/// Whether `name` is an engine name: the wire rule, shared with the runtime's snapshot.
pub(crate) fn valid_engine_name(name: &str) -> bool {
    tqa_types::valid_decision_engine_name(name)
}

/// Checks every rule of ADR-0013 point 6.
///
/// # Errors
/// The first field that breaks its rule, with its reason.
pub(crate) fn validate(
    document: &EngineDocument,
    origin: Origin,
) -> Result<(), (&'static str, InvalidReason)> {
    if !valid_engine_name(&document.name) {
        return Err(("name", InvalidReason::NameInvalid));
    }
    if let Some(display) = &document.display_name {
        let count = display.chars().count();
        if !(1..=MAX_DISPLAY_NAME_CHARS).contains(&count)
            || display.chars().any(char::is_control)
            || display.trim() != display
        {
            return Err(("display_name", InvalidReason::DisplayNameInvalid));
        }
    }
    if origin == Origin::Admin && document.insecure_http {
        return Err(("insecure_http", InvalidReason::FileOnly));
    }
    if let Some(path) = &document.ca_file {
        if origin == Origin::Admin {
            return Err(("ca_file", InvalidReason::FileOnly));
        }
        if !path.is_absolute() {
            return Err(("ca_file", InvalidReason::ParameterInvalid));
        }
    }
    check_url(&document.url, document.insecure_http)?;
    check_auth(&document.auth)?;
    if let Some(reference) = &document.key_ref {
        // A secret reference only, as for a route's key_ref.
        if reference.parse::<crate::secrets::SecretRef>().is_err() {
            return Err(("key_ref", InvalidReason::KeyRefNotReference));
        }
        // The installation's credential, never one user's own (FR-19, FR-24).
        if tqa_types::is_per_user_secret(reference) {
            return Err(("key_ref", InvalidReason::SecretRefReserved));
        }
        if document.auth.scheme == Scheme::None {
            return Err(("key_ref", InvalidReason::AuthInvalid));
        }
    }
    if !(1..=MAX_TIMEOUT_MS).contains(&document.timeout_ms) {
        return Err(("timeout_ms", InvalidReason::LimitOutOfRange));
    }
    match document.protocol {
        EngineProtocol::DecisionV1 => plain_fields(document)?,
        EngineProtocol::TypedQuestionsV1 => typed_fields(document)?,
    }
    budget(document)
}

/// A `decision/v1` engine carries none of the typed-question kind's fields.
fn plain_fields(document: &EngineDocument) -> Result<(), (&'static str, InvalidReason)> {
    if document.model.is_some() {
        return Err(("model", InvalidReason::ParameterInvalid));
    }
    if document.min_confidence.is_some() {
        return Err(("min_confidence", InvalidReason::ParameterInvalid));
    }
    if !document.route_hints.is_empty() {
        return Err(("route_hints", InvalidReason::ParameterInvalid));
    }
    Ok(())
}

/// A typed-question engine: its service requires a model; the threshold is 0 to 1; hints are short, one line each.
fn typed_fields(document: &EngineDocument) -> Result<(), (&'static str, InvalidReason)> {
    match document.model.as_deref() {
        None | Some("") => return Err(("model", InvalidReason::ModelEmpty)),
        Some(model)
            if model.len() > MAX_MODEL_BYTES
                || !model.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-' | b'/')
                }) =>
        {
            return Err(("model", InvalidReason::ParameterInvalid));
        }
        Some(_) => {}
    }
    if document
        .min_confidence
        .is_some_and(|value| !(0.0..=1.0).contains(&value))
    {
        return Err(("min_confidence", InvalidReason::LimitOutOfRange));
    }
    if document.route_hints.len() > MAX_ROUTE_HINTS {
        return Err(("route_hints", InvalidReason::ParameterInvalid));
    }
    for (route, hint) in &document.route_hints {
        if !crate::admin::route_name_valid(route) {
            return Err(("route_hints", InvalidReason::RouteNameInvalid));
        }
        let chars = hint.chars().count();
        if !(1..=MAX_ROUTE_HINT_CHARS).contains(&chars) || hint.chars().any(char::is_control) {
            return Err(("route_hints", InvalidReason::ParameterInvalid));
        }
    }
    Ok(())
}

/// A budget names calls or tokens, each at least 1; a `decision_v1` engine reports no tokens, so it counts calls only.
fn budget(document: &EngineDocument) -> Result<(), (&'static str, InvalidReason)> {
    let Some(budget) = document.budget else {
        return Ok(());
    };
    let named = budget.calls.is_some() || budget.tokens.is_some();
    let positive = budget.calls != Some(0) && budget.tokens != Some(0);
    let counted = !(document.protocol == EngineProtocol::DecisionV1 && budget.tokens.is_some());
    if named && positive && counted {
        Ok(())
    } else {
        Err(("budget", InvalidReason::BudgetInvalid))
    }
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "typed_tests.rs"]
mod typed_tests;
