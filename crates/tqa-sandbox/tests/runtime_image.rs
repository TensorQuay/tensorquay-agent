//! Admission of the built runtime image through the production sandbox backend.

use std::collections::BTreeMap;

use tokio_util::sync::CancellationToken;
use tqa_sandbox::{ByteSize, CpuLimit, ImageRef, SandboxBackend, docker::DockerBackend};

#[path = "support/docker.rs"]
mod support;
use support::{TestResult, bounded, run};

#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and a built runtime image"]
async fn sbx_c_017_runtime_image_starts_with_the_gateway_environment() -> TestResult {
    run(async |fixture| {
        let selected = match std::env::var("TQA_RUNTIME_TEST_IMAGE") {
            Ok(value) if !value.is_empty() => value,
            Err(std::env::VarError::NotPresent) => "tqa-runtime:workstation".into(),
            _ => return Err("TQA_RUNTIME_TEST_IMAGE must be nonempty UTF-8".into()),
        };
        let image = bounded(fixture.docker.inspect_image(&selected))
            .await?
            .id
            .ok_or("runtime image ID missing")?;
        let mut spec = fixture.spec("runtime-image", true)?;
        spec.image = ImageRef::local_id(&image)?;
        assert!(fixture.backend.image_present(&spec.image).await?);
        assert_eq!(
            fixture.backend.check_runtime_image(&spec.image).await?,
            tqa_types::PROTOCOL_REVISION
        );
        spec.env = BTreeMap::from([
            (
                "PATH".into(),
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
            ),
            ("HOME".into(), "/tmp".into()),
        ]);
        spec.gateway_url = "http://gateway.invalid".parse()?;
        spec.cpu = CpuLimit::new(1000)?;
        spec.memory = ByteSize::new(512 * 1024 * 1024)?;
        spec.pids = 256;
        let config = fixture.backend_config()?;
        let backend = DockerBackend::connect(config).await?;
        let handle = backend.create(spec, CancellationToken::new()).await?;
        let actual = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
        assert_eq!(actual.image.as_deref(), Some(image.as_str()));
        assert_eq!(
            actual.state.as_ref().and_then(|state| state.running),
            Some(true),
            "{:?}",
            actual.state
        );
        assert_eq!(
            fixture
                .exec(&handle.id, &["sh", "-c", "id -u; printf '%s\\n' \"$HOME\""])
                .await?,
            (0, "1000\n/tmp\n".into())
        );
        backend.destroy(&handle).await?;
        assert!(backend.list().await?.is_empty());
        Ok(())
    })
    .await
}
