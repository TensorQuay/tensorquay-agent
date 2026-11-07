use super::DockerBackend;
use crate::SandboxError;
use secrecy::{ExposeSecret, SecretString};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::io::AsyncWriteExt;

pub(super) async fn prepare(root: &Path) -> Result<PathBuf, SandboxError> {
    let root = tokio::fs::canonicalize(root).await.map_err(storage_error)?;
    if !tokio::fs::metadata(&root)
        .await
        .map_err(storage_error)?
        .is_dir()
        || root.parent().is_none()
    {
        return Err(SandboxError::invalid(
            "storage_root",
            "expected an existing non-root directory",
        ));
    }
    let directory = root.join("sandboxes");
    match private_directory(&directory).await {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            verify_directory(&directory).await?;
        }
        Err(error) => return Err(storage_error(error)),
    }
    Ok(root)
}

impl DockerBackend {
    pub(super) fn token_path(&self, sandbox_id: &str) -> Result<PathBuf, SandboxError> {
        validate_id(sandbox_id)?;
        Ok(self
            .config
            .storage_root
            .join("sandboxes")
            .join(sandbox_id)
            .join("token"))
    }

    pub(super) async fn write_token(
        &self,
        sandbox_id: &str,
        token: &SecretString,
    ) -> Result<(), SandboxError> {
        let path = self.token_path(sandbox_id)?;
        let directory = path
            .parent()
            .ok_or_else(|| SandboxError::invalid("token_file", "missing credential directory"))?;
        tokio::time::timeout(self.config.operation_timeout, private_directory(directory))
            .await
            .map_err(|_| SandboxError::Timeout)?
            .map_err(storage_error)?;
        let result = tokio::time::timeout(self.config.operation_timeout, write_file(&path, token))
            .await
            .map_err(|_| SandboxError::Timeout)
            .and_then(std::convert::identity);
        if result.is_err() {
            self.remove_token(sandbox_id).await;
        }
        result
    }

    pub(super) async fn remove_token(&self, sandbox_id: &str) {
        let result = async {
            let path = self.token_path(sandbox_id)?;
            remove_missing_ok(tokio::fs::remove_file(&path).await)?;
            let directory = path.parent().ok_or_else(|| {
                SandboxError::invalid("token_file", "missing credential directory")
            })?;
            remove_missing_ok(tokio::fs::remove_dir(directory).await)
        };
        let cleanup = tokio::time::timeout(self.config.operation_timeout, result)
            .await
            .map_err(|_| SandboxError::Timeout)
            .and_then(std::convert::identity);
        if let Err(error) = cleanup {
            tracing::warn!(error_code = error.code(), "credential file cleanup failed");
        }
    }
}

fn validate_id(id: &str) -> Result<(), SandboxError> {
    if id.is_empty()
        || id.len() > 200
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(SandboxError::invalid(
            "token_file",
            "invalid credential directory identifier",
        ));
    }
    Ok(())
}

async fn private_directory(path: &Path) -> io::Result<()> {
    let mut builder = tokio::fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path).await
}

async fn verify_directory(path: &Path) -> Result<(), SandboxError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(storage_error)?;
    if !metadata.is_dir() {
        return Err(SandboxError::Preflight(
            "credential storage must be a real private directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o700 {
            return Err(SandboxError::Preflight(
                "credential storage directory must have mode 0700",
            ));
        }
    }
    Ok(())
}

async fn write_file(path: &Path, token: &SecretString) -> Result<(), SandboxError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).await.map_err(storage_error)?;
    file.write_all(token.expose_secret().as_bytes())
        .await
        .map_err(storage_error)?;
    file.flush().await.map_err(storage_error)?;
    #[cfg(target_os = "linux")]
    {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || std::os::unix::fs::chown(path, Some(1000), None)).await.map_err(|_| SandboxError::Backend("credential ownership task failed".into()))?.map_err(|_| SandboxError::Preflight("credential files must be owned by uid 1000; run as that user or with ownership-changing permission"))?;
    }
    Ok(())
}

fn storage_error(_: io::Error) -> SandboxError {
    SandboxError::Backend("credential storage operation failed".into())
}

fn remove_missing_ok(result: io::Result<()>) -> Result<(), SandboxError> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

#[cfg(test)]
mod tests;
