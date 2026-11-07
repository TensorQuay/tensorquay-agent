use crate::GatewayError;
use rand::TryRngCore;
use secrecy::{ExposeSecret, SecretSlice};
use std::{io::ErrorKind, path::Path};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) async fn load_key(path: &Path) -> Result<SecretSlice<u8>, GatewayError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(_) => (),
        Err(error) if error.kind() == ErrorKind::NotFound => create_key(path).await?,
        Err(_) => return Err(GatewayError::SecretUnavailable),
    }
    let key = read_private(path, 32).await?;
    if key.expose_secret().len() != 32 {
        return Err(GatewayError::Configuration(
            "install key must contain exactly 32 bytes",
        ));
    }
    Ok(key)
}

async fn create_key(path: &Path) -> Result<(), GatewayError> {
    let mut bytes = vec![0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let key: SecretSlice<u8> = bytes.into();
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = match options.open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(()),
        Err(_) => return Err(GatewayError::SecretUnavailable),
    };
    file.write_all(key.expose_secret())
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    file.sync_all()
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let parent = tokio::fs::File::open(path.parent().ok_or(GatewayError::SecretUnavailable)?)
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    parent
        .sync_all()
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    Ok(())
}

pub(crate) async fn read_private(
    path: &Path,
    maximum: usize,
) -> Result<SecretSlice<u8>, GatewayError> {
    let entry = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    if !entry.is_file() {
        return Err(GatewayError::Configuration(
            "secret file must be a regular mode 0600 file",
        ));
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let metadata = file
        .metadata()
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o7777 != 0o600
            || metadata.ino() != entry.ino()
            || metadata.dev() != entry.dev()
        {
            return Err(GatewayError::Configuration(
                "secret file must be a stable regular mode 0600 file",
            ));
        }
    }
    #[cfg(not(unix))]
    return Err(GatewayError::Configuration(
        "secret file permissions require a Unix host",
    ));
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let bytes: SecretSlice<u8> = bytes.into();
    if bytes.expose_secret().len() > maximum {
        return Err(GatewayError::Configuration(
            "secret file exceeds its byte limit",
        ));
    }
    Ok(bytes)
}
