//! SBX-F-029: one helper container makes a new task's missing images.
use std::{error::Error, fs, sync::Arc};

use bollard::models::ContainerCreateBody;
use tokio_util::sync::CancellationToken;

use super::tests::{FakeEngine, backend, is_image_helper};
use crate::SandboxBackend;

/// The image helpers' commands among the engine's helper runs.
pub(super) fn image_helpers(helpers: &[ContainerCreateBody]) -> Vec<Vec<String>> {
    helpers
        .iter()
        .filter_map(|body| body.cmd.clone())
        .filter(|cmd| is_image_helper(cmd))
        .collect()
}

// TEST: SBX-F-029
#[tokio::test]
async fn sbx_f_029_a_new_task_makes_both_images_in_one_helper() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    backend.create(spec, CancellationToken::new()).await?;
    let helpers = engine.helpers.lock().map_err(|_| "lock")?.clone();
    let mkfs = image_helpers(&helpers);
    assert_eq!(mkfs.len(), 1, "one helper for both images");
    assert_eq!(
        mkfs[0].len(),
        4 + 4,
        "the script, its $0, then two (file, size) pairs"
    );
    for image in ["workspaces/workspace_1.ext4", "state/conversation_1.ext4"] {
        assert_eq!(fs::read(root.path().join(image))?, b"ext4", "{image}");
    }
    // Each image is made under a temporary name and moved into place only when mkfs succeeded.
    assert!(mkfs[0][2].contains(".tmp") && mkfs[0][2].contains("mv "));
    Ok(())
}

// TEST: SBX-F-029
#[tokio::test]
async fn sbx_f_029_only_the_missing_images_are_made() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    // A task in a workspace that already exists makes only its state image.
    fs::write(root.path().join("workspaces/workspace_1.ext4"), b"kept")?;
    backend
        .create(spec.clone(), CancellationToken::new())
        .await?;
    let mkfs = image_helpers(&engine.helpers.lock().map_err(|_| "lock")?.clone());
    assert_eq!(mkfs.len(), 1);
    assert_eq!(mkfs[0].len(), 4 + 2, "one (file, size) pair");
    assert!(mkfs[0][4].ends_with("/conversation_1.ext4"));
    assert_eq!(
        fs::read(root.path().join("workspaces/workspace_1.ext4"))?,
        b"kept"
    );
    Ok(())
}

// TEST: SBX-F-029
#[tokio::test]
async fn sbx_f_029_file_names_are_arguments_never_part_of_the_script() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    backend.create(spec, CancellationToken::new()).await?;
    let mkfs = image_helpers(&engine.helpers.lock().map_err(|_| "lock")?.clone());
    assert_eq!(mkfs.len(), 1, "one image helper");
    assert_eq!(mkfs[0][1], "-c");
    for name in ["workspace_1", "conversation_1"] {
        assert!(!mkfs[0][2].contains(name), "{name} is in the script");
    }
    Ok(())
}

// TEST: SBX-F-029
#[tokio::test]
async fn sbx_f_029_a_failed_image_helper_fails_the_create_and_leaves_no_image()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine {
        fail_mkfs: true,
        ..FakeEngine::default()
    });
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    assert!(
        backend
            .create(spec, CancellationToken::new())
            .await
            .is_err()
    );
    for image in ["workspaces/workspace_1.ext4", "state/conversation_1.ext4"] {
        assert!(!root.path().join(image).exists(), "{image}");
    }
    assert!(engine.containers.lock().map_err(|_| "lock")?.is_empty());
    Ok(())
}
