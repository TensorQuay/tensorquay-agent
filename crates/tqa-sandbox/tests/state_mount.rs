//! SBX-C-016: state is a separate writable host directory, never part of the workspace.

use std::{error::Error, path::Path, time::SystemTime};
use tqa_sandbox::{
    BackendKind, SandboxError, StateMount, StateSource, WorkspaceSource, docker::container_config,
};

use docker_support::specifications as support;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn sbx_c_016_state_is_mounted_separately_and_reserved_in_the_environment() -> TestResult {
    let spec = support::spec()?;
    let body = container_config(
        &spec,
        "tqa-unit",
        SystemTime::UNIX_EPOCH,
        Path::new("/storage/sandboxes/unit/token"),
    )?;
    let env = body.env.ok_or("missing environment")?;
    assert!(
        env.iter()
            .any(|value| value == "TQA_STATE_DIR=/run/tqa/state")
    );
    let mounts = body
        .host_config
        .ok_or("missing host config")?
        .mounts
        .ok_or("missing mounts")?;
    assert_eq!(mounts.len(), 3);
    let state = mounts
        .iter()
        .find(|mount| mount.target.as_deref() == Some("/run/tqa/state"))
        .ok_or("missing state mount")?;
    assert_eq!(state.read_only, Some(false));
    assert_eq!(
        state.source.as_deref(),
        spec.state.host_directory().and_then(Path::to_str)
    );
    let mut invalid = spec;
    invalid
        .env
        .insert("TQA_STATE_DIR".into(), "/workspace".into());
    assert!(invalid.validate(BackendKind::Docker).is_err());
    Ok(())
}

#[test]
fn sbx_c_016_state_paths_are_absolute_and_disjoint_from_the_workspace() -> TestResult {
    for path in [
        "/",
        "relative",
        "/state/../other",
        "/workspace",
        "/workspace/private",
        "/",
    ] {
        let mut spec = support::spec()?;
        spec.workspace.source = WorkspaceSource::HostDirectory("/workspace".into());
        spec.state = StateMount::directory(path);
        assert!(spec.validate(BackendKind::Docker).is_err(), "{path}");
    }
    let mut spec = support::spec()?;
    spec.state = StateMount::directory("/state");
    spec.workspace.source = WorkspaceSource::HostDirectory("/state/workspace".into());
    assert!(spec.validate(BackendKind::Docker).is_err());
    Ok(())
}

#[test]
fn sbx_d_005_docker_refuses_a_block_image_state_before_any_engine_call() -> TestResult {
    let mut spec = support::spec()?;
    spec.state = StateMount {
        source: StateSource::BlockImage("/state/conversation_1.ext4".into()),
    };
    assert!(matches!(
        spec.validate(BackendKind::Docker),
        Err(SandboxError::InvalidSpec { field: "state", .. })
    ));
    let token = Path::new("/storage/sandboxes/unit/token");
    assert!(container_config(&spec, "tqa-unit", SystemTime::UNIX_EPOCH, token).is_err());
    assert!(spec.validate(BackendKind::Firecracker).is_ok());
    spec.state = StateMount {
        source: StateSource::BlockImage("relative.ext4".into()),
    };
    assert!(spec.validate(BackendKind::Firecracker).is_err());
    Ok(())
}

#[path = "support/docker.rs"]
mod docker_support;

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_016_state_aliases_cannot_expose_workspace_or_credential_storage() -> TestResult {
    use tqa_sandbox::SandboxBackend;
    docker_support::run(async |fixture| {
        let mut spec = fixture.spec("state-overlap", true)?;
        spec.state = StateMount::directory(fixture.storage.path().join("sandboxes"));
        assert!(
            fixture
                .backend
                .create(spec, tokio_util::sync::CancellationToken::new())
                .await
                .is_err()
        );
        #[cfg(unix)]
        {
            let alias = fixture.state.path().join("workspace-alias");
            tokio::fs::symlink(fixture.workspace.path(), &alias).await?;
            let mut spec = fixture.spec("state-alias", true)?;
            spec.state = StateMount::directory(alias);
            assert!(
                fixture
                    .backend
                    .create(spec, tokio_util::sync::CancellationToken::new())
                    .await
                    .is_err()
            );
        }
        assert!(fixture.backend.list().await?.is_empty());
        Ok(())
    })
    .await
}
