//! A profile conversation's effective limits (ADR-0012 decision 4), merged with the install's at each session so the
//! install's later changes apply, and the runtime's part of the profile. A profile only narrows.
use tqa_types::{ProfileSnapshot, RunLimits, SkillsFilter};

use super::document::{Limits, Profile, Skills};
use crate::GatewayError;

#[cfg(test)]
#[path = "effective_tests.rs"]
mod tests;

/// Each run limit is the smaller of the install's and the profile's.
pub(crate) fn limits(install: RunLimits, profile: &Limits) -> RunLimits {
    RunLimits {
        max_steps: profile
            .max_steps
            .map_or(install.max_steps, |own| own.min(install.max_steps)),
        wall_clock_secs: profile
            .wall_clock_secs
            .map_or(install.wall_clock_secs, |own| {
                own.min(install.wall_clock_secs)
            }),
        tool_result_bytes: profile
            .tool_result_bytes
            .map_or(install.tool_result_bytes, |own| {
                own.min(install.tool_result_bytes)
            }),
        max_output_tokens: profile
            .max_output_tokens
            .map_or(install.max_output_tokens, |own| {
                own.min(install.max_output_tokens)
            }),
    }
}

/// What a conversation runs with: the install's limits, merged with its profile copy when it has one, and what only
/// the runtime can apply of the profile.
///
/// # Errors
/// A copy this gateway cannot read or send: the session then fails closed.
pub(crate) fn effective(
    install: RunLimits,
    copy: Option<serde_json::Value>,
) -> Result<(RunLimits, Option<ProfileSnapshot>), GatewayError> {
    let Some(copy) = copy else {
        return Ok((install, None));
    };
    let profile: Profile = serde_json::from_value(copy)?;
    let snapshot = snapshot(&profile)?;
    Ok((limits(install, &profile.limits), Some(snapshot)))
}

/// The tools a profile's runs may have: the runtime's own set when the profile names none, else those it names.
fn tools(profile: &Profile) -> Option<Vec<String>> {
    profile.tools.clone()
}

/// The runtime's part of a profile: its name, instructions, built-in tool allow-list and skills filter.
///
/// # Errors
/// An internal error for a copy outside the wire's bounds, which the validator prevents.
pub(crate) fn snapshot(profile: &Profile) -> Result<ProfileSnapshot, GatewayError> {
    let snapshot = ProfileSnapshot {
        name: profile.name.clone(),
        instructions: profile.instructions.clone(),
        tools: tools(profile),
        skills: match &profile.skills {
            Skills::All => SkillsFilter::All,
            Skills::None => SkillsFilter::None,
            Skills::Only(names) => SkillsFilter::Only(names.clone()),
        },
    };
    snapshot.check().map_err(|_| {
        GatewayError::Configuration("a stored profile is outside the protocol's bounds")
    })?;
    Ok(snapshot)
}
