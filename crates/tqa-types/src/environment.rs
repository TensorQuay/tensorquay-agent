//! Shared environment admission rules and canonical capability names.
mod validation;
use crate::FrameError;
use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::borrow::Cow;
pub(crate) use validation::{capabilities, capabilities_schema, template, template_schema};

/// A declared environment capability, ordered independently of registry input order.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Capability {
    /// Bounded shell execution.
    Shell,
    /// A Python toolchain.
    Python,
    /// A Node toolchain.
    Node,
    /// A native build toolchain.
    Build,
    /// A headless browser.
    Browser,
    /// Python's data libraries (pandas, matplotlib and openpyxl) installed, for tables, charts and spreadsheets.
    Data,
    /// A graphical desktop the agent sees and operates (revision 14, ADR-0039): a display, a window manager, desktop
    /// apps and a headed browser, with the `computer` tool.
    Desktop,
}

impl Capability {
    /// Returns the stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Python => "python",
            Self::Node => "node",
            Self::Build => "build",
            Self::Browser => "browser",
            Self::Data => "data",
            Self::Desktop => "desktop",
        }
    }
}

/// A pinned environment snapshot. Parsing validates it and normalizes capability order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Fields")]
pub struct Environment {
    /// Lowercase ASCII slug, one through 64 bytes, beginning with a letter.
    pub template: String,
    /// SHA-256 pin or pinned registry reference, at most 512 ASCII bytes.
    /// Empty only for the legacy Minimal environment with the shell capability.
    pub digest: String,
    /// Zero through seven unique declared capabilities in canonical enum order.
    pub capabilities: Vec<Capability>,
    /// The template version whose image the conversation was moved to, at least 1; never a digest (revision 8,
    /// ADR-0032 decision 7). Absent from older gateways and for the legacy Minimal default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    template: String,
    digest: String,
    capabilities: Vec<Capability>,
    #[serde(default, deserialize_with = "version")]
    version: Option<u64>,
}

impl TryFrom<Fields> for Environment {
    type Error = FrameError;
    fn try_from(fields: Fields) -> Result<Self, Self::Error> {
        Self::new(fields.template, fields.digest, fields.capabilities)?.with_version(fields.version)
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            template: "minimal".into(),
            digest: String::new(),
            capabilities: vec![Capability::Shell],
            version: None,
        }
    }
}

impl Environment {
    /// Validates a registry snapshot and normalizes its capability order without dropping duplicates.
    ///
    /// # Errors
    /// Rejects invalid template names, duplicate capabilities, and unpinned or credential-bearing references.
    pub fn new(
        template: String,
        digest: String,
        capabilities: Vec<Capability>,
    ) -> Result<Self, FrameError> {
        let mut value = Self {
            template,
            digest,
            capabilities,
            version: None,
        };
        value.validate()?;
        value.capabilities.sort_unstable();
        Ok(value)
    }

    /// Sets the template version, keeping the wire rules: at least 1, and only on a pinned image.
    ///
    /// # Errors
    /// Rejects version 0 and a version on the legacy Minimal default.
    pub fn with_version(mut self, version: Option<u64>) -> Result<Self, FrameError> {
        self.version = version;
        self.validate()?;
        Ok(self)
    }

    /// Applies the wire admission rules to a programmatically constructed snapshot.
    /// Call `new` to also normalize capability order.
    ///
    /// # Errors
    /// Rejects invalid names, duplicate capabilities or an invalid image pin.
    pub fn validate(&self) -> Result<(), FrameError> {
        validation::validate_template(&self.template)?;
        validation::validate_capabilities(&self.capabilities)?;
        if self.version == Some(0) {
            return Err(FrameError {
                code: crate::ErrorCode::InvalidFrame,
                path: "environment.version".into(),
                detail: "expected a template version of at least 1".into(),
            });
        }
        if !self.is_legacy() {
            validation::validate_digest(&self.digest)?;
        }
        Ok(())
    }

    /// Identifies the omitted-field default used by older gateways.
    #[must_use]
    pub fn is_legacy(&self) -> bool {
        self.template == "minimal"
            && self.digest.is_empty()
            && self.capabilities == [Capability::Shell]
            && self.version.is_none()
    }
}

impl JsonSchema for Environment {
    fn schema_name() -> Cow<'static, str> {
        "Environment".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type":"object", "additionalProperties":false,
            "required":["template","digest","capabilities"],
            "properties":{
                "template":template_schema(generator),
                "digest":{"type":"string","maxLength":512},
                "capabilities":capabilities_schema(generator),
                "version":version_schema(generator)
            },
            "anyOf":[
                {"properties":{"digest":{"type":"string","pattern":validation::DIGEST_PATTERN}}},
                {"properties":{"template":{"const":"minimal"},"digest":{"const":""},
                    "capabilities":{"const":["shell"]},"version":false}}
            ]
        })
    }
}

/// Parses a present template version: an integer of at least 1 (`null` is refused; leave the field out instead).
pub(crate) fn version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    match u64::deserialize(deserializer) {
        Ok(version) if version >= 1 => Ok(Some(version)),
        _ => Err(D::Error::custom(
            "version: expected a template version of at least 1",
        )),
    }
}
pub(crate) fn version_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"integer","format":"uint64","minimum":1})
}
