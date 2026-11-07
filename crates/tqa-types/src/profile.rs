//! The agent profile a session runs under (revision 8, ADR-0012 decision 6): its name, extra instructions, the built-in
//! tools it may use and its skills. Only the gateway sends it, and only to runtimes at revision 8 or later; connectors
//! and MCP servers stay in `mcp`, so no `mcp.` name is a profile tool.

use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::borrow::Cow;

use crate::{
    MAX_PROFILE_INSTRUCTIONS_BYTES, MAX_PROFILE_NAME_BYTES, MAX_PROFILE_SKILLS, MAX_PROFILE_TOOLS,
    MAX_SKILL_NAME_BYTES, PROFILE_NAME_PATTERN, SKILL_NAME_PATTERN, tool_name::BUILTIN_PATTERN,
    valid_builtin_tool_name,
};

/// A profile as the runtime applies it. Parsing validates every bound and refuses unknown fields.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Fields")]
pub struct ProfileSnapshot {
    /// The profile's name under [`PROFILE_NAME_PATTERN`].
    pub name: String,
    /// Extra instructions for the system prompt: 1 through 16 KiB of UTF-8, no NUL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// The built-in tools the session may use, at most 64 distinct names; `None` is every built-in tool and an empty
    /// list is none of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// Which skills the session may load; always written, and every skill when absent on input.
    pub skills: SkillsFilter,
}

/// Which skills a profile allows: `"all"`, `"none"` or a list of 1 through 64 distinct skill names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum SkillsFilter {
    /// Every skill.
    #[default]
    All,
    /// No skill.
    None,
    /// Only these skills, each under [`SKILL_NAME_PATTERN`].
    Only(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    name: String,
    #[serde(default, deserialize_with = "present")]
    instructions: Option<String>,
    #[serde(default, deserialize_with = "present")]
    tools: Option<Vec<String>>,
    #[serde(default)]
    skills: SkillsFilter,
}

impl TryFrom<Fields> for ProfileSnapshot {
    type Error = String;
    fn try_from(fields: Fields) -> Result<Self, Self::Error> {
        let value = Self {
            name: fields.name,
            instructions: fields.instructions,
            tools: fields.tools,
            skills: fields.skills,
        };
        value.check()?;
        Ok(value)
    }
}

impl ProfileSnapshot {
    /// Applies the wire rules to a programmatically built snapshot.
    ///
    /// # Errors
    /// Returns the first rule the snapshot breaks, naming the field.
    pub fn check(&self) -> Result<(), String> {
        if !valid_profile_name(&self.name) {
            return Err(format!("name: expected {PROFILE_NAME_PATTERN}"));
        }
        if let Some(text) = &self.instructions
            && (text.is_empty()
                || text.len() > MAX_PROFILE_INSTRUCTIONS_BYTES
                || text.contains('\0'))
        {
            return Err("instructions: expected 1 through 16384 bytes without NUL".into());
        }
        if let Some(tools) = &self.tools
            && (tools.len() > MAX_PROFILE_TOOLS
                || !tools.iter().all(|tool| valid_builtin_tool_name(tool))
                || has_duplicates(tools))
        {
            return Err("tools: expected at most 64 distinct built-in tool names".into());
        }
        if let SkillsFilter::Only(skills) = &self.skills
            && (skills.is_empty()
                || skills.len() > MAX_PROFILE_SKILLS
                || !skills.iter().all(|skill| valid_skill_name(skill))
                || has_duplicates(skills))
        {
            return Err(
                "skills: expected \"all\", \"none\" or 1 through 64 distinct skill names".into(),
            );
        }
        Ok(())
    }
}

/// Whether a profile name follows [`PROFILE_NAME_PATTERN`].
#[must_use]
pub fn valid_profile_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() <= MAX_PROFILE_NAME_BYTES
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

/// Whether a skill name follows [`SKILL_NAME_PATTERN`].
#[must_use]
pub fn valid_skill_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=MAX_SKILL_NAME_BYTES).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// A present optional field holds a value; `null` is refused (leave the field out instead).
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

fn has_duplicates(names: &[String]) -> bool {
    names
        .iter()
        .enumerate()
        .any(|(index, name)| names[..index].contains(name))
}

impl Serialize for SkillsFilter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_str("all"),
            Self::None => serializer.serialize_str("none"),
            Self::Only(skills) => skills.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for SkillsFilter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Word(String),
            List(Vec<String>),
        }
        match Wire::deserialize(deserializer)
            .map_err(|_| D::Error::custom("skills: expected \"all\", \"none\" or a list"))?
        {
            Wire::Word(word) if word == "all" => Ok(Self::All),
            Wire::Word(word) if word == "none" => Ok(Self::None),
            Wire::Word(_) => Err(D::Error::custom("skills: expected \"all\" or \"none\"")),
            Wire::List(skills) => Ok(Self::Only(skills)),
        }
    }
}

impl JsonSchema for ProfileSnapshot {
    fn schema_name() -> Cow<'static, str> {
        "ProfileSnapshot".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // JSON Schema counts characters, so the 16 KiB byte bound is a character bound here; the parser checks bytes.
        json_schema!({
            "type":"object","additionalProperties":false,"required":["name"],
            "properties":{
                "name":{"type":"string","pattern":PROFILE_NAME_PATTERN},
                "instructions":{"type":"string","minLength":1,"maxLength":MAX_PROFILE_INSTRUCTIONS_BYTES,
                    "pattern":"^[^\\u0000]*$"},
                "tools":{"type":"array","maxItems":MAX_PROFILE_TOOLS,"uniqueItems":true,
                    "items":{"type":"string","pattern":BUILTIN_PATTERN}},
                "skills":{"anyOf":[
                    {"type":"string","enum":["all","none"]},
                    {"type":"array","minItems":1,"maxItems":MAX_PROFILE_SKILLS,"uniqueItems":true,
                        "items":{"type":"string","pattern":SKILL_NAME_PATTERN}}
                ]}
            }
        })
    }
}

/// The schema of the optional profile field: present means an object, never `null`.
pub(crate) fn profile_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<ProfileSnapshot>()
}

/// Parses an optional profile, naming the field in every refusal.
pub(crate) fn profile<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ProfileSnapshot>, D::Error> {
    ProfileSnapshot::deserialize(deserializer)
        .map(Some)
        .map_err(|error| D::Error::custom(format!("profile: {error}")))
}
