use super::{DockerBackend, image_policy::validate_image, io};
use crate::{SandboxError, SandboxSpec};
use bollard::query_parameters::CreateImageOptions;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
mod tests;

impl DockerBackend {
    pub(super) async fn ensure_image(
        &self,
        spec: &SandboxSpec,
        cancel: &CancellationToken,
    ) -> Result<(), SandboxError> {
        let reference = spec.image.to_string();
        let inspect = io::cancellable(
            cancel,
            io::request(
                self.config.operation_timeout,
                self.docker.inspect_image(&reference),
            ),
        )
        .await;
        let image = match inspect {
            Ok(image) => image,
            Err(SandboxError::NotFound) if spec.image.is_local() => {
                return Err(SandboxError::ImageNotFound);
            }
            Err(SandboxError::NotFound) => {
                tracing::info!(image_digest = spec.image.digest(), "pulling sandbox image");
                io::cancellable(cancel, async {
                    tokio::time::timeout(self.config.pull_timeout, self.pull_image(&reference))
                        .await
                        .map_err(|_| SandboxError::Timeout)?
                })
                .await?;
                io::cancellable(
                    cancel,
                    io::request(
                        self.config.operation_timeout,
                        self.docker.inspect_image(&reference),
                    ),
                )
                .await
                .map_err(image_not_found)?
            }
            Err(error) => return Err(error),
        };
        validate_image(&image, spec)?;
        self.warn_if_tag_moved(spec, image.id.as_deref(), cancel)
            .await;
        Ok(())
    }

    /// The recorded digest is always what runs; when the name's tag now points at another local image, an operator
    /// should know the pin and the tag disagree (SBX-D-004). Advisory only: a failed check never fails the create.
    async fn warn_if_tag_moved(
        &self,
        spec: &SandboxSpec,
        pinned: Option<&str>,
        cancel: &CancellationToken,
    ) {
        let Some(name) = spec.image.name() else {
            return;
        };
        let tagged = io::cancellable(
            cancel,
            io::request(
                self.config.operation_timeout,
                self.docker.inspect_image(name),
            ),
        )
        .await;
        match tagged {
            Ok(tagged) if tagged.id.as_deref() != pinned => tracing::warn!(
                image = name,
                image_digest = spec.image.digest(),
                "image tag has moved; the recorded digest is used"
            ),
            Ok(_) | Err(SandboxError::NotFound) => {}
            Err(error) => tracing::debug!(code = error.code(), "image tag check failed"),
        }
    }

    async fn pull_image(&self, reference: &str) -> Result<(), SandboxError> {
        let mut stream = self.docker.create_image(
            Some(CreateImageOptions {
                from_image: Some(reference.into()),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(item) = stream.next().await {
            // Registry text is untrusted and may echo credentials; never retain or log it.
            item.map_err(|_| SandboxError::ImageNotFound)?;
        }
        Ok(())
    }
}

fn image_not_found(error: SandboxError) -> SandboxError {
    if error == SandboxError::NotFound {
        SandboxError::ImageNotFound
    } else {
        error
    }
}
