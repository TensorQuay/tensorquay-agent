//! Opt-in Docker isolation and lifecycle contracts.

use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{ReadinessSource, SandboxBackend, SandboxError, docker::DockerBackend};
use tqa_types::ConversationId;

#[path = "support/docker.rs"]
mod support;
use support::{TestResult, run};

struct NeverReady;
#[async_trait]
impl ReadinessSource for NeverReady {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        false
    }
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_001_readiness_cancellation_timeout_and_exit_leave_cleanup_possible() -> TestResult {
    run(async |fixture| {
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(
            fixture
                .backend
                .create(fixture.spec("cancelled", false)?, cancel)
                .await,
            Err(SandboxError::Cancelled)
        );
        let handle = fixture
            .backend
            .create(fixture.spec("pending", false)?, CancellationToken::new())
            .await?;
        assert_eq!(
            fixture
                .backend
                .wait_ready(
                    &handle,
                    &NeverReady,
                    Duration::from_millis(10),
                    CancellationToken::new()
                )
                .await,
            Err(SandboxError::Timeout)
        );
        fixture.backend.destroy(&handle).await?;
        assert_eq!(
            fixture
                .backend
                .wait_ready(
                    &handle,
                    &NeverReady,
                    Duration::from_secs(1),
                    CancellationToken::new()
                )
                .await,
            Err(SandboxError::NotFound)
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_010_start_failure_removes_partial_container_network_volume_and_process() -> TestResult
{
    run(async |fixture| {
        let mut config = fixture.backend_config()?;
        config.test_command_override = Some(vec!["/missing-test-command".into()]);
        let backend = DockerBackend::connect(config).await?;
        assert!(matches!(
            backend
                .create(
                    fixture.spec("start_failure", false)?,
                    CancellationToken::new()
                )
                .await,
            Err(SandboxError::Backend(_))
        ));
        assert!(backend.list().await?.is_empty());
        assert!(
            support::bounded(fixture.docker.inspect_network(&fixture.prefix, None))
                .await
                .is_err()
        );
        assert_eq!(fixture.leftovers().await?, Vec::<String>::new());
        Ok(())
    })
    .await
}

// TEST: SBX-C-014
#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_010_an_image_declaring_a_volume_is_refused_and_leaves_nothing() -> TestResult {
    run(async |fixture| {
        let image = committed_image(
            fixture,
            bollard::models::ContainerConfig {
                volumes: Some(vec!["/data".into()]),
                ..Default::default()
            },
        )
        .await?;
        let mut spec = fixture.spec("declares-volume", false)?;
        spec.image = tqa_sandbox::ImageRef::local_id(image)?;
        let refused = fixture.backend.create(spec, CancellationToken::new()).await;
        assert!(
            matches!(
                refused,
                Err(SandboxError::InvalidSpec { field: "image", .. })
            ),
            "{refused:?}"
        );
        assert_eq!(fixture.leftovers().await?, Vec::<String>::new());
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_001_wait_ready_reports_the_entrypoint_exit_before_its_deadline() -> TestResult {
    run(async |fixture| {
        let image = exiting_image(fixture).await?;
        let config = fixture.backend_config()?;
        let backend = DockerBackend::connect(config).await?;
        let mut spec = fixture.spec("exit-two", false)?;
        spec.image = tqa_sandbox::ImageRef::local_id(image)?;
        let handle = backend.create(spec, CancellationToken::new()).await?;
        let error = tokio::time::timeout(
            Duration::from_secs(3),
            backend.wait_ready(
                &handle,
                &NeverReady,
                Duration::from_mins(1),
                CancellationToken::new(),
            ),
        )
        .await?
        .err()
        .ok_or("exited runtime reported ready")?;
        assert_eq!(error.code(), "sandbox.runtime_exited");
        assert_eq!(
            error.to_string(),
            "runtime exited before registration: code 2"
        );
        backend.destroy(&handle).await?;
        assert!(backend.list().await?.is_empty());
        Ok(())
    })
    .await
}

async fn exiting_image(fixture: &support::Fixture) -> Result<String, Box<dyn std::error::Error>> {
    committed_image(
        fixture,
        bollard::models::ContainerConfig {
            entrypoint: Some(vec!["sh".into(), "-c".into(), "exit 2".into()]),
            cmd: Some(vec![]),
            ..Default::default()
        },
    )
    .await
}

/// A local image: the pinned test image committed with `config`, tagged with the fixture's prefix.
async fn committed_image(
    fixture: &support::Fixture,
    config: bollard::models::ContainerConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    use bollard::{
        models::ContainerCreateBody,
        query_parameters::{CommitContainerOptions, CreateContainerOptions},
    };
    let container = support::bounded(fixture.docker.create_container(
        None::<CreateContainerOptions>,
        ContainerCreateBody {
            image: Some(format!("{}@{}", support::IMAGE, support::DIGEST)),
            labels: Some(std::collections::HashMap::from([(
                "tqa.instance".into(),
                fixture.prefix.clone(),
            )])),
            ..Default::default()
        },
    ))
    .await?;
    let image = support::bounded(fixture.docker.commit_container(
        CommitContainerOptions {
            container: Some(container.id),
            repo: Some(support::IMAGE.into()),
            tag: Some(fixture.prefix.clone()),
            ..Default::default()
        },
        config,
    ))
    .await?;
    Ok(image.id)
}
