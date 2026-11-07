use super::{SecretFiles, files};
use std::os::unix::fs::PermissionsExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// TEST: GW-I-015
#[tokio::test]
async fn gw_i_029_install_key_is_private_persistent_and_exactly_32_bytes() -> TestResult {
    use secrecy::ExposeSecret;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("install.key");
    let first = files::load_key(&path).await?;
    let second = files::load_key(&path).await?;
    assert_eq!(first.expose_secret().len(), 32);
    assert_eq!(first.expose_secret(), second.expose_secret());
    assert_eq!(
        tokio::fs::metadata(&path).await?.permissions().mode() & 0o777,
        0o600
    );
    for mode in [0o400, 0o640, 0o644] {
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).await?;
        assert!(files::load_key(&path).await.is_err());
    }
    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await?;
    tokio::fs::write(&path, b"too-short").await?;
    assert!(files::load_key(&path).await.is_err());
    let alias = directory.path().join("alias.key");
    tokio::fs::symlink(&path, &alias).await?;
    assert!(files::load_key(&alias).await.is_err());
    assert!(files::load_key(directory.path()).await.is_err());
    assert!(
        files::load_key(&directory.path().join("absent/key"))
            .await
            .is_err()
    );
    for path in ["relative", "/", "/state/../key"] {
        assert!(SecretFiles::new(path.into(), None).is_err());
    }
    Ok(())
}
