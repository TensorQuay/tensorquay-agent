//! Environment templates from `[[templates]]` (ARCHITECTURE §4.7), validated once at startup.
use std::collections::BTreeSet;

use super::sizes::{self, Size};
use crate::{GatewayError, InvalidReason};
use serde::Deserialize;
use tqa_sandbox::ImageRef;
use tqa_types::{Capability, Environment};

/// More templates than this is a configuration mistake, not a catalogue.
const MAX_TEMPLATES: usize = 32;
const MAX_DISPLAY_NAME_CHARS: usize = 64;
/// The most credential-file paths one template may acknowledge (ADR-0032).
const MAX_ACKNOWLEDGED: usize = 16;
const MAX_PATH_BYTES: usize = 512;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TemplateInput {
    name: String,
    display_name: String,
    /// The administrator's one-line purpose, shown where people pick an environment (GW-I-132); display only.
    #[serde(default)]
    description: Option<String>,
    image: String,
    capabilities: Vec<Capability>,
    #[serde(default)]
    default: bool,
    /// CPUs in steps of 0.5 (the environment sizes design); unset uses the installation's default.
    #[serde(default)]
    cpus: Option<f64>,
    #[serde(default)]
    memory_mib: Option<u64>,
    #[serde(default)]
    pids: Option<u32>,
    /// The disk of a workspace created for this environment; an existing workspace keeps its size.
    #[serde(default)]
    workspace_mib: Option<u64>,
    /// Credential files the template's image may hold (ADR-0032): found, they are warnings. File-only.
    #[serde(default)]
    acknowledged_secret_files: Vec<String>,
}

/// One validated environment template: its pinned image, declared capabilities and resource overrides.
#[derive(Clone, Debug)]
pub struct Template {
    environment: Environment,
    display_name: String,
    description: Option<String>,
    size: Size,
    is_default: bool,
    acknowledged_secret_files: BTreeSet<String>,
    /// Made from `[runtime] image` because the file has no `[[templates]]` (the quick start's Minimal): nobody chose it
    /// as the default, so the Workstation built-in may take the default from it.
    implicit: bool,
}

impl Template {
    /// The Minimal environment older runtimes assumed: no image pin, stored with an empty image.
    pub(crate) fn legacy_minimal() -> Self {
        Self {
            environment: Environment::default(),
            display_name: "Minimal".into(),
            description: None,
            size: Size::default(),
            is_default: true,
            acknowledged_secret_files: BTreeSet::new(),
            implicit: true,
        }
    }

    /// The template name, a lowercase slug.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.environment.template
    }
    /// The name shown to people.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    /// The administrator's one-line purpose, if any (GW-I-132).
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    /// Whether new conversations use this template when none is chosen.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.is_default
    }
    /// The environment snapshot a runtime receives: name, pinned digest and canonical capabilities.
    #[must_use]
    pub const fn environment(&self) -> &Environment {
        &self.environment
    }
    pub(crate) const fn cpu_millicores(&self) -> Option<u32> {
        self.size.cpu_millicores
    }
    pub(crate) const fn memory_bytes(&self) -> Option<u64> {
        self.size.memory_bytes
    }
    pub(crate) const fn workspace_bytes(&self) -> Option<u64> {
        self.size.workspace_bytes
    }
    /// The environment's own size; unset fields use the installation's defaults.
    pub(crate) const fn size(&self) -> Size {
        self.size
    }
    /// Made from `[runtime] image` for a file with no `[[templates]]`, not chosen by anyone.
    pub(crate) const fn implicit(&self) -> bool {
        self.implicit
    }
    /// The credential-file paths the install file acknowledges for this template (ADR-0032).
    pub(crate) const fn acknowledged_secret_files(&self) -> &BTreeSet<String> {
        &self.acknowledged_secret_files
    }
}

/// Validates the configured templates. Without any, the runtime image becomes the one default Minimal template.
///
/// # Errors
/// Rejects unpinned images, invalid or duplicate names, capabilities and resources, and anything but one default.
pub(super) fn templates(
    inputs: Vec<TemplateInput>,
    runtime_image: Option<&ImageRef>,
) -> Result<Vec<Template>, GatewayError> {
    if inputs.is_empty() {
        let Some(image) = runtime_image else {
            return Ok(vec![Template::legacy_minimal()]);
        };
        // The runtime's Minimal has python3 and its python link, as its label says (GW-U-101).
        let environment = Environment::new(
            "minimal".into(),
            image.to_string(),
            vec![Capability::Shell, Capability::Python],
        )
        .map_err(|_| invalid("runtime image is not a valid environment pin"))?;
        return Ok(vec![Template {
            environment,
            ..Template::legacy_minimal()
        }]);
    }
    if inputs.len() > MAX_TEMPLATES {
        return Err(SetRefusal::TemplateCount.start_up());
    }
    let templates = inputs
        .into_iter()
        .map(|input| checked(input).map_err(Refusal::start_up))
        .collect::<Result<Vec<_>, _>>()?;
    validate_set(&templates).map_err(SetRefusal::start_up)?;
    Ok(templates)
}

/// Why one template's definition is refused: its field and closed API reason, and a start-up message naming the same
/// reason code (GW-U-041).
#[derive(Clone, Copy, Debug)]
enum Refusal {
    Name,
    NameReserved,
    DisplayName,
    Description,
    Image,
    Capabilities,
    Cpus,
    Memory,
    Pids,
    Workspace,
    AcknowledgedSecretFiles,
}

impl Refusal {
    const fn api(self) -> (&'static str, InvalidReason) {
        match self {
            Self::Name => ("name", InvalidReason::NameInvalid),
            Self::NameReserved => ("name", InvalidReason::NameReserved),
            Self::DisplayName => ("display_name", InvalidReason::DisplayNameInvalid),
            Self::Description => ("description", InvalidReason::DescriptionInvalid),
            Self::Image => ("image", InvalidReason::ImageNotPinned),
            Self::Capabilities => ("capabilities", InvalidReason::CapabilitiesInvalid),
            Self::Cpus => ("cpus", InvalidReason::LimitOutOfRange),
            Self::Memory => ("memory_mib", InvalidReason::LimitOutOfRange),
            Self::Pids => ("pids", InvalidReason::LimitOutOfRange),
            Self::Workspace => ("workspace_mib", InvalidReason::LimitOutOfRange),
            Self::AcknowledgedSecretFiles => {
                ("acknowledged_secret_files", InvalidReason::ParameterInvalid)
            }
        }
    }

    const fn start_up(self) -> GatewayError {
        invalid(match self {
            Self::Name => "a template name is invalid (name_invalid)",
            Self::NameReserved => "the template name new is reserved (name_reserved)",
            Self::DisplayName => {
                "a template display_name must be 1 through 64 characters (display_name_invalid)"
            }
            Self::Description => {
                "a template description must be one line of 1 through 160 characters, without surrounding spaces (description_invalid)"
            }
            Self::Image => {
                "a template image must be pinned by digest, without credentials (image_not_pinned)"
            }
            Self::Capabilities => {
                "template capabilities must be at most five, without repeats (capabilities_invalid)"
            }
            Self::Cpus | Self::Memory | Self::Pids | Self::Workspace => {
                "template resources exceed supported bounds (limit_out_of_range)"
            }
            Self::AcknowledgedSecretFiles => {
                "template acknowledged_secret_files must be at most 16 absolute, normalised paths (parameter_invalid)"
            }
        })
    }
}

/// A rule over the whole template set. The file refuses it at start; through the API it is a conflict about the set,
/// which carries the same code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SetRefusal {
    /// Two templates share a name.
    NameTaken,
    /// Not exactly one template is the default.
    DefaultCount,
    /// More than [`MAX_TEMPLATES`].
    TemplateCount,
}

impl SetRefusal {
    const fn start_up(self) -> GatewayError {
        invalid(match self {
            Self::NameTaken => "template names must be unique (name_taken)",
            Self::DefaultCount => "exactly one template must be the default (default_count)",
            Self::TemplateCount => "at most 32 templates may be configured (template_count)",
        })
    }
}

/// One template's definition, from the file or an administrator's write: the same field and reason from both
/// (ADR-0028, GW-U-041). The shared protocol rules check the name, the capability set and the pin.
///
/// # Errors
/// The field and closed reason of the first invalid field.
pub(crate) fn validate(input: TemplateInput) -> Result<Template, (&'static str, InvalidReason)> {
    checked(input).map_err(Refusal::api)
}

/// The name no environment may take: the web's Add page lives at `/settings/environments/new` (GW-U-093).
const RESERVED_NAME: &str = "new";

fn checked(input: TemplateInput) -> Result<Template, Refusal> {
    if input.name == RESERVED_NAME {
        return Err(Refusal::NameReserved);
    }
    let characters = input.display_name.chars().count();
    if input.display_name.trim().is_empty() || characters > MAX_DISPLAY_NAME_CHARS {
        return Err(Refusal::DisplayName);
    }
    if input
        .description
        .as_deref()
        .is_some_and(|text| !crate::description::valid(text))
    {
        return Err(Refusal::Description);
    }
    let size = size(&input)?;
    let image = super::sandbox::image(&input.image).map_err(|_| Refusal::Image)?;
    let acknowledged_secret_files = acknowledged(input.acknowledged_secret_files)?;
    let environment =
        Environment::new(input.name, image.to_string(), input.capabilities).map_err(|error| {
            match error.path.as_str() {
                "environment.template" => Refusal::Name,
                "environment.capabilities" => Refusal::Capabilities,
                _ => Refusal::Image,
            }
        })?;
    Ok(Template {
        environment,
        display_name: input.display_name,
        description: input.description,
        size,
        is_default: input.default,
        acknowledged_secret_files,
        implicit: false,
    })
}

/// The environment's own size, each field within its bounds (the ceiling is the installation's, checked apart).
fn size(input: &TemplateInput) -> Result<Size, Refusal> {
    Ok(Size {
        cpu_millicores: input
            .cpus
            .map(|cpus| sizes::cpu_millicores(cpus).ok_or(Refusal::Cpus))
            .transpose()?,
        memory_bytes: input
            .memory_mib
            .map(|mib| sizes::mib_bytes(mib, &sizes::MEMORY_MIB).ok_or(Refusal::Memory))
            .transpose()?,
        pids: input
            .pids
            .map(|pids| {
                sizes::PIDS
                    .contains(&pids)
                    .then_some(pids)
                    .ok_or(Refusal::Pids)
            })
            .transpose()?,
        workspace_bytes: input
            .workspace_mib
            .map(|mib| sizes::mib_bytes(mib, &sizes::WORKSPACE_MIB).ok_or(Refusal::Workspace))
            .transpose()?,
    })
}

/// The acknowledged credential-file paths: at most 16, each absolute and normalised (no empty, `.` or `..` component,
/// no trailing slash), as the scan reports image paths.
fn acknowledged(paths: Vec<String>) -> Result<BTreeSet<String>, Refusal> {
    let normalised = |path: &str| {
        path.len() <= MAX_PATH_BYTES
            && !path.contains('\0')
            && path.strip_prefix('/').is_some_and(|rest| {
                rest.split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
            })
    };
    if paths.len() > MAX_ACKNOWLEDGED || !paths.iter().all(|path| normalised(path)) {
        return Err(Refusal::AcknowledgedSecretFiles);
    }
    Ok(paths.into_iter().collect())
}

/// The rules over the whole set: at most [`MAX_TEMPLATES`], unique names, exactly one default.
///
/// # Errors
/// The first broken rule.
pub(crate) fn validate_set(templates: &[Template]) -> Result<(), SetRefusal> {
    if templates.len() > MAX_TEMPLATES {
        return Err(SetRefusal::TemplateCount);
    }
    let mut names: Vec<&str> = templates.iter().map(Template::name).collect();
    names.sort_unstable();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(SetRefusal::NameTaken);
    }
    if templates
        .iter()
        .filter(|template| template.is_default)
        .count()
        != 1
    {
        return Err(SetRefusal::DefaultCount);
    }
    Ok(())
}

const fn invalid(message: &'static str) -> GatewayError {
    GatewayError::Configuration(message)
}

#[cfg(test)]
#[path = "templates_tests.rs"]
mod tests;
