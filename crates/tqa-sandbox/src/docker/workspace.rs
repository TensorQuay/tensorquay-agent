use super::{DockerBackend, io};
use crate::{BackendKind, SandboxError, SandboxSpec, StateSource, WorkspaceSource};
use tokio_util::sync::CancellationToken;

impl DockerBackend {
    pub(super) async fn prepare_workspace(
        &self,
        spec: &mut SandboxSpec,
        cancel: &CancellationToken,
    ) -> Result<(), SandboxError> {
        match &mut spec.workspace.source {
            WorkspaceSource::HostDirectory(path) => {
                *path = io::cancellable(cancel, async {
                    tokio::time::timeout(self.config.operation_timeout, resolve(path, "workspace"))
                        .await
                        .map_err(|_| SandboxError::Timeout)?
                })
                .await?;
            }
            WorkspaceSource::NamedVolume(name) => {
                io::cancellable(
                    cancel,
                    io::request(
                        self.config.operation_timeout,
                        self.docker.inspect_volume(name),
                    ),
                )
                .await
                .map_err(missing_volume)?;
            }
            WorkspaceSource::BlockImage(_) => {
                return Err(SandboxError::invalid(
                    "workspace",
                    "block images require the microVM backend",
                ));
            }
        }
        let StateSource::HostDirectory(state) = &mut spec.state.source else {
            return Err(SandboxError::invalid(
                "state",
                "block images require the microVM backend",
            ));
        };
        *state = io::cancellable(cancel, async {
            tokio::time::timeout(self.config.operation_timeout, resolve(state, "state"))
                .await
                .map_err(|_| SandboxError::Timeout)?
        })
        .await?;
        let credentials = self.config.storage_root.join("sandboxes");
        reject_overlap(state, &credentials, "state")?;
        if let WorkspaceSource::HostDirectory(path) = &spec.workspace.source {
            reject_overlap(path, &credentials, "workspace")?;
        }
        self.prepare_skills(spec, &credentials, cancel).await?;
        spec.validate(BackendKind::Docker)
    }

    /// Resolves each skills tier to a canonical directory, so a symlink cannot hide an overlap with private storage.
    async fn prepare_skills(
        &self,
        spec: &mut SandboxSpec,
        credentials: &std::path::Path,
        cancel: &CancellationToken,
    ) -> Result<(), SandboxError> {
        for tier in [&mut spec.skills.install, &mut spec.skills.user] {
            let Some(path) = tier else { continue };
            *path = io::cancellable(cancel, async {
                tokio::time::timeout(self.config.operation_timeout, resolve(path, "skills"))
                    .await
                    .map_err(|_| SandboxError::Timeout)?
            })
            .await?;
            reject_overlap(path, credentials, "skills")?;
        }
        Ok(())
    }
}

fn reject_overlap(
    path: &std::path::Path,
    credentials: &std::path::Path,
    field: &'static str,
) -> Result<(), SandboxError> {
    if credentials.starts_with(path) || path.starts_with(credentials) {
        return Err(SandboxError::invalid(
            field,
            "mount must not overlap credential storage",
        ));
    }
    Ok(())
}

async fn resolve(
    path: &std::path::Path,
    field: &'static str,
) -> Result<std::path::PathBuf, SandboxError> {
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| SandboxError::invalid(field, "mount directory cannot be resolved"))?;
    let metadata = tokio::fs::metadata(&canonical)
        .await
        .map_err(|_| SandboxError::invalid(field, "mount directory cannot be inspected"))?;
    if !metadata.is_dir() {
        return Err(SandboxError::invalid(
            field,
            "expected an existing mount directory",
        ));
    }
    Ok(canonical)
}
fn missing_volume(error: SandboxError) -> SandboxError {
    if error == SandboxError::NotFound {
        SandboxError::invalid(
            "workspace",
            "named volume must be provisioned before creation",
        )
    } else {
        error
    }
}
