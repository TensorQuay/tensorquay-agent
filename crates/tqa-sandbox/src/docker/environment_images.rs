//! The images an administrator can make an environment from (the built-in environments design): those on the engine
//! host that carry the runtime's protocol-revision label, so every build of our runtime and anything built FROM it.
//! Read-only: nothing is pulled.
use std::{collections::BTreeMap, time::Duration};

use bollard::{Docker, models::ImageSummary, query_parameters::ListImagesOptions};

use super::io;
use crate::SandboxError;

/// The most images one listing returns (newest first): enough for any real host, bounded for the API.
pub const MAX_LISTED: usize = 200;

/// One image as the engine reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRecord {
    /// The image ID (`sha256:…`), the pin an environment uses.
    pub id: String,
    /// Its local tags, without Docker's `<none>:<none>` placeholder.
    pub tags: Vec<String>,
    /// Seconds since the Unix epoch.
    pub created: i64,
    /// Its size on the engine host, in bytes.
    pub size_bytes: u64,
    /// Its labels: ours say which environment it is and what it can do.
    pub labels: BTreeMap<String, String>,
}

/// Lists the environment images on the engine host.
pub struct EnvironmentImages {
    docker: Docker,
    operation_timeout: Duration,
}

impl EnvironmentImages {
    /// Uses an existing engine connection; each listing is bounded by `operation_timeout`.
    #[must_use]
    pub const fn new(docker: Docker, operation_timeout: Duration) -> Self {
        Self {
            docker,
            operation_timeout,
        }
    }

    /// Uses the engine the environment names (`DOCKER_HOST`, else the default socket), as the backends do.
    ///
    /// # Errors
    /// `Backend` when the engine address cannot be used at all.
    pub fn from_defaults(operation_timeout: Duration) -> Result<Self, SandboxError> {
        let docker = Docker::connect_with_defaults()
            .map_err(|_| SandboxError::Backend("the Docker Engine address is unusable".into()))?;
        Ok(Self::new(docker, operation_timeout))
    }

    /// The images carrying the runtime's revision label, newest first, at most [`MAX_LISTED`].
    ///
    /// # Errors
    /// The engine failed or didn't answer within the bound.
    pub async fn list(&self) -> Result<Vec<ImageRecord>, SandboxError> {
        let filters = std::collections::HashMap::from([(
            "label".to_owned(),
            vec![tqa_types::RUNTIME_REVISION_LABEL.to_owned()],
        )]);
        let summaries = io::request(
            self.operation_timeout,
            self.docker.list_images(Some(ListImagesOptions {
                filters: Some(filters),
                ..Default::default()
            })),
        )
        .await?;
        Ok(records(summaries))
    }
}

/// The engine's summaries as records: newest first (ties by ID), at most [`MAX_LISTED`], placeholder tags dropped.
pub(crate) fn records(summaries: Vec<ImageSummary>) -> Vec<ImageRecord> {
    let mut records: Vec<ImageRecord> = summaries
        .into_iter()
        .map(|summary| ImageRecord {
            id: summary.id,
            tags: summary
                .repo_tags
                .into_iter()
                .filter(|tag| tag != "<none>:<none>")
                .collect(),
            created: summary.created,
            size_bytes: u64::try_from(summary.size).unwrap_or(0),
            labels: summary.labels.into_iter().collect(),
        })
        .collect();
    records.sort_by(|left, right| {
        right
            .created
            .cmp(&left.created)
            .then_with(|| left.id.cmp(&right.id))
    });
    records.truncate(MAX_LISTED);
    records
}

#[cfg(test)]
#[path = "environment_images_tests.rs"]
mod tests;
