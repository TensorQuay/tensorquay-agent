use crate::{GatewayConfig, GatewayError};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

#[cfg(all(test, unix))]
#[path = "storage_tests.rs"]
mod tests;

/// The directory of agent teams' handed-back files, by member task.
const HANDED: &str = "handed";
/// The screenshot store's directory under the storage root (ADR-0039 section 9).
const SCREENS: &str = "screens";

#[derive(Clone)]
pub(crate) struct Storage {
    root: Arc<PathBuf>,
    timeout: std::time::Duration,
    cancel: CancellationToken,
    /// Workspaces are `workspaces/<id>.ext4` images (the microVM backend); the gateway never mounts or reads them.
    block_images: bool,
    /// The resolved install tier of read-only skills; `None` when not configured or on the microVM backend.
    skills_install: Option<Arc<PathBuf>>,
}

impl Storage {
    pub async fn prepare(
        config: &GatewayConfig,
        cancel: CancellationToken,
    ) -> Result<Self, GatewayError> {
        let root = crate::io::bounded(config.io_timeout(), &cancel, async {
            create_private(&config.storage_root).await?;
            let root = tokio::fs::canonicalize(&config.storage_root).await?;
            for name in ["blobs", "workspaces", "runtime-state", HANDED] {
                create_private(&root.join(name)).await?;
            }
            Ok(root)
        })
        .await?;
        Ok(Self {
            root: Arc::new(root),
            timeout: config.io_timeout(),
            cancel,
            block_images: config.uses_block_images(),
            skills_install: None,
        })
    }

    /// Resolves `[skills] root` against this storage root and the installation's secret and configuration files. It is
    /// checked on every backend but mounted only by Docker, so the microVM backend keeps no install tier.
    pub async fn with_skills(
        mut self,
        config: &GatewayConfig,
        secret_files: Option<&crate::SecretFiles>,
    ) -> Result<Self, GatewayError> {
        let Some(root) = &config.skills.root else {
            return Ok(self);
        };
        let mut files: Vec<&Path> = secret_files
            .into_iter()
            .flat_map(crate::SecretFiles::paths)
            .collect();
        files.push(&config.models_path);
        let protected = crate::skills::protected(&self.root, &files);
        let resolved = crate::io::bounded(
            self.timeout,
            &self.cancel,
            crate::skills::resolve_install_tier(root, &protected),
        )
        .await?;
        self.skills_install = (!self.block_images).then(|| Arc::new(resolved));
        Ok(self)
    }

    /// The resolved install tier, when one is mounted.
    pub fn skills_install(&self) -> Option<PathBuf> {
        self.skills_install.as_deref().cloned()
    }

    /// A user's own read-only skills tier, `skills/users/<id>`, created empty and private on first use.
    pub async fn user_skills(&self, user: uuid::Uuid) -> Result<PathBuf, GatewayError> {
        let path = self
            .root
            .join("skills")
            .join("users")
            .join(user.to_string());
        crate::io::bounded(self.timeout, &self.cancel, create_private(&path)).await?;
        Ok(path)
    }

    /// Bytes a workspace image occupies on the host disk (images are sparse); `None` before the backend created it.
    pub async fn image_allocation(&self, id: uuid::Uuid) -> Option<u64> {
        use std::os::unix::fs::MetadataExt;
        let image = self.root.join("workspaces").join(format!("{id}.ext4"));
        let metadata = crate::io::bounded(self.timeout, &self.cancel, async {
            Ok(tokio::fs::metadata(&image).await?)
        })
        .await
        .ok()?;
        Some(metadata.blocks().saturating_mul(512))
    }

    /// Whether workspaces are block images rather than directories.
    pub const fn block_images(&self) -> bool {
        self.block_images
    }

    /// A workspace's directory, without creating it.
    pub fn workspace_path(&self, id: uuid::Uuid) -> PathBuf {
        self.root.join("workspaces").join(id.to_string())
    }

    pub async fn workspace(&self, id: uuid::Uuid) -> Result<PathBuf, GatewayError> {
        let path = self.root.join("workspaces").join(id.to_string());
        crate::io::bounded(self.timeout, &self.cancel, async {
            create_private(&path).await
        })
        .await?;
        Ok(path)
    }

    /// Removes a deleted workspace's directory tree. Symbolic links inside it are removed, never followed.
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn remove_workspace_tree(&self, id: uuid::Uuid) -> Result<(), GatewayError> {
        let path = self.root.join("workspaces").join(id.to_string());
        if self.block_images {
            // The image is the workspace; there is no tree, and nothing inside it is read.
            let image = path.with_extension("ext4");
            return crate::io::bounded(self.timeout, &self.cancel, async {
                match tokio::fs::remove_file(&image).await {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(GatewayError::from(error)),
                }
            })
            .await;
        }
        crate::io::bounded(self.timeout, &self.cancel, async {
            match tokio::fs::remove_dir_all(&path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(GatewayError::from(error)),
            }
        })
        .await
    }

    /// Removes a purged task's own files: its large frames (`blobs/<id>/`) and its runtime state, a directory on Docker
    /// or `runtime-state/<id>.ext4` on the microVM backend (both are tried: either may be left from a backend change).
    /// Symbolic links inside are removed, never followed; a path already gone is done.
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn remove_task_files(&self, id: uuid::Uuid) -> Result<(), GatewayError> {
        let name = id.to_string();
        crate::io::bounded(self.timeout, &self.cancel, async {
            remove_tree(&self.root.join("blobs").join(&name)).await?;
            remove_tree(&self.root.join("runtime-state").join(&name)).await?;
            remove_tree(&self.root.join(HANDED).join(&name)).await?;
            remove_tree(&self.root.join(SCREENS).join(&name)).await?;
            remove_file(&self.root.join("runtime-state").join(format!("{name}.ext4"))).await
        })
        .await
    }

    /// Where a conversation's stored screenshot `id` lives, `screens/<conversation>/<id>.png` (ADR-0039 section 9).
    pub fn screenshot(&self, conversation: uuid::Uuid, id: &str) -> PathBuf {
        self.root
            .join(SCREENS)
            .join(conversation.to_string())
            .join(format!("{id}.png"))
    }

    /// Writes a screenshot's PNG whole, through a temporary name, so a reader never sees part of one.
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn keep_screenshot(
        &self,
        conversation: uuid::Uuid,
        id: &str,
        png: &[u8],
    ) -> Result<(), GatewayError> {
        let path = self.screenshot(conversation, id);
        let directory = path.parent().ok_or(GatewayError::Io)?.to_path_buf();
        let staging = directory.join(format!(".{id}.{}", uuid::Uuid::now_v7().simple()));
        let kept = crate::io::bounded(self.timeout, &self.cancel, async {
            create_private(&directory).await?;
            tokio::fs::write(&staging, png)
                .await
                .map_err(|_| GatewayError::Io)?;
            tokio::fs::rename(&staging, &path)
                .await
                .map_err(|_| GatewayError::Io)
        })
        .await;
        // A failed or timed-out write leaves no half file behind (best effort: a missing one is fine).
        if kept.is_err() {
            let _ = tokio::fs::remove_file(&staging).await;
        }
        kept
    }

    /// Removes one stored screenshot's file; a missing one is already gone.
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn remove_screenshot(
        &self,
        conversation: uuid::Uuid,
        id: &str,
    ) -> Result<(), GatewayError> {
        let path = self.screenshot(conversation, id);
        crate::io::bounded(self.timeout, &self.cancel, remove_file(&path)).await
    }

    /// Where a member task's handed-back files are stored, `handed/<member>/` (ADR-0034 point 6): one directory per
    /// report, by its seq. Removed with the task's other files at its purge.
    pub fn handed(&self, member: uuid::Uuid) -> PathBuf {
        self.root.join(HANDED).join(member.to_string())
    }

    /// A new private folder for a report's files while they are read, `handed/<member>/.staging-<id>/`.
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn handed_staging(&self, member: uuid::Uuid) -> Result<PathBuf, GatewayError> {
        let parent = self.handed(member);
        let staging = parent.join(format!(".staging-{}", uuid::Uuid::now_v7().simple()));
        crate::io::bounded(self.timeout, &self.cancel, async {
            create_private(&parent).await?;
            create_private(&staging).await
        })
        .await?;
        Ok(staging)
    }

    /// Moves a report's read files to `handed/<member>/<seq>/`, in the transaction that stores the report. A folder
    /// already there can only be left by a report that never committed (a seq is the team's, under its row lock), so it
    /// is replaced.
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn keep_handed(
        &self,
        staging: &Path,
        member: uuid::Uuid,
        seq: u64,
    ) -> Result<(), GatewayError> {
        let kept = self.handed(member).join(seq.to_string());
        crate::io::bounded(self.timeout, &self.cancel, async {
            remove_tree(&kept).await?;
            tokio::fs::rename(staging, &kept).await?;
            Ok(())
        })
        .await
    }

    /// Removes a staging folder or a kept report's files; one already gone is done.
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn remove_handed(&self, path: &Path) -> Result<(), GatewayError> {
        crate::io::bounded(self.timeout, &self.cancel, remove_tree(path)).await
    }

    /// Removes a purged workspace's recorded changes, `changes/<id>/` (snapshot manifests and file copies).
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn remove_changes(&self, id: uuid::Uuid) -> Result<(), GatewayError> {
        let path = self.root.join("changes").join(id.to_string());
        crate::io::bounded(self.timeout, &self.cancel, remove_tree(&path)).await
    }

    /// Removes one snapshot's manifest, `changes/<workspace>/snapshots/<snapshot>.json`.
    ///
    /// # Errors
    /// A storage failure or the bound elapsing.
    pub async fn remove_snapshot(
        &self,
        workspace: uuid::Uuid,
        snapshot: uuid::Uuid,
    ) -> Result<(), GatewayError> {
        let path = self
            .root
            .join("changes")
            .join(workspace.to_string())
            .join("snapshots")
            .join(format!("{snapshot}.json"));
        crate::io::bounded(self.timeout, &self.cancel, remove_file(&path)).await
    }

    /// A workspace's ext4 image for the microVM backend. Its private parent exists; the backend creates the image.
    pub async fn workspace_image(&self, id: uuid::Uuid) -> Result<PathBuf, GatewayError> {
        self.image_in("workspaces", id).await
    }

    /// A conversation's runtime-state image for the microVM backend, beside the Docker state directories.
    pub async fn state_image(&self, id: uuid::Uuid) -> Result<PathBuf, GatewayError> {
        self.image_in("runtime-state", id).await
    }

    async fn image_in(&self, parent: &str, id: uuid::Uuid) -> Result<PathBuf, GatewayError> {
        let directory = self.root.join(parent);
        crate::io::bounded(self.timeout, &self.cancel, create_private(&directory)).await?;
        Ok(directory.join(format!("{id}.ext4")))
    }

    /// A conversation's runtime state directory, without creating it (the live quota measures it, N4).
    pub fn runtime_state_path(&self, id: uuid::Uuid) -> PathBuf {
        self.root.join("runtime-state").join(id.to_string())
    }

    pub async fn runtime_state(&self, id: uuid::Uuid) -> Result<PathBuf, GatewayError> {
        let path = self.root.join("runtime-state").join(id.to_string());
        crate::io::bounded(self.timeout, &self.cancel, create_private(&path)).await?;
        Ok(path)
    }

    pub async fn remove_workspace(&self, path: &Path) -> Result<(), GatewayError> {
        crate::io::bounded(self.timeout, &self.cancel, async {
            tokio::fs::remove_dir(path)
                .await
                .map_err(GatewayError::from)
        })
        .await
    }

    pub async fn write_blob(
        &self,
        id: uuid::Uuid,
        sequence: i64,
        bytes: &[u8],
    ) -> Result<String, GatewayError> {
        let key = format!("{id}/{sequence}");
        let directory = self.root.join("blobs").join(id.to_string());
        let target = self.root.join("blobs").join(&key);
        let temporary = directory.join(format!("{}.pending", uuid::Uuid::now_v7()));
        let result = crate::io::bounded(self.timeout, &self.cancel, async {
            create_private(&directory).await?;
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temporary).await?;
            file.write_all(bytes).await?;
            file.flush().await?;
            file.sync_all().await?;
            tokio::fs::rename(&temporary, target).await?;
            tokio::fs::File::open(&directory).await?.sync_all().await?;
            Ok(key)
        })
        .await;
        if result.is_err() {
            self.remove_path(&temporary).await;
        }
        result
    }

    pub async fn read_blob(
        &self,
        id: uuid::Uuid,
        sequence: i64,
        key: &str,
    ) -> Result<Vec<u8>, GatewayError> {
        if key != format!("{id}/{sequence}") {
            return Err(GatewayError::Io);
        }
        crate::io::bounded(
            self.timeout,
            &self.cancel,
            crate::io::read_bounded(
                &self.root.join("blobs").join(key),
                tqa_types::MAX_FRAME_BYTES,
            ),
        )
        .await
    }

    async fn remove_path(&self, path: &Path) {
        let result = tokio::time::timeout(self.timeout, tokio::fs::remove_file(path)).await;
        match result {
            Ok(Ok(())) => (),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => tracing::warn!("temporary storage cleanup failed"),
        }
    }
}

async fn create_private(path: &Path) -> Result<(), GatewayError> {
    let mut builder = tokio::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path).await?;
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.is_dir() {
        return Err(GatewayError::Io);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(GatewayError::Configuration(
                "storage directories must be private",
            ));
        }
    }
    Ok(())
}

/// Removes a directory tree; one already gone is done. `remove_dir_all` removes symbolic links, never follows them.
async fn remove_tree(path: &Path) -> Result<(), GatewayError> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(GatewayError::from(error)),
    }
}

/// Removes a file; one already gone is done.
async fn remove_file(path: &Path) -> Result<(), GatewayError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(GatewayError::from(error)),
    }
}
