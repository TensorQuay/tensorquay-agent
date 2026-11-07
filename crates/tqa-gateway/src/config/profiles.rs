//! Shared agent profiles from `[[profiles]]` (ADR-0012), validated at start-up with the API's validator: an invalid
//! entry stops start-up naming the same reason code the API gives (GW-U-049). Whether the names it uses exist is
//! checked when the seed is imported, once the settings are loaded (GW-I-088).
use std::collections::BTreeSet;

use crate::{
    GatewayError, InvalidReason,
    profiles::{MAX_SHARED_PROFILES, Profile, ProfileInput, validate},
};

#[cfg(test)]
#[path = "profiles_tests.rs"]
mod tests;

/// The start-up message for one refused entry: the API's reason code, which is what an operator searches for.
const fn refusal(reason: InvalidReason) -> &'static str {
    match reason {
        InvalidReason::NameInvalid => "a [[profiles]] entry is invalid (name_invalid)",
        InvalidReason::DisplayNameInvalid => {
            "a [[profiles]] entry is invalid (display_name_invalid)"
        }
        InvalidReason::ValueTooLong => "a [[profiles]] entry is invalid (value_too_long)",
        InvalidReason::ValueNul => "a [[profiles]] entry is invalid (value_nul)",
        InvalidReason::ValueEmpty => "a [[profiles]] entry is invalid (value_empty)",
        InvalidReason::RouteNameInvalid => "a [[profiles]] entry is invalid (route_name_invalid)",
        InvalidReason::ListInvalid => "a [[profiles]] entry is invalid (list_invalid)",
        InvalidReason::UnknownReference => "a [[profiles]] entry is invalid (unknown_reference)",
        InvalidReason::LimitOutOfRange => "a [[profiles]] entry is invalid (limit_out_of_range)",
        _ => "a [[profiles]] entry is invalid",
    }
}

/// The file's shared profiles: each valid, names unique, at most the install's shared bound.
///
/// # Errors
/// The first invalid entry's reason code; repeated names (`name_taken`); too many (`profile_count`).
pub(super) fn profiles(inputs: Vec<ProfileInput>) -> Result<Vec<Profile>, GatewayError> {
    if i64::try_from(inputs.len()).unwrap_or(i64::MAX) > MAX_SHARED_PROFILES {
        return Err(GatewayError::Configuration(
            "at most 64 shared profiles may be configured (profile_count)",
        ));
    }
    let profiles = inputs
        .into_iter()
        .map(|input| {
            validate(input).map_err(|(_, reason)| GatewayError::Configuration(refusal(reason)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let names: BTreeSet<&str> = profiles
        .iter()
        .map(|profile| profile.name.as_str())
        .collect();
    if names.len() != profiles.len() {
        return Err(GatewayError::Configuration(
            "profile names in [[profiles]] must be unique (name_taken)",
        ));
    }
    Ok(profiles)
}
