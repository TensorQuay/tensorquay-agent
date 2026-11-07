//! An agent profile's definition (ADR-0012, GW-U-049): one validator for the file (`[[profiles]]`) and the API, with
//! field-level reasons. Only syntax and bounds are checked here; whether a named route, template, server or connector
//! exists depends on the install and is checked where the profile is written and used.
use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize};
use tqa_tools::builtin::profile_tool;

use crate::{InvalidReason, config::BusyPolicy};

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;

const MAX_DISPLAY_NAME_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 512;
// The bounds and name rules the runtime checks on the wire (revision 8), so the gateway never stores what it cannot
// send: instructions are a prompt section, never a document.
use tqa_types::{
    MAX_PROFILE_INSTRUCTIONS_BYTES as MAX_INSTRUCTIONS_BYTES,
    MAX_PROFILE_NAME_BYTES as MAX_NAME_BYTES, MAX_PROFILE_SKILLS as MAX_SKILLS,
    MAX_PROFILE_TOOLS as MAX_TOOLS, valid_profile_name, valid_skill_name,
};
const MAX_ROUTE_NAME_BYTES: usize = 128;
/// A conversation holds at most this many servers and connectors together (G6), so a profile names no more.
const MAX_SERVERS_AND_CONNECTORS: usize = tqa_types::MAX_MCP_SERVERS;

/// Which discovered skills a profile's runs see.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Skills {
    /// Every discovered skill.
    #[default]
    All,
    /// No skill at all.
    None,
    /// Only the named skills.
    #[serde(untagged)]
    Only(Vec<String>),
}

impl<'de> Deserialize<'de> for Skills {
    /// `"all"`, `"none"`, or a list of skill names.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Word(String),
            Names(Vec<String>),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Word(word) if word == "all" => Ok(Self::All),
            Raw::Word(word) if word == "none" => Ok(Self::None),
            Raw::Word(_) => Err(serde::de::Error::custom(
                "expected all, none or a list of skill names",
            )),
            Raw::Names(names) => Ok(Self::Only(names)),
        }
    }
}

/// The run limits a profile may lower; each is a ceiling under the install's (ADR-0012 decision 4).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Limits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_clock_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result_bytes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

/// A profile as written, before validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileInput {
    name: String,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    model_route: Option<String>,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    skills: Skills,
    #[serde(default)]
    mcp_servers: Vec<String>,
    #[serde(default)]
    connectors: Vec<String>,
    #[serde(default)]
    decision_engine: Option<String>,
    #[serde(default)]
    limits: Limits,
    #[serde(default)]
    busy_policy: Option<BusyPolicy>,
    /// Removed with `git_push` (ADR-0037 §7): read only so a profile that still sets it is refused by name.
    #[serde(default)]
    repositories: Option<serde::de::IgnoredAny>,
}

/// A validated profile; read back only from what this gateway stored.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Profile {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub model_route: Option<String>,
    pub template: Option<String>,
    pub tools: Option<Vec<String>>,
    pub skills: Skills,
    pub mcp_servers: Vec<String>,
    pub connectors: Vec<String>,
    /// The decision engine its conversations consult (ADR-0013 point 6); none by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_engine: Option<String>,
    pub limits: Limits,
    pub busy_policy: Option<BusyPolicy>,
}

type Refusal = (&'static str, InvalidReason);

/// Free text within `max` characters (or bytes) and without NUL.
fn text(
    field: &'static str,
    value: Option<&str>,
    length: usize,
    max: usize,
) -> Result<(), Refusal> {
    match value {
        Some(value) if value.contains('\0') => Err((field, InvalidReason::ValueNul)),
        Some(_) if length > max => Err((field, InvalidReason::ValueTooLong)),
        _ => Ok(()),
    }
}

/// Distinct names, at most `max`, each accepted by `valid`.
fn names(
    field: &'static str,
    list: &[String],
    max: usize,
    valid: fn(&str) -> bool,
) -> Result<(), Refusal> {
    let distinct: BTreeSet<&String> = list.iter().collect();
    if list.len() > max || distinct.len() != list.len() || !list.iter().all(|name| valid(name)) {
        return Err((field, InvalidReason::ListInvalid));
    }
    Ok(())
}

fn route(name: Option<&str>) -> Result<(), Refusal> {
    if name.is_some_and(|name| {
        name.is_empty() || name.len() > MAX_ROUTE_NAME_BYTES || name.chars().any(char::is_control)
    }) {
        return Err(("model_route", InvalidReason::RouteNameInvalid));
    }
    Ok(())
}

fn tools(list: Option<&[String]>) -> Result<(), Refusal> {
    let Some(list) = list else {
        return Ok(());
    };
    names("tools", list, MAX_TOOLS, |_| true)?;
    if !list.iter().all(|name| profile_tool(name)) {
        return Err(("tools", InvalidReason::UnknownReference));
    }
    Ok(())
}

fn limits(limits: &Limits) -> Result<(), Refusal> {
    let out = |field| Err((field, InvalidReason::LimitOutOfRange));
    if limits
        .max_steps
        .is_some_and(|value| !(1..=500).contains(&value))
    {
        return out("limits.max_steps");
    }
    if limits
        .wall_clock_secs
        .is_some_and(|value| !(1..=86_400).contains(&value))
    {
        return out("limits.wall_clock_secs");
    }
    if limits
        .tool_result_bytes
        .is_some_and(|value| !(1024..=262_144).contains(&value))
    {
        return out("limits.tool_result_bytes");
    }
    if limits
        .max_output_tokens
        .is_some_and(|value| !(1..=16_777_216).contains(&value))
    {
        return out("limits.max_output_tokens");
    }
    Ok(())
}

/// The template and decision engine a profile names, checked as names only (whether they exist is checked against the
/// snapshot when the profile is used).
fn references(template: Option<&str>, engine: Option<&str>) -> Result<(), Refusal> {
    // Template names follow the same slug rule as profile names.
    if template.is_some_and(|name| !valid_profile_name(name)) {
        return Err(("template", InvalidReason::NameInvalid));
    }
    if engine.is_some_and(|name| !crate::decision_engines::valid_engine_name(name)) {
        return Err(("decision_engine", InvalidReason::NameInvalid));
    }
    Ok(())
}

/// A profile's definition, from the file or the API: the first invalid field and its closed reason (GW-U-049).
///
/// # Errors
/// The field and reason of the first invalid field.
pub(crate) fn validate(input: ProfileInput) -> Result<Profile, Refusal> {
    if !valid_profile_name(&input.name) {
        return Err(("name", InvalidReason::NameInvalid));
    }
    let display = input.display_name.chars().count();
    if input.display_name.trim().is_empty() || display > MAX_DISPLAY_NAME_CHARS {
        return Err(("display_name", InvalidReason::DisplayNameInvalid));
    }
    let description = input.description.as_deref();
    text(
        "description",
        description,
        description.map_or(0, |value| value.chars().count()),
        MAX_DESCRIPTION_CHARS,
    )?;
    let instructions = input.instructions.as_deref();
    // Left out, never sent empty (revision 8's snapshot refuses an empty string).
    if instructions.is_some_and(str::is_empty) {
        return Err(("instructions", InvalidReason::ValueEmpty));
    }
    text(
        "instructions",
        instructions,
        instructions.map_or(0, str::len),
        MAX_INSTRUCTIONS_BYTES,
    )?;
    if input.repositories.is_some() {
        return Err(("repositories", InvalidReason::Removed));
    }
    route(input.model_route.as_deref())?;
    references(input.template.as_deref(), input.decision_engine.as_deref())?;
    tools(input.tools.as_deref())?;
    if let Skills::Only(list) = &input.skills {
        // "none" is the one spelling of no skills (revision 8's snapshot refuses an empty list).
        if list.is_empty() {
            return Err(("skills", InvalidReason::ListInvalid));
        }
        names("skills", list, MAX_SKILLS, valid_skill_name)?;
    }
    names(
        "mcp_servers",
        &input.mcp_servers,
        MAX_SERVERS_AND_CONNECTORS,
        |name| !name.is_empty() && name.len() <= MAX_NAME_BYTES,
    )?;
    names(
        "connectors",
        &input.connectors,
        MAX_SERVERS_AND_CONNECTORS,
        |name| !name.is_empty() && name.len() <= MAX_NAME_BYTES,
    )?;
    if input.mcp_servers.len() + input.connectors.len() > MAX_SERVERS_AND_CONNECTORS {
        return Err(("connectors", InvalidReason::ListInvalid));
    }
    limits(&input.limits)?;
    Ok(Profile {
        name: input.name,
        display_name: input.display_name,
        description: input.description,
        instructions: input.instructions,
        model_route: input.model_route,
        template: input.template,
        tools: input.tools,
        skills: input.skills,
        mcp_servers: input.mcp_servers,
        connectors: input.connectors,
        decision_engine: input.decision_engine,
        limits: input.limits,
        busy_policy: input.busy_policy,
    })
}
