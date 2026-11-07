//! The runtime-image preflight: the image must speak at least the gateway's protocol revision.
use super::{DockerBackend, io};
use crate::{ImageRef, SandboxError};
use bollard::models::ImageInspect;
use tqa_types::{RUNTIME_REVISION_LABEL, check_runtime_revision, parse_revision};

#[cfg(test)]
mod tests;

impl DockerBackend {
    /// Checks the configured runtime image without pulling it: present, and labelled with a
    /// protocol revision at or above the floor. An unlabelled image counts as revision 1.
    ///
    /// # Errors
    /// Returns `ImageNotFound`, `RuntimeTooOld` with both revisions, `InvalidSpec` for a
    /// malformed label, or bounded transport failures. No error names the image.
    pub async fn check_runtime_image(&self, image: &ImageRef) -> Result<u16, SandboxError> {
        let inspected = io::request(
            self.config.operation_timeout,
            self.docker.inspect_image(&image.to_string()),
        )
        .await
        .map_err(|error| match error {
            SandboxError::NotFound => SandboxError::ImageNotFound,
            other => other,
        })?;
        let has = label_revision(&inspected)?;
        check_runtime_revision(has)
            .map_err(|too_old| SandboxError::RuntimeTooOld {
                needs: too_old.needs,
                has: too_old.has,
            })
            .map(|()| has)
    }
}

fn label_revision(image: &ImageInspect) -> Result<u16, SandboxError> {
    let label = image
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get(RUNTIME_REVISION_LABEL));
    parse_revision(label.map(String::as_str)).map_err(|_| {
        SandboxError::invalid("image", "the runtime protocol revision label is malformed")
    })
}
