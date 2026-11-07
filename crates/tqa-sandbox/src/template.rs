//! Custom template registration checks that need no engine (ADR-0032): derivation from a first-party base, and the
//! image facts that decide whether an image may become a template. The engine-side checks (the runtime binary and the
//! microVM rootfs) and the order around these belong to their callers.
use std::collections::BTreeSet;

use bollard::models::ImageInspect;
use tqa_types::{RUNTIME_REVISION_LABEL, check_runtime_revision, parse_revision};

use crate::{SandboxError, docker::image_policy::unset_environment};

mod runtime_binary;
mod scan_image;
pub mod secret_files;
#[cfg(test)]
mod tests;
mod verify;

pub use runtime_binary::{
    ImageFiles, MAX_RUNTIME_BINARY_BYTES, RUNTIME_BINARY, RuntimeBinaryError, runtime_sha256,
};
pub use verify::{Failure, ImageSource, RootfsSource, Verdict, VerifyRequest, verify};

/// Base images a template may derive from: each file-provided template's image (ADR-0032), so as many as templates.
pub const MAX_BASE_IMAGES: usize = 32;

/// A first-party base image: its layer chain and the entrypoint that starts the runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseImage {
    name: String,
    layers: Vec<String>,
    entrypoint: Vec<String>,
}

impl BaseImage {
    /// A base named `name` (the first-party template it is) with its layer diff IDs and entrypoint.
    #[must_use]
    pub fn new(name: &str, layers: Vec<String>, entrypoint: Vec<String>) -> Self {
        Self {
            name: name.to_owned(),
            layers,
            entrypoint,
        }
    }
    /// How many layers the base has: a derived image's own layers follow them.
    #[must_use]
    pub const fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// The first-party template this base is.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The validated set of base images an install accepts.
#[derive(Clone, Debug)]
pub struct Bases(Vec<BaseImage>);

impl Bases {
    /// Validates the bases.
    ///
    /// # Errors
    /// `InvalidSpec` on `bases`: none, more than [`MAX_BASE_IMAGES`], or a base without layers.
    pub fn new(bases: Vec<BaseImage>) -> Result<Self, SandboxError> {
        if bases.is_empty()
            || bases.len() > MAX_BASE_IMAGES
            || bases.iter().any(|base| base.layers.is_empty())
        {
            return Err(SandboxError::invalid(
                "bases",
                "one to 32 base images, each with layers, are required",
            ));
        }
        Ok(Self(bases))
    }
}

/// Why an image may not become a template. Each has a stable code shared by the admin API and the seed (ADR-0032).
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TemplateRefusal {
    /// The image's layers do not begin with every layer of a first-party base.
    #[error("the image does not derive from a first-party base image")]
    NotDerived,
    /// The runtime inside speaks an older protocol than this gateway accepts (ADR-0026).
    #[error("the image's runtime speaks protocol revision {has}; at least {needs} is needed")]
    RuntimeTooOld {
        /// The lowest revision accepted.
        needs: u16,
        /// The image's revision.
        has: u16,
    },
    /// The protocol revision label is not a revision.
    #[error("the runtime protocol revision label is malformed")]
    RuntimeLabel,
    /// The image declares volumes, which would add mounts nobody approved.
    #[error("image-declared volumes would add unapproved mounts")]
    ImageVolumes,
    /// The image sets environment keys the sandbox environment leaves unset; names only, never values.
    #[error("{0}")]
    ImageEnvUnset(String),
    /// The image user is not a numeric non-root `uid:gid`.
    #[error("the image user must be a numeric non-root uid:gid")]
    ImageUser,
    /// The entrypoint differs from the base's, so another process could be the one that dials the gateway.
    #[error("the image entrypoint must be its base image's")]
    ImageEntrypoint,
}

impl TemplateRefusal {
    /// The stable reason code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotDerived => "not_derived",
            Self::RuntimeTooOld { .. } => "runtime_too_old",
            Self::RuntimeLabel => "runtime_label",
            Self::ImageVolumes => "image_volumes",
            Self::ImageEnvUnset(_) => "image_env_unset",
            Self::ImageUser => "image_user",
            Self::ImageEntrypoint => "image_entrypoint",
        }
    }
}

/// Checks an inspected image against the bases, in order: derivation, the protocol label, volumes, the environment
/// (`sandbox_env` holds the keys the sandbox environment sets), the user and the entrypoint. The first failure wins.
///
/// # Errors
/// The [`TemplateRefusal`] of the first failed check.
pub fn check_image<'a>(
    image: &ImageInspect,
    bases: &'a Bases,
    sandbox_env: &BTreeSet<String>,
) -> Result<&'a BaseImage, TemplateRefusal> {
    let layers = image
        .root_fs
        .as_ref()
        .and_then(|root| root.layers.as_deref())
        .unwrap_or_default();
    // The most specific base: Workstation derives from Minimal, so an image on Workstation matches both.
    let base = bases
        .0
        .iter()
        .filter(|base| layers.starts_with(&base.layers))
        .max_by_key(|base| base.layers.len())
        .ok_or(TemplateRefusal::NotDerived)?;
    let config = image.config.as_ref();
    let label = config
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get(RUNTIME_REVISION_LABEL));
    let has =
        parse_revision(label.map(String::as_str)).map_err(|_| TemplateRefusal::RuntimeLabel)?;
    check_runtime_revision(has).map_err(|too_old| TemplateRefusal::RuntimeTooOld {
        needs: too_old.needs,
        has: too_old.has,
    })?;
    if config
        .and_then(|config| config.volumes.as_ref())
        .is_some_and(|volumes| !volumes.is_empty())
    {
        return Err(TemplateRefusal::ImageVolumes);
    }
    let unset: Vec<Option<&str>> = config
        .and_then(|config| config.env.as_ref())
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry.split_once('=') {
            Some((key, _)) if sandbox_env.contains(key) => None,
            Some((key, _)) => Some(Some(key)),
            None => Some(None),
        })
        .collect();
    if !unset.is_empty() {
        return Err(TemplateRefusal::ImageEnvUnset(unset_environment(&unset)));
    }
    if !config
        .and_then(|config| config.user.as_deref())
        .is_some_and(numeric_non_root)
    {
        return Err(TemplateRefusal::ImageUser);
    }
    let entrypoint = config
        .and_then(|config| config.entrypoint.as_deref())
        .unwrap_or_default();
    if entrypoint != base.entrypoint.as_slice() {
        return Err(TemplateRefusal::ImageEntrypoint);
    }
    Ok(base)
}

/// `uid:gid`, both decimal without a leading zero, so neither is root (the rootfs builder's rule).
fn numeric_non_root(user: &str) -> bool {
    let id = |part: &str| {
        part.as_bytes()
            .first()
            .is_some_and(|first| (b'1'..=b'9').contains(first))
            && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    user.split_once(':')
        .is_some_and(|(uid, gid)| id(uid) && id(gid))
}
