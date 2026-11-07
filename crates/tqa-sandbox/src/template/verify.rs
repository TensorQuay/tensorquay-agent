//! A template's checks on the engine (ADR-0032): the image and its bases inspected, the pure checks
//! ([`super::check_image`]), the runtime binary compared with the base's (read without running either image), and on
//! microVM installs the rootfs status. Engine trouble is `Unavailable`, never a verdict.
use std::{collections::BTreeSet, time::Duration};

use async_trait::async_trait;
use bollard::models::ImageInspect;
use futures::stream::BoxStream;

use super::{
    BaseImage, Bases, ImageFiles, RuntimeBinaryError, TemplateRefusal, check_image, runtime_sha256,
    scan_image::{SCAN_TIMEOUT, ScanFailure, scan_image},
    secret_files::{Bound, Bounds, SecretFiles},
};
use crate::{ImageRef, SandboxError, firecracker::RootfsStatus};

#[cfg(test)]
mod tests;

/// The engine's view of images: their inspection, and their files read without running them.
#[async_trait]
pub trait ImageSource: ImageFiles {
    /// Inspects an image by reference; `ImageNotFound` or `NotFound` when the engine does not have it.
    async fn inspect(&self, image: &str) -> Result<ImageInspect, SandboxError>;
    /// The image as the engine exports it (`docker save`: a tar of its layers and metadata), in chunks.
    fn export<'a>(&'a self, image: &str) -> BoxStream<'a, Result<Vec<u8>, SandboxError>>;
}

/// The microVM rootfs status of an image (ADR-0032).
#[async_trait]
pub trait RootfsSource: Send + Sync {
    /// The status; an error when the storage could not be read.
    async fn status(&self, image: &ImageRef) -> Result<RootfsStatus, SandboxError>;
}

#[async_trait]
impl RootfsSource for crate::firecracker::FirecrackerBackend {
    async fn status(&self, image: &ImageRef) -> Result<RootfsStatus, SandboxError> {
        self.template_status(image).await
    }
}

/// One template image to check.
#[derive(Debug)]
pub struct VerifyRequest<'a> {
    /// Its pinned image.
    pub image: &'a str,
    /// The bases it may derive from: each base template's name and pinned image.
    pub bases: &'a [(String, String)],
    /// The environment keys the sandbox sets.
    pub sandbox_env: &'a BTreeSet<String>,
    /// The bound for each engine read.
    pub bound: Duration,
    /// The credential-file paths the install file acknowledges for this template: found, they are warnings.
    pub acknowledged_secret_files: &'a BTreeSet<String>,
    /// The credential-file scan's bounds (install file settings).
    pub scan_bounds: Bounds,
}

/// Why the engine's checks refused a template. Exhaustive on purpose: a new failure must be mapped by every caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Failure {
    /// The engine does not have the image (or its pin does not match).
    ImageNotFound,
    /// One of the checks that need no engine.
    Refusal(TemplateRefusal),
    /// Its runtime binary differs from its base's.
    RuntimeModified,
    /// It has no runtime binary.
    RuntimeMissing,
    /// Its runtime binary is not one bounded regular file.
    RuntimeUnreadable,
    /// Its microVM rootfs is not ready.
    Rootfs(RootfsStatus),
    /// Its own layers hold credential files the install file does not acknowledge: every hit is listed, the
    /// acknowledged ones marked (paths and rules only).
    SecretFiles(SecretFiles),
    /// Its own layers are past one of the credential-file scan's bounds, so they were not scanned whole.
    ScanLimit {
        /// The bound it exceeds.
        bound: Bound,
        /// That bound's configured value.
        limit: u64,
    },
}

/// The engine checks' verdict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// Every check passed: the base template's name and the image's local ID.
    Verified {
        /// The base template's name.
        base: String,
        /// The local image ID.
        image_id: String,
        /// Credential files found that the install file acknowledges: warnings, paths only.
        acknowledged: SecretFiles,
    },
    /// A check failed.
    Refused(Failure),
    /// The engine could not answer; nothing is known.
    Unavailable,
}

/// Runs the engine checks for one template image.
pub async fn verify(
    source: &dyn ImageSource,
    rootfs: Option<&dyn RootfsSource>,
    request: &VerifyRequest<'_>,
) -> Verdict {
    match checked(source, rootfs, request).await {
        Ok(verdict) | Err(verdict) => verdict,
    }
}

/// The checks in order; `Err` carries the verdict that ends them early.
async fn checked(
    source: &dyn ImageSource,
    rootfs: Option<&dyn RootfsSource>,
    request: &VerifyRequest<'_>,
) -> Result<Verdict, Verdict> {
    let inspected = match source.inspect(request.image).await {
        Ok(inspected) => inspected,
        Err(SandboxError::ImageNotFound | SandboxError::NotFound) => {
            return Err(Verdict::Refused(Failure::ImageNotFound));
        }
        Err(_) => return Err(Verdict::Unavailable),
    };
    let image_id = inspected.id.clone().ok_or(Verdict::Unavailable)?;
    let mut bases = Vec::new();
    let mut base_ids = Vec::new();
    for (name, image) in request.bases {
        match source.inspect(image).await {
            Ok(base) => {
                let layers = base
                    .root_fs
                    .and_then(|root| root.layers)
                    .unwrap_or_default();
                let entrypoint = base
                    .config
                    .and_then(|config| config.entrypoint)
                    .unwrap_or_default();
                if !layers.is_empty() {
                    bases.push(BaseImage::new(name, layers, entrypoint));
                    base_ids.push((name.clone(), base.id.unwrap_or_default()));
                }
            }
            // A base the engine does not have cannot be derived from.
            Err(SandboxError::ImageNotFound | SandboxError::NotFound) => {}
            Err(_) => return Err(Verdict::Unavailable),
        }
    }
    let Ok(bases) = Bases::new(bases) else {
        return Err(Verdict::Refused(Failure::Refusal(
            TemplateRefusal::NotDerived,
        )));
    };
    let base = check_image(&inspected, &bases, request.sandbox_env)
        .map_err(|refusal| Verdict::Refused(Failure::Refusal(refusal)))?;
    let base_id = base_ids
        .iter()
        .find(|(name, _)| name == base.name())
        .map(|(_, id)| id.clone())
        .unwrap_or_default();
    // A template that is its own base carries its own runtime and has no layers of its own.
    let acknowledged = if base_id == image_id {
        SecretFiles::default()
    } else {
        let own_layers = inspected
            .root_fs
            .as_ref()
            .and_then(|root| root.layers.clone())
            .unwrap_or_default()
            .split_off(base.layer_count());
        derived(source, request, &image_id, base_id, own_layers).await?
    };
    if let Some(rootfs) = rootfs {
        let pin: ImageRef = request.image.parse().map_err(|_| Verdict::Unavailable)?;
        match rootfs.status(&pin).await {
            Ok(RootfsStatus::Ready) => {}
            Ok(status) => return Err(Verdict::Refused(Failure::Rootfs(status))),
            Err(_) => return Err(Verdict::Unavailable),
        }
    }
    Ok(Verdict::Verified {
        base: base.name().to_owned(),
        image_id,
        acknowledged,
    })
}

/// A derived image's own checks: its runtime binary is its base's, and its own layers hold no credential file the
/// install file does not acknowledge. Returns the acknowledged ones (warnings).
async fn derived(
    source: &dyn ImageSource,
    request: &VerifyRequest<'_>,
    image_id: &str,
    base_id: String,
    own_layers: Vec<String>,
) -> Result<SecretFiles, Verdict> {
    let image = ImageRef::local_id(image_id.to_owned()).map_err(|_| Verdict::Unavailable)?;
    let runtime = runtime_sha256(source, &image, request.bound)
        .await
        .map_err(|error| match error {
            RuntimeBinaryError::Missing => Verdict::Refused(Failure::RuntimeMissing),
            RuntimeBinaryError::NotAFile
            | RuntimeBinaryError::TooLarge
            | RuntimeBinaryError::Malformed => Verdict::Refused(Failure::RuntimeUnreadable),
            RuntimeBinaryError::Timeout | RuntimeBinaryError::Engine(_) => Verdict::Unavailable,
        })?;
    // The base's binary is the reference: failing to read it says nothing about the template.
    let base_image = ImageRef::local_id(base_id).map_err(|_| Verdict::Unavailable)?;
    let expected = runtime_sha256(source, &base_image, request.bound)
        .await
        .map_err(|_| Verdict::Unavailable)?;
    if runtime != expected {
        return Err(Verdict::Refused(Failure::RuntimeModified));
    }
    let hits = scan_image(
        source,
        image_id,
        own_layers,
        request.scan_bounds,
        SCAN_TIMEOUT,
    )
    .await
    .map_err(|failure| match failure {
        ScanFailure::Limit(bound) => Verdict::Refused(Failure::ScanLimit {
            bound,
            limit: bound.limit(request.scan_bounds),
        }),
        ScanFailure::Unavailable => Verdict::Unavailable,
    })?;
    // Every hit is listed, the acknowledged ones marked; any unacknowledged one refuses the template.
    let found = SecretFiles::listing(&hits, request.acknowledged_secret_files);
    if found.unacknowledged > 0 {
        return Err(Verdict::Refused(Failure::SecretFiles(found)));
    }
    Ok(found)
}
