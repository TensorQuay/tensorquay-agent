//! The Docker engine behind a template's runtime-binary check (ADR-0032, SBX-U-008): a container created from the image
//! with no network and never started, the engine's tar archive of one path, and a forced removal.
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use bollard::{
    Docker,
    models::{ContainerCreateBody, ContainerSummary, HostConfig, ImageInspect},
    query_parameters::{
        CreateContainerOptions, DownloadFromContainerOptionsBuilder, ListContainersOptions,
        RemoveContainerOptions,
    },
};
use futures::{StreamExt, stream::BoxStream};

use super::io;
use crate::{
    SandboxError,
    template::{ImageFiles, ImageSource},
};

#[cfg(test)]
mod tests;

/// Every check container's name starts with this, and carries [`CHECK_LABEL`], so a leftover is recognisable.
pub const CHECK_PREFIX: &str = "tqa-template-check-";
/// The label marking a template check container.
pub const CHECK_LABEL: &str = "tqa.template-check";
/// The most leftovers one sweep removes; the next start-up takes the rest.
const MAX_SWEPT: usize = 256;

/// Reads files out of images through a Docker engine; each call is bounded by `operation_timeout`.
#[derive(Clone, Debug)]
pub struct DockerImageFiles {
    docker: Docker,
    operation_timeout: Duration,
    /// The gateway's instance (its sandbox network prefix): every check container carries it as `tqa.instance`, so a
    /// sweep never touches another gateway's containers on a shared engine.
    instance: String,
}

impl DockerImageFiles {
    /// Uses an existing engine connection for the gateway `instance`.
    #[must_use]
    pub fn new(docker: Docker, operation_timeout: Duration, instance: impl Into<String>) -> Self {
        Self {
            docker,
            operation_timeout,
            instance: instance.into(),
        }
    }

    /// Uses the engine the environment names (`DOCKER_HOST`, else the default socket), as the backends do. Nothing is
    /// contacted here: an engine that is down makes each call fail, which the checks treat as unavailable.
    ///
    /// # Errors
    /// `Backend` when the engine address cannot be used at all.
    pub fn from_defaults(
        operation_timeout: Duration,
        instance: impl Into<String>,
    ) -> Result<Self, SandboxError> {
        let docker = Docker::connect_with_defaults()
            .map_err(|_| SandboxError::Backend("the Docker Engine address is unusable".into()))?;
        Ok(Self::new(docker, operation_timeout, instance))
    }

    /// Removes this instance's check containers created at least `min_age` ago (SBX-D-009): leftovers of a gateway
    /// that stopped mid-check. A younger one may belong to a check still running; other instances' are never touched.
    /// Returns how many it removed.
    ///
    /// # Errors
    /// The engine failed or timed out listing or removing them.
    pub async fn sweep(&self, min_age: Duration) -> Result<usize, SandboxError> {
        let filters = HashMap::from([(
            "label".to_owned(),
            vec![
                CHECK_LABEL.to_owned(),
                format!("tqa.instance={}", self.instance),
            ],
        )]);
        let containers = io::request(
            self.operation_timeout,
            self.docker.list_containers(Some(ListContainersOptions {
                all: true,
                filters: Some(filters),
                ..Default::default()
            })),
        )
        .await?;
        let due = due(&containers, SystemTime::now(), min_age);
        for id in &due {
            self.remove_named(id).await?;
        }
        Ok(due.len())
    }

    async fn remove_named(&self, name: &str) -> Result<(), SandboxError> {
        match io::request(
            self.operation_timeout,
            self.docker.remove_container(
                name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            ),
        )
        .await
        {
            Ok(()) | Err(SandboxError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// A name no other check container on this engine has: this process, a counter and the time.
fn check_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!(
        "{CHECK_PREFIX}{}-{}-{nanos}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// The listed containers created at least `min_age` before `now`, at most [`MAX_SWEPT`]. The engine gives creation
/// times in whole seconds; one without an ID or a time is left alone.
fn due(containers: &[ContainerSummary], now: SystemTime, min_age: Duration) -> Vec<String> {
    let Some(cutoff) = now.checked_sub(min_age) else {
        return Vec::new();
    };
    containers
        .iter()
        .filter_map(|container| {
            let created = u64::try_from(container.created?).ok()?;
            let created = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(created))?;
            (created <= cutoff).then(|| container.id.clone()).flatten()
        })
        .take(MAX_SWEPT)
        .collect()
}

/// The container: the image, no network at all, the check label and the instance; it is never started.
fn check_body(image: &str, instance: &str) -> ContainerCreateBody {
    ContainerCreateBody {
        image: Some(image.to_owned()),
        network_disabled: Some(true),
        labels: Some(
            [
                (CHECK_LABEL.to_owned(), "1".to_owned()),
                ("tqa.instance".to_owned(), instance.to_owned()),
            ]
            .into(),
        ),
        host_config: Some(HostConfig {
            network_mode: Some("none".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[async_trait]
impl ImageFiles for DockerImageFiles {
    async fn create_stopped(&self, image: &str) -> Result<String, SandboxError> {
        let name = check_name();
        let created = io::request(
            self.operation_timeout,
            self.docker.create_container(
                Some(CreateContainerOptions {
                    name: Some(name.clone()),
                    ..Default::default()
                }),
                check_body(image, &self.instance),
            ),
        )
        .await;
        match created {
            Ok(created) => Ok(created.id),
            Err(SandboxError::NotFound) => Err(SandboxError::ImageNotFound),
            // The engine may still create it after the timeout: remove it by its name, which only this call knows.
            Err(SandboxError::Timeout) => {
                self.remove_named(&name).await?;
                Err(SandboxError::Timeout)
            }
            Err(error) => Err(error),
        }
    }

    fn archive<'a>(
        &'a self,
        container: &str,
        path: &str,
    ) -> BoxStream<'a, Result<Vec<u8>, SandboxError>> {
        let options = DownloadFromContainerOptionsBuilder::new()
            .path(path)
            .build();
        self.docker
            .download_from_container(container, Some(options))
            .map(|chunk| {
                chunk
                    .map(|bytes| bytes.to_vec())
                    .map_err(|error| io::map_error(&error))
            })
            .boxed()
    }

    async fn remove(&self, container: &str) -> Result<(), SandboxError> {
        self.remove_named(container).await
    }
}

#[async_trait]
impl ImageSource for DockerImageFiles {
    async fn inspect(&self, image: &str) -> Result<ImageInspect, SandboxError> {
        io::request(self.operation_timeout, self.docker.inspect_image(image))
            .await
            .map_err(|error| match error {
                SandboxError::NotFound => SandboxError::ImageNotFound,
                other => other,
            })
    }

    fn export<'a>(&'a self, image: &str) -> BoxStream<'a, Result<Vec<u8>, SandboxError>> {
        self.docker
            .export_image(image)
            .map(|chunk| {
                chunk
                    .map(|bytes| bytes.to_vec())
                    .map_err(|error| io::map_error(&error))
            })
            .boxed()
    }
}
