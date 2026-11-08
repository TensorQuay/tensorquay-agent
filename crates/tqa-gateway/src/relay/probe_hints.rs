//! What a failed probe suggests: an upstream 400 whose wording points at a route setting
//! suggests that setting. Only the wording decides, never a provider's name; a suggestion is a hint, and the probe never
//! changes the route.
use serde::Serialize;
use std::borrow::Cow;

/// A route setting the refusal points at.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct Suggestion {
    /// The route field.
    pub field: &'static str,
    /// The value to try: fixed, or a number the check read (the server's context, GW-U-153).
    pub value: Cow<'static, str>,
    /// Why, in one sentence.
    pub reason: &'static str,
}

/// Schema keywords `schema_keywords = "basic"` drops, lower-cased.
const DROPPED_KEYWORDS: [&str; 11] = [
    "additionalproperties",
    "pattern",
    "minlength",
    "maxlength",
    "$ref",
    "const",
    "default",
    "multipleof",
    "exclusiveminimum",
    "exclusivemaximum",
    "$defs",
];
/// Words that say a named thing is refused.
const REFUSED: [&str; 4] = [
    "not supported",
    "unsupported",
    "unknown name",
    "not allowed",
];
/// Words that, next to `schema`, say a tool schema broke the strict rules.
const STRICT: [&str; 4] = [
    "strict",
    "additionalproperties",
    "'required'",
    "invalid schema",
];

/// What the strict-string check suggests when strict mode lost a line break that the same request without it kept
/// (GW-U-064).
pub(super) const STRICT_LOSES_LINE_BREAKS: Suggestion = Suggestion {
    field: "strict",
    value: Cow::Borrowed("off"),
    reason: "the endpoint dropped line breaks from a strict tool's text argument but kept them without strict; off sends every tool without strict",
};

/// Words in a 404's body that say the model is unknown (`model_not_found`, "model \"x\" not found", "the model `x`
/// does not exist"): the endpoint is there, the model isn't.
const MODEL_UNKNOWN: [&str; 5] = [
    "not found",
    "not_found",
    "does not exist",
    "doesn't exist",
    "no such",
];

/// The check's own request refused with 404 or 405 at the protocol's path: unless the body says the model is unknown,
/// the address has no endpoint for this protocol (`endpoint_missing`, Models #3): most likely another protocol, or an
/// address missing its version path. A 404 naming an unknown model stays `not_found`; every other failure is unchanged.
pub(super) fn protocol_endpoint(mut failure: super::probe::Failure) -> super::probe::Failure {
    let model_unknown = || {
        let text = failure
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        text.contains("model") && MODEL_UNKNOWN.iter().any(|words| text.contains(words))
    };
    match failure.status {
        Some(405) => failure.code = "endpoint_missing",
        Some(404) if !model_unknown() => failure.code = "endpoint_missing",
        _ => {}
    }
    failure
}

/// The setting a refusal with `status` and this redacted body excerpt points at, if any.
pub(super) fn suggestion(status: u16, excerpt: &str) -> Option<Suggestion> {
    if status != 400 {
        return None;
    }
    let text = excerpt.to_lowercase();
    let names_dropped_keyword = DROPPED_KEYWORDS.iter().any(|keyword| {
        ['"', '\''].iter().any(|quote| {
            text.contains(&format!("{quote}{keyword}{quote}"))
                || text.contains(&format!("\\{quote}{keyword}\\{quote}"))
        })
    });
    if names_dropped_keyword && REFUSED.iter().any(|word| text.contains(word)) {
        return Some(Suggestion {
            field: "schema_keywords",
            value: Cow::Borrowed("basic"),
            reason: "the endpoint refused a JSON Schema keyword in a tool schema; basic keeps only the widely supported ones",
        });
    }
    if text.contains("schema") && STRICT.iter().any(|word| text.contains(word)) {
        return Some(Suggestion {
            field: "strict",
            value: Cow::Borrowed("off"),
            reason: "the endpoint refused a tool schema under strict mode; off sends every tool without strict",
        });
    }
    None
}
