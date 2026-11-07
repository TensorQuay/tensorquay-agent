//! Built-in environments (the built-in environments design): the images on the engine host that the install built. A
//! built-in is the image its canonical tag (`tqa-runtime:<name>`) names, when that image's environment label agrees: an
//! image built FROM Workstation inherits the label but never the tag, so it can't take a built-in's place. Every image
//! with one of our labels is also classified, with plain reasons, for the administrator's image list.
use async_trait::async_trait;
use serde::Serialize;
use tqa_types::{Capability, MIN_RUNTIME_REVISION, RUNTIME_REVISION_LABEL, parse_revision};

use crate::config::BuiltinImages;

/// The built-in environments, in the order they are listed.
pub(crate) const BUILT_INS: [&str; 4] = ["minimal", "workstation", "data", "desktop"];
const ENVIRONMENT: &str = "org.tensorquay.environment";
const NAME: &str = "org.tensorquay.environment.name";
const CAPABILITIES: &str = "org.tensorquay.environment.capabilities";
/// The built-in a recipe image (deploy/build-environment.sh) is built from.
const BASE: &str = "org.tensorquay.environment.base";

/// One image as the engine reports it.
pub(crate) type EngineImage = tqa_sandbox::docker::ImageRecord;

/// The container engine didn't answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineUnavailable;

/// The environment images on the engine host: tqa-sandbox's listing, or a fake in tests. Never pulls.
#[async_trait]
pub trait EnvironmentImageSource: Send + Sync {
    /// The images carrying the runtime's revision label, newest first.
    ///
    /// # Errors
    /// The engine didn't answer.
    async fn list(&self) -> Result<Vec<EngineImage>, EngineUnavailable>;
}

#[async_trait]
impl EnvironmentImageSource for tqa_sandbox::docker::EnvironmentImages {
    async fn list(&self) -> Result<Vec<EngineImage>, EngineUnavailable> {
        Self::list(self).await.map_err(|error| {
            tracing::debug!(code = error.code(), "the engine's image list failed");
            EngineUnavailable
        })
    }
}

/// A built-in environment found on the host.
#[derive(Clone, Debug)]
pub(crate) struct BuiltIn {
    pub name: &'static str,
    pub display_name: String,
    pub capabilities: Vec<Capability>,
    pub image: String,
    pub created: i64,
}

/// A built-in's own size where the installation's defaults don't suit it: its CPU in millicores, its memory in bytes and
/// its process limit. The registration gives it to the built-in, and the image list says it, so an environment started
/// from the built-in's card takes it too (GW-U-197).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnSize {
    pub cpu_millicores: i32,
    pub memory_bytes: i64,
    pub pids: i32,
}

impl OwnSize {
    /// As the image list says it: CPUs, memory in MiB and processes.
    pub(crate) fn listed(self) -> serde_json::Value {
        serde_json::json!({
            "cpus": f64::from(self.cpu_millicores) / 1000.0,
            "memory_mib": self.memory_bytes >> 20,
            "pids": self.pids,
        })
    }
}

/// The built-in's own size, when it has one. Desktop's (ADR-0039 section 10): a display and a headed browser need more
/// than the defaults; like every environment's size, one above the installation's ceiling is lowered to it at start and
/// recorded (`clamped_from`). The other built-ins take the defaults.
pub(crate) fn own_size(name: &str) -> Option<OwnSize> {
    (name == "desktop").then_some(OwnSize {
        cpu_millicores: 2000,
        memory_bytes: 4 << 30,
        pids: 1024,
    })
}

/// Whether an image holds a runtime this gateway can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Runtime {
    Current,
    TooOld,
    None,
}

/// What the image list says about one image.
#[derive(Clone, Debug)]
pub(crate) struct Classified {
    pub environment: Option<String>,
    pub display_name: Option<String>,
    pub capabilities: Vec<String>,
    pub capabilities_valid: bool,
    pub runtime: Runtime,
    /// The built-in whose label this image carries without being it: another build, or an image built FROM it.
    pub based_on: Option<String>,
    pub usable: bool,
    pub reason: Option<String>,
}

/// The built-ins on the host: for each name, the image its canonical tag names, when that image's label agrees, its
/// runtime is current and its capabilities are valid.
pub(crate) fn built_ins(images: &[EngineImage], pinned: &BuiltinImages) -> Vec<BuiltIn> {
    BUILT_INS
        .iter()
        .filter_map(|&name| {
            let image = images
                .iter()
                .find(|image| is_built_in(image, name, pinned))?;
            let classified = classify(image, pinned);
            let capabilities = capabilities(image.labels.get(CAPABILITIES)).ok()?;
            (classified.usable && classified.environment.as_deref() == Some(name)).then(|| {
                BuiltIn {
                    name,
                    display_name: classified.display_name.unwrap_or_else(|| name.to_owned()),
                    capabilities,
                    image: image.id.clone(),
                    created: image.created,
                }
            })
        })
        .collect()
}

/// The built-ins `[runtime.builtin_images]` pins to an image the engine doesn't hold: registration says so, since such
/// a built-in is not added (its tag never stands in).
pub(crate) fn missing_pins(images: &[EngineImage], pinned: &BuiltinImages) -> Vec<&'static str> {
    BUILT_INS
        .iter()
        .copied()
        .filter(|name| {
            pinned
                .get(name)
                .is_some_and(|id| !images.iter().any(|image| image.id == id))
        })
        .collect()
}

/// The image list's view of one image.
pub(crate) fn classify(image: &EngineImage, pinned: &BuiltinImages) -> Classified {
    let label = |key: &str| {
        image
            .labels
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
    };
    let environment = label(ENVIRONMENT);
    let runtime = match image.labels.get(RUNTIME_REVISION_LABEL) {
        None => Runtime::None,
        Some(value) => match parse_revision(Some(value)) {
            Ok(revision) if revision >= MIN_RUNTIME_REVISION => Runtime::Current,
            _ => Runtime::TooOld,
        },
    };
    let listed: Vec<String> = image
        .labels
        .get(CAPABILITIES)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let checked = environment
        .as_ref()
        .map(|_| capabilities(image.labels.get(CAPABILITIES)));
    let reason = match (runtime, &checked) {
        (Runtime::None, _) => Some("there is no TensorQuay runtime in it".to_owned()),
        (Runtime::TooOld, _) => Some("too old for this version: rebuild it".to_owned()),
        (Runtime::Current, Some(Err(reason))) => Some(reason.clone()),
        (Runtime::Current, _) => None,
    };
    // An image carrying a built-in's own label without its tag (built FROM it, or an earlier build), or a recipe image
    // naming the built-in it starts from.
    let based_on = match environment.as_deref() {
        Some(name) if BUILT_INS.contains(&name) => (!is_built_in(image, name, pinned))
            .then(|| label(NAME).unwrap_or_else(|| name.to_owned())),
        _ => label(BASE)
            .filter(|base| BUILT_INS.contains(&base.as_str()))
            .map(|base| display(&base)),
    };
    Classified {
        display_name: label(NAME),
        environment,
        capabilities: listed,
        capabilities_valid: !matches!(checked, Some(Err(_))),
        runtime,
        based_on,
        usable: reason.is_none(),
        reason,
    }
}

/// A built-in's name for people: Minimal, Workstation, Data.
fn display(name: &str) -> String {
    let mut letters = name.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// Whether `image` is the built-in `name`: the image pinned by `[runtime.builtin_images]` when there is one (never the
/// tag then, GW-U-120), otherwise the image its canonical tag names.
fn is_built_in(image: &EngineImage, name: &str, pinned: &BuiltinImages) -> bool {
    pinned.get(name).map_or_else(
        || image.tags.contains(&canonical_tag(name)),
        |id| image.id == id,
    )
}

fn canonical_tag(name: &str) -> String {
    format!("tqa-runtime:{name}")
}

/// The capability label as capabilities: at least one, each one known.
fn capabilities(value: Option<&String>) -> Result<Vec<Capability>, String> {
    let names: Vec<&str> = value
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if names.is_empty() {
        return Err("its capability label names no capability".to_owned());
    }
    names
        .into_iter()
        .map(|name| {
            serde_json::from_value::<Capability>(serde_json::Value::String(name.to_owned()))
                .map_err(|_| format!("its capability label names an unknown capability: {name}"))
        })
        .collect()
}

#[cfg(test)]
#[path = "builtin_tests.rs"]
mod tests;
