use super::Storage;
use crate::GatewayConfig;
use std::os::unix::fs::{PermissionsExt, symlink};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn gw_i_005_storage_enforces_private_directories_and_exact_blob_keys()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let config = GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:0'\npublic_origin='http://localhost'\nmodels_path='models.toml'\n[storage]\nroot='{}'",
        root.path().display()
    ))?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))?;
    assert!(
        Storage::prepare(&config, CancellationToken::new())
            .await
            .is_err()
    );
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let storage = Storage::prepare(&config, CancellationToken::new()).await?;
    let id = uuid::Uuid::now_v7();
    let key = storage.write_blob(id, 1, b"fixture").await?;
    assert_eq!(storage.read_blob(id, 1, &key).await?, b"fixture");
    assert_eq!(
        std::fs::metadata(root.path().join("blobs").join(&key))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(storage.read_blob(id, 1, "../elsewhere").await.is_err());
    let workspace = storage.workspace(id).await?;
    storage.remove_workspace(&workspace).await?;
    symlink(root.path(), &workspace)?;
    assert!(storage.workspace(id).await.is_err());
    Ok(())
}

#[tokio::test]
async fn gw_i_057_block_image_storage_never_touches_a_workspace_tree()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let config = GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:0'\npublic_origin='http://localhost'\nmodels_path='models.toml'\n[storage]\nroot='{}'\n\
         [runtime]\nbackend='firecracker'\n[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'",
        root.path().display(),
        "b".repeat(64)
    ))?;
    let storage = Storage::prepare(&config, CancellationToken::new()).await?;
    assert!(storage.block_images());
    let id = uuid::Uuid::now_v7();
    let image = storage.workspace_image(id).await?;
    assert_eq!(
        image,
        root.path().join("workspaces").join(format!("{id}.ext4"))
    );
    assert_eq!(storage.image_allocation(id).await, None, "not created yet");
    std::fs::write(&image, vec![1_u8; 8192])?;
    assert!(
        storage
            .image_allocation(id)
            .await
            .is_some_and(|bytes| bytes >= 8192)
    );
    // A same-named directory is never followed or removed on this backend; only the image goes.
    std::fs::create_dir(storage.workspace_path(id))?;
    std::fs::write(storage.workspace_path(id).join("keep"), b"x")?;
    storage.remove_workspace_tree(id).await?;
    assert!(!image.exists());
    assert!(storage.workspace_path(id).join("keep").exists());
    storage.remove_workspace_tree(id).await?;
    Ok(())
}
