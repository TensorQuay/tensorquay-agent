//! The API reasons for tqa-llm's route validator (ADR-0028): each typed reason maps 1:1, by its stable code, to a
//! closed API reason with the same code, so the file and the API refuse a route for the same reason (GW-U-031).
use crate::{GatewayError, InvalidReason};

/// The route fields each reason names. A reason tqa-llm adds later is `route_invalid` until it is named here and in
/// the API's closed list.
const FIELDS: [(InvalidReason, &str); 23] = [
    (InvalidReason::RouteNameInvalid, "name"),
    // The environments' reason too (GW-U-093): the name is a page's address.
    (InvalidReason::NameReserved, "name"),
    (InvalidReason::ModelEmpty, "model"),
    (InvalidReason::BaseUrlInvalid, "base_url"),
    (InvalidReason::KeyRefRequired, "key_ref"),
    (InvalidReason::KeyRefNotReference, "key_ref"),
    (InvalidReason::KeyRefReserved, "key_ref"),
    (InvalidReason::AuthHeaderInvalid, "auth"),
    (InvalidReason::AuthQueryInvalid, "auth"),
    (InvalidReason::ReasoningInvalid, "reasoning"),
    (InvalidReason::BudgetInvalid, "reasoning"),
    (InvalidReason::MaxOutputRequired, "max_output"),
    (InvalidReason::LimitOutOfRange, "limits"),
    (InvalidReason::ExtraBodyReserved, "extra_body"),
    (InvalidReason::ExtraHeaderInvalid, "extra_headers"),
    (InvalidReason::ExtraHeaderReserved, "extra_headers"),
    (InvalidReason::ExtraHeaderCredential, "extra_headers"),
    (InvalidReason::QuirkUnsupported, "quirks"),
    (InvalidReason::StrictInvalid, "strict"),
    (InvalidReason::SchemaKeywordsInvalid, "schema_keywords"),
    (InvalidReason::InstructionsRoleInvalid, "instructions_role"),
    (InvalidReason::FirstResponseInvalid, "first_response_secs"),
    (InvalidReason::FallbackInvalid, "fallbacks"),
];

/// The API's refusal for the validator's error; a refusal without a route rule (a duplicate name) is `route_invalid`.
pub(crate) fn refusal(error: &tqa_llm::RegistryError) -> GatewayError {
    let (field, reason) = error
        .reason()
        .map_or(("route", InvalidReason::RouteInvalid), |reason| {
            reason_of(reason.code())
        });
    GatewayError::Invalid { field, reason }
}

/// The field and API reason for a validator reason's stable code.
pub(crate) fn reason_of(code: &str) -> (&'static str, InvalidReason) {
    FIELDS
        .iter()
        .find(|(reason, _)| reason.as_str() == code)
        .map_or(("route", InvalidReason::RouteInvalid), |(reason, field)| {
            (*field, *reason)
        })
}

#[cfg(test)]
#[path = "route_reasons_tests.rs"]
mod tests;
