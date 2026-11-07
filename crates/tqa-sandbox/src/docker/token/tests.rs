use super::{prepare, remove_missing_ok, validate_id};
use std::{error::Error, io, time::Duration};

#[test]
fn sbx_c_005_token_directory_identifiers_cannot_escape_storage() {
    for value in [
        String::new(),
        "x".repeat(201),
        "../outside".into(),
        "/absolute".into(),
        "dot.name".into(),
    ] {
        assert!(validate_id(&value).is_err());
    }
    assert!(validate_id("sandbox_123-safe").is_ok());
    assert!(remove_missing_ok(Ok(())).is_ok());
    assert!(remove_missing_ok(Err(io::ErrorKind::NotFound.into())).is_ok());
    assert!(remove_missing_ok(Err(io::ErrorKind::PermissionDenied.into())).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn sbx_c_005_storage_rejects_symlinks_and_public_permissions() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    tokio::time::timeout(Duration::from_secs(5), async {
        let root = tempfile::tempdir()?;
        let canonical = prepare(root.path()).await?;
        assert_eq!(prepare(root.path()).await?, canonical);
        let private = canonical.join("sandboxes");
        let mode = tokio::fs::metadata(&private).await?.permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        tokio::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755)).await?;
        assert!(prepare(root.path()).await.is_err());
        tokio::fs::remove_dir(&private).await?;
        let external = tempfile::tempdir()?;
        tokio::fs::symlink(external.path(), &private).await?;
        assert!(prepare(root.path()).await.is_err());
        assert!(prepare(&root.path().join("missing")).await.is_err());
        let file = root.path().join("file");
        tokio::fs::write(&file, b"fixture").await?;
        assert!(prepare(&file).await.is_err());
        Ok::<_, Box<dyn Error>>(())
    })
    .await?
}
