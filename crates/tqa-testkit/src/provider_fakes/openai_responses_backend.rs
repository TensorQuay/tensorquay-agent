//! The hosted Responses backend reached through an account proxy, as observed: stricter than the documented API, and
//! answering with a flat `{"detail": "..."}` body instead of the documented error object.

use super::{Rule, Source, Violation, items, strict};
use serde_json::{Value, json};

pub(super) const PATH: &str = "/v1/responses";

const OBSERVED: &str = "observed on 27 Sep 2026";

/// `{"detail": "..."}`, as observed.
pub(super) fn error_body(_status: u16, violation: &Violation) -> Value {
    json!({"detail": violation.message})
}

pub(super) const RULES: &[Rule] = &[
    Rule {
        name: "no_system_input",
        source: Source::Observed(OBSERVED),
        status: 400,
        check: |body| {
            items(body, "input")
                .any(|item| item["role"] == "system")
                .then(|| Violation::new("System messages are not allowed"))
        },
    },
    Rule {
        name: "unsupported_parameters",
        source: Source::Observed(OBSERVED),
        status: 400,
        check: |body| {
            let parameter = ["metadata", "temperature"]
                .into_iter()
                .find(|parameter| body.get(*parameter).is_some())?;
            Some(Violation::new(format!(
                "Unsupported parameter: {parameter}"
            )))
        },
    },
    Rule {
        name: "strict_object_root",
        source: Source::Documented("structured outputs: the root schema is an object, not anyOf"),
        status: 400,
        check: |body| strict::first(body, strict::object_root),
    },
    Rule {
        name: "strict_supported_keywords",
        source: Source::Documented(
            "structured outputs: composition keywords such as allOf, not and if/then/else are not supported",
        ),
        status: 400,
        check: |body| strict::first(body, strict::supported_keywords),
    },
    Rule {
        name: "strict_closed_objects",
        source: Source::Documented(
            "structured outputs: additionalProperties must be false on every object",
        ),
        status: 400,
        check: |body| strict::first(body, strict::closed_objects),
    },
    Rule {
        name: "strict_all_required",
        source: Source::Observed(
            "observed on 27 Sep 2026: \"Invalid schema for function 'edit_file': In context=('properties', 'edits', 'items'), 'required' is required to be supplied and to be an array including every key in properties. Missing 'replace_all'.\"",
        ),
        status: 400,
        check: |body| strict::first(body, strict::all_required),
    },
];
