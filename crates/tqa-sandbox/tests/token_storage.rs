//! SBX-C-005 and SBX-C-016: credential storage is private and outside the workspace.

use std::{
    error::Error,
    path::Path,
    time::{Duration, SystemTime},
};
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{SandboxBackend, WorkspaceSource, docker::container_config};

#[path = "support/docker.rs"]
mod support;
use support::specifications;

#[test]
fn sbx_c_005_credential_paths_reject_traversal() -> Result<(), Box<dyn Error>> {
    let spec = specifications::spec()?;
    for file in ["relative/token", "/data/../token", "/data/not-token"] {
        assert!(
            container_config(&spec, "tqa-unit", SystemTime::UNIX_EPOCH, Path::new(file)).is_err()
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn sbx_c_005_non_utf8_credential_path_is_rejected() -> Result<(), Box<dyn Error>> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
    let path = PathBuf::from(OsString::from_vec(b"/storage-\xff/token".to_vec()));
    assert!(
        container_config(
            &specifications::spec()?,
            "tqa-unit",
            SystemTime::UNIX_EPOCH,
            &path
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_016_workspace_cannot_expose_credential_storage() -> support::TestResult {
    support::run(async |fixture| {
        let mut spec = fixture.spec("overlap", true)?;
        spec.workspace.source = WorkspaceSource::HostDirectory(fixture.storage.path().to_owned());
        assert!(
            fixture
                .backend
                .create(spec, CancellationToken::new())
                .await
                .is_err()
        );
        assert!(fixture.backend.list().await?.is_empty());
        let mut entries = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::fs::read_dir(fixture.storage.path().join("sandboxes")),
        )
        .await??;
        assert!(
            tokio::time::timeout(Duration::from_secs(5), entries.next_entry())
                .await??
                .is_none()
        );
        Ok(())
    })
    .await
}
